#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::Path;

fn write_cbz(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts: zip::write::FileOptions<'_, ()> = zip::write::SimpleFileOptions::default();
    for i in 0..2u8 {
        zip.start_file(format!("{:04}.png", i + 1), opts).unwrap();
        zip.write_all(&[i, i.wrapping_add(7), 0xAB, 0xCD]).unwrap();
    }
    zip.finish().unwrap();
}

fn build_archive(dir: &Path) -> std::path::PathBuf {
    let src = dir.join("src.cbz");
    write_cbz(&src);
    let series = vec![kani_core::archive::ArchiveSeries {
        slug: "fixture-1".to_string(),
        metadata_json: "{\"title\":\"Fixture\"}".to_string(),
        cover: None,
        chapters: vec![kani_core::archive::ArchiveChapter {
            number_prefix: "0001".to_string(),
            slug: "c1".to_string(),
            manifest: kani_core::manifest::manifest_for_cbz(&src).unwrap(),
            cbz_path: src,
        }],
    }];
    let out = dir.join("kani-archive");
    kani_core::archive::write_archive(&series, &out, true, |_, _| {}).unwrap();
    out
}

#[test]
fn verify_succeeds_on_an_intact_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = build_archive(tmp.path());
    assert!(kani_cli::commands::archive::verify(&archive).is_ok());
}

#[test]
fn verify_fails_on_a_corrupted_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = build_archive(tmp.path());

    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(archive.join("ARCHIVE.json")).unwrap())
            .unwrap();
    let cbz = index["series"][0]["chapters"][0]["cbz"].as_str().unwrap();
    let victim = archive.join(cbz);
    let mut bytes = std::fs::read(&victim).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&victim, &bytes).unwrap();

    // `verify` also errors when the archive cannot be read at all, so asserting
    // only is_err() would pass if the corruption merely made the CBZ unopenable
    // and the hash comparison never ran.
    let error = kani_cli::commands::archive::verify(&archive)
        .expect_err("a silent pass on a damaged archive is worse than no check at all")
        .to_string();
    assert!(
        error.contains("failed verification"),
        "the hash comparison must be what rejects it, got: {error}"
    );
}

#[test]
fn verify_fails_when_there_is_no_index() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(
        kani_cli::commands::archive::verify(tmp.path()).is_err(),
        "an empty directory must not read as a clean archive"
    );
}

#[test]
fn manifest_reads_a_cbz_off_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let cbz = tmp.path().join("x.cbz");
    write_cbz(&cbz);
    assert!(kani_cli::commands::archive::manifest(&cbz).is_ok());
    assert!(kani_cli::commands::archive::manifest(tmp.path()).is_err());
}
