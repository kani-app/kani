use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use std::io::Cursor;
use time::OffsetDateTime;

use super::AppService;
use crate::error::{Result, ServiceError};
use crate::ids::{ChapterId, MangaId, UserId};
use kani_shared::types::ChapterSortOrder;

impl AppService {
    /// Root OPDS navigation feed — links to catalogue and search.
    pub fn opds_root_feed(&self, base_url: &str) -> String {
        let updated = now_rfc3339();
        let mut w = Utf8Writer::new();
        w.decl();
        w.open(
            "feed",
            &[
                ("xmlns", "http://www.w3.org/2005/Atom"),
                ("xmlns:opds", "http://opds-spec.org/2010/catalog"),
            ],
        );
        w.leaf("id", &[], "urn:kani:root");
        w.leaf("title", &[], "Kani Manga Reader");
        w.leaf("updated", &[], &updated);
        w.self_close(
            "link",
            &[
                ("rel", "self"),
                ("href", &format!("{base_url}/opds")),
                ("type", ATOM_PROFILE),
            ],
        );
        w.self_close(
            "link",
            &[
                ("rel", "start"),
                ("href", &format!("{base_url}/opds")),
                ("type", ATOM_PROFILE),
            ],
        );
        w.self_close(
            "link",
            &[
                ("rel", "search"),
                ("href", &format!("{base_url}/opds/search")),
                ("type", "application/opensearchdescription+xml"),
            ],
        );
        nav_entry(
            &mut w,
            "urn:kani:catalogue",
            "Library",
            "Browse the full manga library",
            &updated,
            &format!("{base_url}/opds/catalogue"),
        );
        w.close("feed");
        w.finish()
    }

    /// Paginated acquisition feed of the library. `page` is 1-based.
    pub async fn opds_catalogue_feed(
        &self,
        page: i32,
        page_size: i32,
        search: Option<String>,
        user_id: UserId,
        base_url: &str,
    ) -> Result<String> {
        let (manga_list, has_next, _) = self
            .get_library_filtered(
                user_id,
                &crate::service::library::LibraryFilter {
                    page,
                    page_size,
                    search: search.clone(),
                    ..Default::default()
                },
            )
            .await?;

        let updated = now_rfc3339();
        let mut w = Utf8Writer::new();
        w.decl();
        w.open(
            "feed",
            &[
                ("xmlns", "http://www.w3.org/2005/Atom"),
                ("xmlns:opds", "http://opds-spec.org/2010/catalog"),
            ],
        );
        w.leaf("id", &[], "urn:kani:catalogue");
        w.leaf("title", &[], "Kani Library");
        w.leaf("updated", &[], &updated);
        w.self_close(
            "link",
            &[
                ("rel", "self"),
                (
                    "href",
                    &catalogue_href(base_url, page, page_size, search.as_deref()),
                ),
                ("type", ATOM_PROFILE),
            ],
        );
        w.self_close(
            "link",
            &[
                ("rel", "start"),
                ("href", &format!("{base_url}/opds")),
                ("type", ATOM_PROFILE),
            ],
        );
        if page > 1 {
            w.self_close(
                "link",
                &[
                    ("rel", "previous"),
                    (
                        "href",
                        &catalogue_href(base_url, page - 1, page_size, search.as_deref()),
                    ),
                    ("type", ATOM_PROFILE),
                ],
            );
        }
        if has_next {
            w.self_close(
                "link",
                &[
                    ("rel", "next"),
                    (
                        "href",
                        &catalogue_href(base_url, page + 1, page_size, search.as_deref()),
                    ),
                    ("type", ATOM_PROFILE),
                ],
            );
        }

        for m in &manga_list {
            w.open("entry", &[]);
            w.leaf("id", &[], &format!("urn:kani:manga:{}", m.id));
            w.leaf("title", &[], &m.name);
            w.leaf("updated", &[], &updated);
            w.self_close(
                "link",
                &[
                    ("rel", "http://opds-spec.org/image"),
                    ("href", &format!("{base_url}/rest/manga/{}/cover", m.id)),
                    ("type", "image/jpeg"),
                ],
            );
            w.self_close(
                "link",
                &[
                    ("rel", "subsection"),
                    ("href", &format!("{base_url}/opds/manga/{}", m.id)),
                    ("type", ATOM_PROFILE),
                ],
            );
            w.close("entry");
        }

        w.close("feed");
        Ok(w.finish())
    }

