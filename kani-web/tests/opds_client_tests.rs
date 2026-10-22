#![allow(clippy::unwrap_used)]

//! OPDS **client** tests — the consumption side.
//!
//! Every other OPDS test checks one endpoint in isolation and asserts on the
//! feed with `text.contains(...)`. That leaves the thing the feed exists for
//! untested: a third-party reader parses the XML and *navigates by the links it
//! finds*. A feed can satisfy every substring assertion and still be unusable —
//! malformed XML, or an advertised `href` that 404s.
//!
//! These tests behave like that reader: parse each feed as real XML, follow the
//! hrefs it advertises, and fetch what the PSE template promises.

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{
    build_test_app_with_opds, create_admin, insert_chapter, insert_manga, insert_source, login,
    test_state,
};
use http_body_util::BodyExt as _;
use kani_web::state::AppState;
use quick_xml::events::Event;
use std::io::Write as _;
use tower::ServiceExt;

fn make_png(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img = image::ImageBuffer::from_pixel(w, h, image::Rgb([r, g, b]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

fn write_cbz(path: &std::path::Path, pages: &[Vec<u8>]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    for (i, data) in pages.iter().enumerate() {
        w.start_file(format!("{:04}.png", i + 1), opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
}

#[derive(Debug)]
struct Link {
    rel: String,
    href: String,
    /// `pse:count` when present — the page count a reader paginates over.
    pse_count: Option<u32>,
}

#[derive(Debug, Default)]
struct Feed {
    title: Option<String>,
    links: Vec<Link>,
    entry_titles: Vec<String>,
}

/// Parse a feed the way a reader must: strictly. A malformed document is an
/// error here, which is the whole point — `contains()` cannot tell the
/// difference between valid XML and a broken document that happens to include
/// the right bytes.
fn parse_feed(xml: &str) -> Feed {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut feed = Feed::default();
    let mut buf = Vec::new();
    let mut in_entry = false;
    let mut capture_title = false;
    let mut current_title = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => panic!("the feed is not well-formed XML, no reader could use it: {e}"),
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "entry" => in_entry = true,
                    "title" => {
                        capture_title = true;
                        current_title.clear();
                    }
                    "link" => {
                        let mut rel = String::new();
                        let mut href = String::new();
                        let mut pse_count = None;
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val = attr.unescape_value().unwrap_or_default().to_string();
                            match key.as_str() {
                                "rel" => rel = val,
                                "href" => href = val,
                                "pse:count" => pse_count = val.parse().ok(),
                                _ => {}
                            }
                        }
                        feed.links.push(Link {
                            rel,
                            href,
                            pse_count,
                        });
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "entry" => in_entry = false,
                    "title" => {
                        capture_title = false;
                        let text = std::mem::take(&mut current_title);
                        if in_entry {
                            feed.entry_titles.push(text);
                        } else if feed.title.is_none() {
                            feed.title = Some(text);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if capture_title => {
                current_title.push_str(&t.xml_content().unwrap_or_default());
            }
            Ok(Event::GeneralRef(r)) if capture_title => {
                if let Some(ch) = r.resolve_char_ref().ok().flatten() {
                    current_title.push(ch);
                } else {
                    let name = String::from_utf8_lossy(r.as_ref()).to_string();
                    current_title.push_str(match name.as_str() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        other => panic!("feed used an entity no reader must guess at: &{other};"),
                    });
                }
            }
            _ => {}
        }
        buf.clear();
    }
    feed
}

fn authed(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("Cookie", cookie)
        .body(Body::empty())
        .unwrap()
}

/// Strip the absolute prefix the feed advertises so the href can be fed back
/// into the test router, exactly as a reader would dereference it.
fn to_path(href: &str) -> String {
    href.strip_prefix("http://localhost:8242")
        .unwrap_or(href)
        .to_string()
}

async fn get_text(app: &axum::Router, uri: &str, cookie: &str) -> (StatusCode, String) {
    let res = app.clone().oneshot(authed(uri, cookie)).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn seed_downloaded_chapter(state: &AppState, manga_title: &str) -> i64 {
    let src = insert_source(&state.db, "src").await;
    let manga = insert_manga(&state.db, src, "m1", manga_title).await;
    let ch = insert_chapter(&state.db, manga, "c1", 1.0).await;
    sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
        .bind(ch)
        .execute(&state.db)
        .await
        .unwrap();

    let library_path = state.service.settings.read().await.library_path.clone();
    let manga_dir = library_path.join(format!(
        "{} - {}",
        kani_core::utilities::sanitize_filename(manga_title),
        manga.0
    ));
    std::fs::create_dir_all(&manga_dir).unwrap();
    let pages = vec![
        make_png(4, 6, 10, 20, 30),
        make_png(4, 8, 40, 50, 60),
        make_png(8, 8, 70, 80, 90),
    ];
    let info = state.service.chapter_cbz_path(ch).await.unwrap();
    write_cbz(&info.path, &pages);
    manga.0
}

#[tokio::test]
async fn a_client_can_navigate_from_the_root_feed_to_every_page() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let manga_id = seed_downloaded_chapter(&state, "Test Manga").await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, u, p).await;

    let (status, xml) = get_text(&app, "/opds", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let root = parse_feed(&xml);
    let catalogue = root
        .links
        .iter()
        .find(|l| l.href.contains("/catalogue"))
        .expect("the root feed must advertise a catalogue link");

    let (status, xml) = get_text(&app, &to_path(&catalogue.href), &cookie).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the advertised catalogue link must resolve"
    );
    let cat = parse_feed(&xml);
    assert!(
        cat.entry_titles.iter().any(|t| t == "Test Manga"),
        "the catalogue lists the manga, got {:?}",
        cat.entry_titles
    );

    let (status, xml) = get_text(&app, &format!("/opds/manga/{manga_id}"), &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let manga_feed = parse_feed(&xml);
    let chapter_link = manga_feed
        .links
        .iter()
        .find(|l| l.rel == "subsection")
        .expect("the manga feed must advertise a subsection link to the chapter");

    let (status, xml) = get_text(&app, &to_path(&chapter_link.href), &cookie).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the advertised chapter link must resolve"
    );
    let chapter_feed = parse_feed(&xml);
    let stream = chapter_feed
        .links
        .iter()
        .find(|l| l.rel == "http://vaemendis.net/opds-pse/stream")
        .expect("the chapter feed must advertise a PSE stream link");
    assert!(
        stream.pse_count.is_some(),
        "the PSE stream link must carry pse:count so a reader knows how many pages to expect"
    );
    let count = stream.pse_count.unwrap();
    assert_eq!(count, 3, "pse:count must match the pages in the archive");

    for page in 1..=count {
        let href = to_path(&stream.href).replace("{pageNumber}", &page.to_string());
        let res = app.clone().oneshot(authed(&href, &cookie)).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "page {page} of {count} was advertised but did not resolve: {href}"
        );
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        assert!(
            bytes.starts_with(b"\x89PNG"),
            "page {page} must be served as the image it is"
        );
    }

    let past = count + 1;
    let href = to_path(&stream.href).replace("{pageNumber}", &past.to_string());
    let res = app.clone().oneshot(authed(&href, &cookie)).await.unwrap();
    assert!(
        !res.status().is_success(),
        "page {past} is past pse:count ({count}) and must not resolve"
    );
}

#[tokio::test]
async fn a_title_with_xml_metacharacters_still_produces_a_parseable_feed() {
    const NASTY: &str = r#"Fullmetal & <Alchemist> "Brotherhood" 'ova' -- <![CDATA[x]]>"#;

    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let manga_id = seed_downloaded_chapter(&state, NASTY).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, u, p).await;

    for uri in ["/opds/catalogue", &format!("/opds/manga/{manga_id}")] {
        let (status, xml) = get_text(&app, uri, &cookie).await;
        assert_eq!(status, StatusCode::OK);
        let feed = parse_feed(&xml);
        let seen = feed
            .entry_titles
            .iter()
            .chain(feed.title.iter())
            .any(|t| t == NASTY);
        assert!(
            seen,
            "the title must round-trip through escaping intact in {uri}, got \
             titles {:?} / {:?}",
            feed.title, feed.entry_titles
        );
    }
}
