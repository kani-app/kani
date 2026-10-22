#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, test_service};
use kani_app::ids::{ChapterId, MangaId};
use kani_app::service::archive::ArchiveSpec;
use std::io::Write;
use std::path::Path;

fn png_bytes(shade: u8) -> Vec<u8> {
    let mut img = image::GrayImage::new(16, 24);
    for (x, _y, p) in img.enumerate_pixels_mut() {
        *p = image::Luma([shade.wrapping_add((x % 255) as u8)]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn write_cbz(path: &Path, shades: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    for (i, s) in shades.iter().enumerate() {
        zip.start_file(format!("{:04}.png", i + 1), opts).unwrap();
        zip.write_all(&png_bytes(*s)).unwrap();
    }
    zip.finish().unwrap();
}

async fn seed(
    svc: &kani_app::service::AppService,
    title: &str,
    record_manifest: bool,
) -> (MangaId, ChapterId) {
    let src = insert_source(&svc.db, &format!("src-{title}")).await;
    let manga = insert_manga(&svc.db, src, "m1", title).await;
    let chapter = insert_chapter(&svc.db, manga, "c1", 1.0).await;
    sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
        .bind(chapter)
        .execute(&svc.db)
        .await
        .unwrap();

    let library = { svc.settings.read().await.library_path.clone() };
    std::fs::create_dir_all(library.join(format!(
        "{} - {}",
        kani_core::utilities::sanitize_filename(title),
        manga.0
    )))
    .unwrap();

    let cbz = svc.chapter_cbz_path(chapter).await.unwrap().path;
    write_cbz(&cbz, &[11, 77]);
    if record_manifest {
        svc.record_chapter_manifest(chapter, cbz).await;
    }
    (manga, chapter)
}

fn find_archive_root(library: &Path) -> std::path::PathBuf {
    let archives = library.join("_archives");
    let stamp = std::fs::read_dir(&archives)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    stamp.join("kani-archive")
}

#[tokio::test]
async fn an_export_verifies_with_no_database_present() {
    let svc = test_service().await;
    seed(&svc, "Alpha", true).await;
    let library = { svc.settings.read().await.library_path.clone() };

    let report = svc
        .export_archive(&ArchiveSpec::default(), None)
        .await
        .unwrap();
    assert_eq!(report.series_count, 1);
    assert_eq!(report.chapter_count, 1);

    let root = find_archive_root(&library);
    let v = kani_core::archive::verify_archive(&root).unwrap();
    assert!(v.is_ok(), "{:?}", v.failures);
}

#[tokio::test]
async fn the_layout_matches_the_documented_format() {
    let svc = test_service().await;
    seed(&svc, "Beta", true).await;
    let library = { svc.settings.read().await.library_path.clone() };
    svc.export_archive(&ArchiveSpec::default(), None)
        .await
        .unwrap();
    let root = find_archive_root(&library);

    assert!(root.join("ARCHIVE.json").exists());
    assert!(root.join("README.html").exists(), "viewer ships by default");

    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("ARCHIVE.json")).unwrap()).unwrap();
    let series_slug = index["series"][0]["slug"].as_str().unwrap().to_string();
    let series_dir = root.join("series").join(&series_slug);
    assert!(series_dir.join("series.json").exists());

    let cbz = index["series"][0]["chapters"][0]["cbz"].as_str().unwrap();
    assert!(cbz.ends_with(".cbz"), "{cbz}");
    assert!(root.join(cbz).exists());
    assert!(
        root.join(format!("{cbz}.manifest.json")).exists(),
        "every chapter carries its manifest beside it"
    );
}

#[tokio::test]
async fn the_sidecar_manifest_matches_the_stored_one() {
    let svc = test_service().await;
    let (_, chapter) = seed(&svc, "Gamma", true).await;
    let library = { svc.settings.read().await.library_path.clone() };
    svc.export_archive(&ArchiveSpec::default(), None)
        .await
        .unwrap();
    let root = find_archive_root(&library);

    let stored: String = sqlx::query_scalar("SELECT manifest_json FROM chapters WHERE id = ?")
        .bind(chapter.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let stored: kani_core::manifest::ChapterManifest = serde_json::from_str(&stored).unwrap();

    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("ARCHIVE.json")).unwrap()).unwrap();
    let man_rel = index["series"][0]["chapters"][0]["manifest"]
        .as_str()
        .unwrap();
    let exported: kani_core::manifest::ChapterManifest =
        serde_json::from_str(&std::fs::read_to_string(root.join(man_rel)).unwrap()).unwrap();

    assert_eq!(
        exported, stored,
        "the export must reuse what download captured, not re-derive it"
    );
}

#[tokio::test]
async fn a_chapter_without_a_stored_manifest_is_still_exported() {
    let svc = test_service().await;
    seed(&svc, "Delta", false).await;
    let library = { svc.settings.read().await.library_path.clone() };

    let report = svc
        .export_archive(&ArchiveSpec::default(), None)
        .await
        .unwrap();
    assert_eq!(
        report.chapter_count, 1,
        "a pre-backfill row must be recomputed, not skipped — otherwise the \
         export silently omits chapters"
    );

    let root = find_archive_root(&library);
    assert!(kani_core::archive::verify_archive(&root).unwrap().is_ok());
}

#[tokio::test]
async fn scoping_by_manga_excludes_the_rest() {
    let svc = test_service().await;
    let (keep, _) = seed(&svc, "Kept", true).await;
    seed(&svc, "Dropped", true).await;
    let library = { svc.settings.read().await.library_path.clone() };

    let report = svc
        .export_archive(
            &ArchiveSpec {
                manga_ids: Some(vec![keep]),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();

    assert_eq!(report.series_count, 1);
    let root = find_archive_root(&library);
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("ARCHIVE.json")).unwrap()).unwrap();
    assert_eq!(index["series"].as_array().unwrap().len(), 1);
    assert!(
        index["series"][0]["slug"]
            .as_str()
            .unwrap()
            .starts_with("kept")
    );
}

#[tokio::test]
async fn a_zipped_export_reports_the_zip_as_its_root() {
    let svc = test_service().await;
    seed(&svc, "Zipped", true).await;

    let report = svc
        .export_archive(
            &ArchiveSpec {
                zip: true,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();

    assert!(report.zipped);
    assert!(report.root.ends_with(".zip"), "{}", report.root);
    assert!(Path::new(&report.root).exists());
    assert!(report.total_bytes > 0);
}

#[tokio::test]
async fn a_produced_zip_resolves_but_an_outside_path_does_not() {
    let svc = test_service().await;
    seed(&svc, "Guarded", true).await;
    let report = svc
        .export_archive(
            &ArchiveSpec {
                zip: true,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();

    assert!(svc.archive_zip_path(&report.root).await.is_ok());
    assert!(
        svc.archive_zip_path("/etc/passwd").await.is_err(),
        "the download path must be pinned under _archives"
    );
}