    /// Acquisition feed of downloaded chapters for one manga.
    pub async fn opds_manga_feed(
        &self,
        manga_id: MangaId,
        user_id: UserId,
        base_url: &str,
    ) -> Result<String> {
        let manga = self.get_manga_by_id(manga_id).await?;

        let (chapters, _, _, _) = self
            .get_local_chapters(
                manga_id,
                1,
                500,
                ChapterSortOrder::default(),
                user_id,
                Some(true),
                None,
                None,
                None,
            )
            .await?;

        let updated = now_rfc3339();
        let mut w = Utf8Writer::new();
        w.decl();
        w.open(
            "feed",
            &[
                ("xmlns", "http://www.w3.org/2005/Atom"),
                ("xmlns:opds", "http://opds-spec.org/2010/catalog"),
                ("xmlns:pse", "http://vaemendis.net/opds-pse/2017"),
            ],
        );
        w.leaf("id", &[], &format!("urn:kani:manga:{manga_id}"));
        w.leaf("title", &[], &manga.name);
        w.leaf("updated", &[], &updated);
        w.self_close(
            "link",
            &[
                ("rel", "self"),
                ("href", &format!("{base_url}/opds/manga/{manga_id}")),
                ("type", ATOM_PROFILE),
            ],
        );
        w.self_close(
            "link",
            &[
                ("rel", "start"),
                ("href", &format!("{base_url}/opds")),
                ("type", ATOM_PROFILE),
            ],
        );
        w.self_close(
            "link",
            &[
                ("rel", "http://opds-spec.org/image"),
                ("href", &format!("{base_url}/rest/manga/{manga_id}/cover")),
                ("type", "image/jpeg"),
            ],
        );

        for ch in &chapters {
            let title = chapter_display_title(ch);
            w.open("entry", &[]);
            w.leaf("id", &[], &format!("urn:kani:chapter:{}", ch.id));
            w.leaf("title", &[], &title);
            w.leaf("updated", &[], &updated);
            w.self_close(
                "link",
                &[
                    ("rel", "http://opds-spec.org/acquisition"),
                    ("href", &format!("{base_url}/opds/chapters/{}/file", ch.id)),
                    ("type", "application/vnd.comicbook+zip"),
                    ("title", &title),
                ],
            );
            w.self_close(
                "link",
                &[
                    ("rel", "subsection"),
                    ("href", &format!("{base_url}/opds/chapters/{}", ch.id)),
                    ("type", ATOM_PROFILE),
                ],
            );
            if let Some(count) = ch.page_count {
                let count_str = count.to_string();
                let stream_href = format!(
                    "{base_url}/opds/chapters/{}/page?page={{pageNumber}}",
                    ch.id
                );
                w.self_close(
                    "link",
                    &[
                        ("rel", "http://vaemendis.net/opds-pse/stream"),
                        ("href", &stream_href),
                        ("type", "image/jpeg"),
                        ("pse:count", &count_str),
                    ],
                );
            }
            w.close("entry");
        }

        w.close("feed");
        Ok(w.finish())
    }

    /// Search results feed.
    pub async fn opds_search_feed(
        &self,
        query: &str,
        page: i32,
        user_id: UserId,
        base_url: &str,
    ) -> Result<String> {
        self.opds_catalogue_feed(page, 20, Some(query.to_owned()), user_id, base_url)
            .await
    }

    /// OpenSearch description document.
    pub fn opds_opensearch_description(&self, base_url: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <ShortName>Kani</ShortName>
  <Description>Search the Kani manga library</Description>
  <Url type="application/atom+xml;profile=opds-catalog"
       template="{base_url}/opds/search?q={{searchTerms}}&amp;page={{startPage}}"/>
</OpenSearchDescription>"#
        )
    }

    /// Returns the cached (or freshly scanned) sorted CBZ page-name list for a chapter.
    /// Keyed on the file's mtime so a rewritten archive invalidates the entry naturally.
    pub async fn cbz_page_index(
        &self,
        chapter_id: ChapterId,
        path: &std::path::Path,
    ) -> Result<std::sync::Arc<Vec<String>>> {
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let key = (chapter_id.0, mtime);

        if let Some(cached) = self.cache.cbz_pages_get(key).await {
            return Ok(cached);
        }

        let path_buf = path.to_path_buf();
        let pages = tokio::task::spawn_blocking(move || kani_core::cbz::list_cbz_pages(&path_buf))
            .await
            .map_err(|e| ServiceError::Internal(format!("cbz index join error: {e}")))??;
        let arc = std::sync::Arc::new(pages);
        self.cache.cbz_pages_put(key, arc.clone()).await;
        Ok(arc)
    }

    /// Single-entry OPDS-PSE acquisition feed for one downloaded chapter.
    pub async fn opds_chapter_feed(
        &self,
        chapter_id: ChapterId,
        user_id: UserId,
        base_url: &str,
    ) -> Result<String> {
        let info = self.chapter_cbz_path(chapter_id).await?;

        let existing_count =
            sqlx::query_scalar!("SELECT page_count FROM chapters WHERE id = ?", chapter_id)
                .fetch_one(&self.db_read)
                .await?;

        let count: i64 = match existing_count {
            Some(pc) => pc,
            None => {
                let pages = self.cbz_page_index(chapter_id, &info.path).await?;
                let n = pages.len() as i64;
                sqlx::query!(
                    "UPDATE chapters SET page_count = ? WHERE id = ? AND page_count IS NULL",
                    n,
                    chapter_id
                )
                .execute(&self.db)
                .await?;
                n
            }
        };

        let progress = self.get_chapter_progress_full(user_id, chapter_id).await?;
        // Progress is stored 0-based (it is an index into the page list), so it
        // must be reported in whichever base the reader is being served.
        let zero_based = self.settings.read().await.opds_page_index_zero_based;
        let last_read_str = progress
            .as_ref()
            .map(|(lp, _, _)| if zero_based { *lp } else { lp + 1 }.to_string());
        let last_read_date = progress.as_ref().and_then(|(_, _, d)| d.clone());

        let updated = now_rfc3339();
        let mut w = Utf8Writer::new();
        w.decl();
        w.open(
            "feed",
            &[
                ("xmlns", "http://www.w3.org/2005/Atom"),
                ("xmlns:opds", "http://opds-spec.org/2010/catalog"),
                ("xmlns:pse", "http://vaemendis.net/opds-pse/2017"),
            ],
        );
        w.leaf("id", &[], &format!("urn:kani:chapter:{chapter_id}"));
        w.leaf("title", &[], &info.chapter_title);
        w.leaf("updated", &[], &updated);
        w.self_close(
            "link",
            &[
                ("rel", "self"),
                ("href", &format!("{base_url}/opds/chapters/{chapter_id}")),
                ("type", ATOM_PROFILE),
            ],
        );
        w.self_close(
            "link",
            &[
                ("rel", "start"),
                ("href", &format!("{base_url}/opds")),
                ("type", ATOM_PROFILE),
            ],
        );

        w.open("entry", &[]);
        w.leaf("id", &[], &format!("urn:kani:chapter:{chapter_id}"));
        w.leaf("title", &[], &info.chapter_title);
        w.leaf("updated", &[], &updated);
        w.self_close(
            "link",
            &[
                ("rel", "http://opds-spec.org/acquisition"),
                (
                    "href",
                    &format!("{base_url}/opds/chapters/{chapter_id}/file"),
                ),
                ("type", "application/vnd.comicbook+zip"),
                ("title", &info.chapter_title),
            ],
        );

        let count_str = count.to_string();
        let stream_href = format!("{base_url}/opds/chapters/{chapter_id}/page?page={{pageNumber}}");
        let mut stream_attrs: Vec<(&str, &str)> = vec![
            ("rel", "http://vaemendis.net/opds-pse/stream"),
            ("href", &stream_href),
            ("type", "image/jpeg"),
            ("pse:count", &count_str),
        ];
        if let Some(lr) = &last_read_str {
            stream_attrs.push(("pse:lastRead", lr));
        }
        if let Some(d) = &last_read_date {
            stream_attrs.push(("pse:lastReadDate", d));
        }
        w.self_close("link", &stream_attrs);

        w.close("entry");
        w.close("feed");
        Ok(w.finish())
    }

    /// Translate the `page` an OPDS reader sent into a 0-based index.
    ///
    /// Readers substitute the PSE `{pageNumber}` template, and the prevailing
    /// reading — the one Komga follows — is that the first page is 1. Kani used
    /// to treat it as a raw 0-based index, so every page was off by one and the
    /// last page 404'd. Operators whose reader really does send 0 first can set
    /// `opds_page_index_zero_based`.
    pub async fn opds_page_to_index(&self, page: usize) -> Result<usize> {
        if self.settings.read().await.opds_page_index_zero_based {
            return Ok(page);
        }
        page.checked_sub(1).ok_or_else(|| {
            ServiceError::Validation("OPDS page numbers are 1-based; page 0 is not valid".into())
        })
    }

    /// Resolves and (optionally) transcodes a single chapter page for OPDS-PSE.
    ///
    /// `page` is as the reader sent it; see [`Self::opds_page_to_index`].
    pub async fn opds_chapter_page(
        &self,
        chapter_id: ChapterId,
        page: usize,
        max_width: u32,
        format: Option<image::ImageFormat>,
    ) -> Result<(Vec<u8>, &'static str)> {
        let page_index = self.opds_page_to_index(page).await?;
        let info = self.chapter_cbz_path(chapter_id).await?;
        let pages = self.cbz_page_index(chapter_id, &info.path).await?;
        if page_index >= pages.len() {
            return Err(ServiceError::NotFound(format!(
                "Page {page} out of range ({} pages)",
                pages.len()
            )));
        }

        let width = max_width.min(crate::tuning::OPDS_MAX_TRANSCODE_WIDTH);
        let path = info.path.clone();
        let result = tokio::task::spawn_blocking(move || {
            kani_core::cbz::read_cbz_page_transcoded(&path, page_index, width, format)
        })
        .await
        .map_err(|e| ServiceError::Internal(format!("page transcode join error: {e}")))??;
        Ok(result)
    }
}

const ATOM_PROFILE: &str = "application/atom+xml;profile=opds-catalog";

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn catalogue_href(base_url: &str, page: i32, page_size: i32, search: Option<&str>) -> String {
    let mut href = format!("{base_url}/opds/catalogue?page={page}&page_size={page_size}");
    if let Some(q) = search {
        href.push_str(&format!("&q={}", urlencoding::encode(q)));
    }
    href
}

fn chapter_display_title(ch: &kani_shared::types::Chapter) -> String {
    let mut s = String::new();
    if let Some(vol) = ch.volume {
        s.push_str(&format!("Vol. {vol} "));
    }
    if ch.number.fract().abs() < f64::EPSILON {
        s.push_str(&format!("Ch. {}", ch.number as i64));
    } else {
        s.push_str(&format!("Ch. {:.1}", ch.number));
    }
    if let Some(ref title) = ch.title
        && !title.is_empty()
    {
        s.push_str(&format!(" - {title}"));
    }
    s
}

fn nav_entry(w: &mut Utf8Writer, id: &str, title: &str, content: &str, updated: &str, href: &str) {
    w.open("entry", &[]);
    w.leaf("id", &[], id);
    w.leaf("title", &[], title);
    w.leaf("updated", &[], updated);
    w.leaf("content", &[("type", "text")], content);
    w.self_close(
        "link",
        &[
            ("rel", "subsection"),
            ("href", href),
            ("type", ATOM_PROFILE),
        ],
    );
    w.close("entry");
}

struct Utf8Writer {
    writer: Writer<Cursor<Vec<u8>>>,
}

impl Utf8Writer {
    fn new() -> Self {
        Self {
            writer: Writer::new(Cursor::new(Vec::new())),
        }
    }

    fn finish(self) -> String {
        String::from_utf8(self.writer.into_inner().into_inner()).unwrap_or_default()
    }

    fn decl(&mut self) {
        let decl = BytesDecl::new("1.0", Some("UTF-8"), None);
        self.writer.write_event(Event::Decl(decl)).ok();
    }

    fn open(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        let mut start = BytesStart::new(tag);
        for (k, v) in attrs {
            start.push_attribute((*k, *v));
        }
        self.writer.write_event(Event::Start(start)).ok();
    }

    fn close(&mut self, tag: &str) {
        self.writer.write_event(Event::End(BytesEnd::new(tag))).ok();
    }

    fn self_close(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        let mut start = BytesStart::new(tag);
        for (k, v) in attrs {
            start.push_attribute((*k, *v));
        }
        self.writer.write_event(Event::Empty(start)).ok();
    }

    fn leaf(&mut self, tag: &str, attrs: &[(&str, &str)], text: &str) {
        self.open(tag, attrs);
        self.writer
            .write_event(Event::Text(BytesText::new(text)))
            .ok();
        self.close(tag);
    }
}
