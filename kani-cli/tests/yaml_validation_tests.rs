#![allow(clippy::unwrap_used)]

use kani_cli::yaml::{schema::YamlExtension, validate};
use std::path::Path;

fn validate_str(
    yaml: &str,
) -> Result<kani_cli::yaml::model::ValidatedExtension, Vec<kani_yaml::YamlError>> {
    let path = Path::new("test.yaml");
    let ext: YamlExtension = serde_yaml::from_str(yaml).expect("fixture must parse as YAML");
    validate::validate(&ext, yaml, path)
}

fn assert_valid(yaml: &str) {
    assert!(validate_str(yaml).is_ok(), "expected valid, got errors");
}

fn assert_invalid_containing(yaml: &str, needle: &str) {
    match validate_str(yaml) {
        Ok(_) => panic!(
            "expected validation error containing '{}' but validation passed",
            needle
        ),
        Err(errs) => {
            let messages: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
            assert!(
                messages.iter().any(|m| m.contains(needle)),
                "expected an error containing '{}', got: {:?}",
                needle,
                messages
            );
        }
    }
}

#[test]
fn valid_minimal_extension() {
    assert_valid(
        r#"
id: minimal
name: Minimal
version: "0.1.0"
base_url: "https://example.com"
"#,
    );
}

#[test]
fn valid_popular_endpoint() {
    assert_valid(
        r#"
id: pop-test
name: PopTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  popular:
    route: "/manga"
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
    );
}

#[test]
fn valid_delegated_popular() {
    assert_valid(
        r#"
id: delegate-test
name: DelegateTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  popular:
    delegate_to: search
    empty_without_filters: true
  search:
    route: "/search"
    queries:
      q: $query$
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
    );
}

#[test]
fn valid_search_endpoint() {
    assert_valid(
        r#"
id: search-test
name: SearchTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  search:
    route: "/search"
    queries:
      q: $query$
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
    );
}

#[test]
fn valid_manga_details_all_required_fields() {
    assert_valid(
        r#"
id: details-test
name: DetailsTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  manga_details:
    route: "/manga/$manga_id$"
    container: ":root"
    fields:
      id: '"$manga_id$"'
      title: 'dom("h1").text()'
      status: 'dom(".status").text()'
"#,
    );
}

#[test]
fn valid_chapter_list() {
    assert_valid(
        r#"
id: chapters-test
name: ChaptersTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  chapter_list:
    route: "/manga/$manga_id$/chapters"
    container: ".chapter"
    fields:
      id: 'self.attr("data-id")'
"#,
    );
}

#[test]
fn valid_pages() {
    assert_valid(
        r#"
id: pages-test
name: PagesTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  pages:
    route: "/chapter/$chapter_id$"
    container: "img"
    fields:
      index: "index()"
      url: 'self.attr("src")'
"#,
    );
}

#[test]
fn valid_full_example_fixture() {
    let path = Path::new("tests/fixtures/manga_details.yaml");
    let src = std::fs::read_to_string(path).unwrap();
    let ext: YamlExtension = serde_yaml::from_str(&src).unwrap();
    assert!(validate::validate(&ext, &src, path).is_ok());
}

#[test]
fn invalid_popular_missing_title() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  popular:
    route: "/manga"
    container: ".item"
    fields:
      id: 'self.attr("href")'
"#,
        "title",
    );
}

#[test]
fn invalid_popular_missing_id() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  popular:
    route: "/manga"
    container: ".item"
    fields:
      title: 'self.text()'
"#,
        "id",
    );
}

#[test]
fn invalid_manga_details_missing_status() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  manga_details:
    route: "/manga/$manga_id$"
    container: ":root"
    fields:
      id: '"$manga_id$"'
      title: 'dom("h1").text()'
"#,
        "status",
    );
}

#[test]
fn invalid_pages_missing_url() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  pages:
    route: "/chapter/$chapter_id$"
    container: "img"
    fields:
      index: "index()"
"#,
        "url",
    );
}

#[test]
fn invalid_pages_missing_index() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  pages:
    route: "/chapter/$chapter_id$"
    container: "img"
    fields:
      url: 'self.attr("src")'
"#,
        "index",
    );
}

#[test]
fn invalid_endpoint_missing_route() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  popular:
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
        "route",
    );
}

#[test]
fn invalid_route_unknown_variable() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  popular:
    route: "/manga/$unknown_var$"
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
        "unknown_var",
    );
}

#[test]
fn invalid_dsl_parse_error() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  popular:
    route: "/manga"
    container: ".item"
    fields:
      id: '!!invalid dsl !!'
      title: 'self.text()'
"#,
        "DSL parse failed",
    );
}

#[test]
fn invalid_delegated_popular_bad_target() {
    assert_invalid_containing(
        r#"
id: err-test
name: ErrTest
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  popular:
    delegate_to: popular
"#,
        "delegate_to",
    );
}

const FILTER_BASE: &str = r#"
id: filter-test
name: FilterTest
version: "0.1.0"
base_url: "https://example.com"
"#;

#[test]
fn valid_filter_id_flat() {
    assert_valid(&format!(
        r#"{}
filters:
  - id: sort
    name: Sort
    type: select
    options:
      - name: Newest
        value: new
"#,
        FILTER_BASE
    ));
}

#[test]
fn valid_filter_id_with_colon() {
    assert_valid(&format!(
        r#"{}
filters:
  - id: "sort:by"
    name: Sort
    type: select
    options:
      - name: Newest
        value: new
"#,
        FILTER_BASE
    ));
}

#[test]
fn valid_mihon_source_id() {
    assert_valid(&format!(
        r#"{}
mihon_source_id: 2499283573021220255
"#,
        FILTER_BASE
    ));
}

#[test]
fn invalid_filter_id_empty() {
    assert_invalid_containing(
        &format!(
            r#"{}
filters:
  - id: ""
    name: Sort
    type: select
"#,
            FILTER_BASE
        ),
        "must not be empty",
    );
}

#[test]
fn invalid_filter_id_whitespace() {
    assert_invalid_containing(
        &format!(
            r#"{}
filters:
  - id: "sort by"
    name: Sort
    type: select
"#,
            FILTER_BASE
        ),
        "whitespace",
    );
}

#[test]
fn invalid_filter_id_leading_colon() {
    assert_invalid_containing(
        &format!(
            r#"{}
filters:
  - id: ":sort"
    name: Sort
    type: select
"#,
            FILTER_BASE
        ),
        "must not start or end",
    );
}

#[test]
fn invalid_filter_id_trailing_colon() {
    assert_invalid_containing(
        &format!(
            r#"{}
filters:
  - id: "sort:"
    name: Sort
    type: select
"#,
            FILTER_BASE
        ),
        "must not start or end",
    );
}

#[test]
fn invalid_filter_id_multiple_colons() {
    assert_invalid_containing(
        &format!(
            r#"{}
filters:
  - id: "sort:by:date"
    name: Sort
    type: select
"#,
            FILTER_BASE
        ),
        "at most one",
    );
}

#[test]
fn valid_filter_format_full() {
    assert_valid(&format!(
        r#"{}
filters:
  - id: genres
    name: Genres
    type: multiselect
    options:
      - name: Action
        value: action
endpoints:
  search:
    route: "/search"
    queries:
      q: $query$
    filter_mapping:
      genres: genre
    filter_format:
      multiselect: bracket
      omit_empty: false
      bool_format: one_zero
      array_separator: "|"
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
        FILTER_BASE
    ));
}

#[test]
fn invalid_filter_format_empty_array_separator() {
    assert_invalid_containing(
        &format!(
            r#"{}
endpoints:
  search:
    route: "/search"
    queries:
      q: $query$
    filter_format:
      array_separator: ""
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
            FILTER_BASE
        ),
        "array_separator",
    );
}

#[test]
fn valid_tuple_split_filter_mapping() {
    assert_valid(&format!(
        r#"{}
filters:
  - id: year_range
    name: Year range
    type: date_range
    min: 1900
    max: 2100
endpoints:
  search:
    route: "/search"
    queries:
      q: $query$
    filter_mapping:
      year_range:
        kind: tuple_split
        from_param: year_from
        to_param: year_to
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
        FILTER_BASE
    ));
}

#[test]
fn valid_static_option_set_ref() {
    assert_valid(&format!(
        r#"{}
option_sets:
  genres:
    - name: Action
      value: action
    - name: Romance
      value: romance
filters:
  - id: genre
    name: Genre
    type: select
    options_ref: genres
"#,
        FILTER_BASE
    ));
}

#[test]
fn valid_fetched_option_set_ref() {
    assert_valid(&format!(
        r#"{}
option_sets:
  tags:
    options_fetched_by:
      route: "/api/tags"
      type: json
      container: "$.tags"
      fields:
        name: "self.field(\"name\").text()"
        value: "self.field(\"id\").text()"
      cache:
        ttl: 600
        key: tags-v1
filters:
  - id: tag
    name: Tag
    type: multiselect
    options_ref: tags
"#,
        FILTER_BASE
    ));
}

#[test]
fn invalid_options_ref_unresolved() {
    assert_invalid_containing(
        &format!(
            r#"{}
filters:
  - id: genre
    name: Genre
    type: select
    options_ref: does_not_exist
"#,
            FILTER_BASE
        ),
        "does_not_exist",
    );
}

#[test]
fn invalid_preference_options_ref_unresolved() {
    assert_invalid_containing(
        &format!(
            r#"{}
preferences:
  - key: lang
    label: Language
    kind: select
    options_ref: does_not_exist
"#,
            FILTER_BASE
        ),
        "does_not_exist",
    );
}

#[test]
fn invalid_fetched_option_set_empty_route() {
    assert_invalid_containing(
        &format!(
            r#"{}
option_sets:
  tags:
    options_fetched_by:
      route: ""
filters:
  - id: tag
    name: Tag
    type: multiselect
    options_ref: tags
"#,
            FILTER_BASE
        ),
        "route",
    );
}

#[test]
fn invalid_cache_ttl_too_long() {
    assert_invalid_containing(
        &format!(
            r#"{}
option_sets:
  tags:
    options_fetched_by:
      route: "/api/tags"
      cache:
        ttl: 9999999
        key: tags-v1
filters:
  - id: tag
    name: Tag
    type: multiselect
    options_ref: tags
"#,
            FILTER_BASE
        ),
        "30 days",
    );
}

#[test]
fn valid_int_range_filter() {
    assert_valid(&format!(
        r#"{}
filters:
  - id: chapters
    name: Chapter count
    type: int_range
    min: 0
    max: 9999
"#,
        FILTER_BASE
    ));
}

#[test]
fn invalid_int_range_missing_max() {
    assert_invalid_containing(
        &format!(
            r#"{}
filters:
  - id: chapters
    name: Chapter count
    type: int_range
    min: 0
"#,
            FILTER_BASE
        ),
        "min",
    );
}

#[test]
fn valid_id_encoding_full_fixture() {
    let path = Path::new("tests/fixtures/id_encoding.yaml");
    let src = std::fs::read_to_string(path).unwrap();
    let ext: YamlExtension = serde_yaml::from_str(&src).unwrap();
    assert!(validate::validate(&ext, &src, path).is_ok());
}

#[test]
fn valid_id_encoding_single_field_no_delimiter_required() {
    assert_valid(&format!(
        r#"{}
id_encoding:
  chapter:
    fields: [hid]
    encoding: passthrough
endpoints:
  pages:
    route: "/chapter/$chapter.hid$"
    container: "img"
    fields:
      index: "index()"
      url: 'self.attr("src")'
"#,
        FILTER_BASE
    ));
}

#[test]
fn invalid_id_encoding_empty_fields() {
    assert_invalid_containing(
        &format!(
            r#"{}
id_encoding:
  manga:
    fields: []
"#,
            FILTER_BASE
        ),
        "must not be empty",
    );
}

#[test]
fn invalid_id_encoding_multi_field_missing_delimiter() {
    assert_invalid_containing(
        &format!(
            r#"{}
id_encoding:
  manga:
    fields: [hid, slug]
    delimiter: ""
"#,
            FILTER_BASE
        ),
        "delimiter",
    );
}

#[test]
fn invalid_id_encoding_duplicate_field() {
    assert_invalid_containing(
        &format!(
            r#"{}
id_encoding:
  manga:
    fields: [hid, hid]
"#,
            FILTER_BASE
        ),
        "duplicate field",
    );
}

#[test]
fn invalid_route_dotted_var_unknown_role() {
    assert_invalid_containing(
        &format!(
            r#"{}
id_encoding:
  manga:
    fields: [hid]
endpoints:
  manga_details:
    route: "/manga/$bogus.hid$"
    container: ":root"
    fields:
      id: '"$manga_id$"'
      title: 'dom("h1").text()'
      status: 'dom(".status").text()'
"#,
            FILTER_BASE
        ),
        "unknown id_encoding role",
    );
}

#[test]
fn invalid_route_dotted_var_undeclared_field() {
    assert_invalid_containing(
        &format!(
            r#"{}
id_encoding:
  manga:
    fields: [hid]
endpoints:
  manga_details:
    route: "/manga/$manga.slug$"
    container: ":root"
    fields:
      id: '"$manga_id$"'
      title: 'dom("h1").text()'
      status: 'dom(".status").text()'
"#,
            FILTER_BASE
        ),
        "not declared in id_encoding",
    );
}

#[test]
fn invalid_route_dotted_var_no_id_encoding_block() {
    assert_invalid_containing(
        &format!(
            r#"{}
endpoints:
  manga_details:
    route: "/manga/$manga.hid$"
    container: ":root"
    fields:
      id: '"$manga_id$"'
      title: 'dom("h1").text()'
      status: 'dom(".status").text()'
"#,
            FILTER_BASE
        ),
        "no such block is declared",
    );
}

#[test]
fn invalid_route_dotted_var_role_not_available_on_endpoint() {
    assert_invalid_containing(
        &format!(
            r#"{}
id_encoding:
  chapter:
    fields: [hid]
endpoints:
  search:
    route: "/search"
    queries:
      q: $query$
      ch: $chapter.hid$
    container: ".item"
    fields:
      id: 'self.attr("href")'
      title: 'self.text()'
"#,
            FILTER_BASE
        ),
        "is not available",
    );
}

#[test]
fn valid_composite_id_field_in_manga_details() {
    assert_valid(&format!(
        r#"{}
id_encoding:
  manga:
    fields: [hid, slug]
endpoints:
  manga_details:
    route: "/manga/$manga_id$"
    container: ":root"
    fields:
      id:
        hid: 'dom("[data-hid]").attr("data-hid")'
        slug: 'dom("[data-slug]").attr("data-slug")'
      title: 'dom("h1").text()'
      status: 'dom(".status").text()'
"#,
        FILTER_BASE
    ));
}

#[test]
fn invalid_composite_id_field_key_mismatch() {
    assert_invalid_containing(
        &format!(
            r#"{}
id_encoding:
  manga:
    fields: [hid, slug]
endpoints:
  manga_details:
    route: "/manga/$manga_id$"
    container: ":root"
    fields:
      id:
        hid: 'dom("[data-hid]").attr("data-hid")'
      title: 'dom("h1").text()'
      status: 'dom(".status").text()'
"#,
            FILTER_BASE
        ),
        "do not match",
    );
}

#[test]
fn invalid_composite_id_field_no_id_encoding_block() {
    assert_invalid_containing(
        &format!(
            r#"{}
endpoints:
  manga_details:
    route: "/manga/$manga_id$"
    container: ":root"
    fields:
      id:
        hid: 'dom("[data-hid]").attr("data-hid")'
      title: 'dom("h1").text()'
      status: 'dom(".status").text()'
"#,
            FILTER_BASE
        ),
        "is not configured",
    );
}

#[test]
fn invalid_composite_id_field_unsupported_endpoint() {
    assert_invalid_containing(
        &format!(
            r#"{}
id_encoding:
  manga:
    fields: [hid]
endpoints:
  pages:
    route: "/chapter/$chapter_id$"
    container: "img"
    fields:
      index: "index()"
      url: 'self.attr("src")'
      id:
        hid: 'self.attr("data-hid")'
"#,
            FILTER_BASE
        ),
        "not supported",
    );
}

const CACHE_BASE: &str = r#"
id: cache-test
name: CacheTest
version: "0.1.0"
base_url: "https://example.com"
"#;

#[test]
fn valid_cache_minimal_entry_defaults() {
    assert_valid(&format!(
        r#"{}
cache:
  search_results: {{}}
"#,
        CACHE_BASE
    ));
}

#[test]
fn valid_cache_namespaces_with_limits() {
    let validated = validate_str(
        r#"
id: cache-ok
name: CacheOk
version: "0.1.0"
base_url: "https://example.com"
cache:
  search_results:
    ttl: 1800
    max_entries: 200
  auth: {}
"#,
    )
    .unwrap_or_else(|e| panic!("expected valid, got {e:?}"));
    let limits = validated.cache_limits();
    assert_eq!(limits["search_results"].ttl_seconds, 1800);
    assert_eq!(limits["search_results"].max_entries, Some(200));
    assert_eq!(limits["auth"].ttl_seconds, 3600, "ttl defaults to an hour");
}

#[test]
fn invalid_cache_empty_name() {
    assert_invalid_containing(
        &format!(
            r#"{}
cache:
  "": {{}}
"#,
            CACHE_BASE
        ),
        "must not be empty",
    );
}

#[test]
fn invalid_cache_name_with_colon() {
    assert_invalid_containing(
        &format!(
            r#"{}
cache:
  "bad:name": {{}}
"#,
            CACHE_BASE
        ),
        "must not contain ':' or '/'",
    );
}

#[test]
fn invalid_cache_name_with_slash() {
    assert_invalid_containing(
        &format!(
            r#"{}
cache:
  "bad/name": {{}}
"#,
            CACHE_BASE
        ),
        "must not contain ':' or '/'",
    );
}

#[test]
fn invalid_cache_ttl_exceeds_max() {
    assert_invalid_containing(
        &format!(
            r#"{}
cache:
  too_long:
    ttl: 9999999
"#,
            CACHE_BASE
        ),
        "ttl must not exceed 30 days",
    );
}

#[test]
fn removed_cache_fields_are_rejected() {
    for field in ["scope: extension", "key_template: \"k\""] {
        let yaml = format!(
            "id: cache-old\nname: X\nversion: \"0.1.0\"\nbase_url: \"https://example.com\"\ncache:\n  ns:\n    {field}\n"
        );
        let ext: Result<YamlExtension, _> = serde_yaml::from_str(&yaml);
        let err = ext.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            err.contains("unknown field"),
            "{field} must be refused, got: {err:?}"
        );
    }
}

#[test]
fn too_many_cache_namespaces_are_rejected() {
    let namespaces: String = (0..=kani_shared::MAX_CACHE_NAMESPACES)
        .map(|i| format!("  ns{i}: {{}}\n"))
        .collect();
    assert_invalid_containing(
        &format!(
            "id: cache-many\nname: X\nversion: \"0.1.0\"\nbase_url: \"https://example.com\"\ncache:\n{namespaces}"
        ),
        "at most 16 namespaces",
    );
}

#[test]
fn cache_max_entries_must_be_within_the_host_limit() {
    for max in ["0", "4097"] {
        assert_invalid_containing(
            &format!(
                "id: cache-max\nname: X\nversion: \"0.1.0\"\nbase_url: \"https://example.com\"\ncache:\n  ns:\n    max_entries: {max}\n"
            ),
            "max_entries must be between 1 and 4096",
        );
    }
}

const METADATA_BASE: &str = r#"
id: metadata-test
name: MetadataTest
version: "0.1.0"
base_url: "https://example.com"
"#;

const TINY_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

#[test]
fn valid_metadata_full_block() {
    assert_valid(&format!(
        r#"{base}
schema_version: 1
min_kani_version: "0.5.0"
requires_capabilities:
  - "unrestricted_http"
metadata:
  icon: "{icon}"
  rate_limit:
    rps: 2.0
    burst: 8
    max_concurrent: 4
  languages:
    - "en"
    - "ja"
  description: "A test extension"
  sections:
    - id: "latest"
      name: "Latest"
      nsfw: false
    - id: "ecchi"
      name: "Ecchi"
      nsfw: true
"#,
        base = METADATA_BASE,
        icon = TINY_PNG_BASE64,
    ));
}

#[test]
fn valid_metadata_block_absent_defaults() {
    assert_valid(METADATA_BASE);
}

#[test]
fn invalid_metadata_icon_bad_base64() {
    assert_invalid_containing(
        &format!(
            r#"{}
metadata:
  icon: "not-valid-base64!!"
"#,
            METADATA_BASE
        ),
        "not valid base64",
    );
}

#[test]
fn invalid_metadata_icon_unrecognized_format() {
    assert_invalid_containing(
        &format!(
            r#"{}
metadata:
  icon: "{}"
"#,
            METADATA_BASE,
            base64_text("just some bytes that aren't an image"),
        ),
        "must be a PNG or WebP image",
    );
}

#[test]
fn invalid_metadata_icon_too_large() {
    let huge = base64_text(&"x".repeat(70 * 1024));
    assert_invalid_containing(
        &format!(
            r#"{}
metadata:
  icon: "{}"
"#,
            METADATA_BASE, huge
        ),
        "over the 64 KiB limit",
    );
}

#[test]
fn invalid_metadata_rate_limit_zero_rps() {
    assert_invalid_containing(
        &format!(
            r#"{}
metadata:
  rate_limit:
    rps: 0
"#,
            METADATA_BASE
        ),
        "must be greater than 0",
    );
}

#[test]
fn invalid_metadata_duplicate_section_id() {
    assert_invalid_containing(
        &format!(
            r#"{}
metadata:
  sections:
    - id: "latest"
      name: "Latest"
    - id: "latest"
      name: "Also Latest"
"#,
            METADATA_BASE
        ),
        "duplicate section id",
    );
}

#[test]
fn invalid_metadata_empty_section_id() {
    assert_invalid_containing(
        &format!(
            r#"{}
metadata:
  sections:
    - id: ""
      name: "Latest"
"#,
            METADATA_BASE
        ),
        "section 'id' must not be empty",
    );
}

#[test]
fn invalid_schema_version_too_new() {
    assert_invalid_containing(
        &format!(
            r#"{}
schema_version: 999
"#,
            METADATA_BASE
        ),
        "is newer than the schema version",
    );
}

#[test]
fn invalid_min_kani_version_bad_semver() {
    assert_invalid_containing(
        &format!(
            r#"{}
min_kani_version: "not-a-version"
"#,
            METADATA_BASE
        ),
        "is not a valid semver version",
    );
}

fn base64_text(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

const CHAPTER_SORT_BASE: &str = r#"
id: sort-test
name: SortTest
version: "0.1.0"
base_url: "https://example.com"
"#;

#[test]
fn valid_chapter_sort_with_default() {
    assert_valid(&format!(
        r#"{base}
chapter_sort:
  default: "number_desc"
  options:
    - id: "number_desc"
      label: "Chapter (descending)"
    - id: "number_asc"
      label: "Chapter (ascending)"
"#,
        base = CHAPTER_SORT_BASE
    ));
}

#[test]
fn valid_chapter_sort_without_default() {
    assert_valid(&format!(
        r#"{base}
chapter_sort:
  options:
    - id: "date"
      label: "Date added"
"#,
        base = CHAPTER_SORT_BASE
    ));
}

#[test]
fn invalid_chapter_sort_empty_options() {
    assert_invalid_containing(
        &format!(
            r#"{base}
chapter_sort:
  options: []
"#,
            base = CHAPTER_SORT_BASE
        ),
        "'options' must not be empty",
    );
}

#[test]
fn invalid_chapter_sort_empty_option_id() {
    assert_invalid_containing(
        &format!(
            r#"{base}
chapter_sort:
  options:
    - id: ""
      label: "Chapter"
"#,
            base = CHAPTER_SORT_BASE
        ),
        "option 'id' must not be empty",
    );
}

#[test]
fn invalid_chapter_sort_default_not_in_options() {
    assert_invalid_containing(
        &format!(
            r#"{base}
chapter_sort:
  default: "missing"
  options:
    - id: "number_desc"
      label: "Chapter (descending)"
"#,
            base = CHAPTER_SORT_BASE
        ),
        "default 'missing' is not listed in options",
    );
}

#[test]
fn valid_for_each_step() {
    assert_valid(&format!(
        r#"{base}
endpoints:
  search:
    route: "https://example.com/search"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
    for_each:
      - endpoint: manga_details
        url_expr: "dom(\".link\").attr(\"href\")"
        merge_as: details
  manga_details:
    route: "https://example.com/manga/$manga_id$"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
      status:
        expr: "dom(\".status\").text()"
"#,
        base = r#"
id: chain-test
name: ChainTest
version: "0.1.0"
base_url: "https://example.com""#
    ));
}

#[test]
fn valid_then_step() {
    assert_valid(&format!(
        r#"{base}
endpoints:
  search:
    route: "https://example.com/search"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
    then:
      - endpoint: manga_details
        url_expr: "dom(\".link\").attr(\"href\")"
        merge_as: meta
  manga_details:
    route: "https://example.com/manga/$manga_id$"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
      status:
        expr: "dom(\".status\").text()"
"#,
        base = r#"
id: chain-test
name: ChainTest
version: "0.1.0"
base_url: "https://example.com""#
    ));
}

#[test]
fn invalid_for_each_undefined_endpoint() {
    assert_invalid_containing(
        &format!(
            r#"{base}
endpoints:
  search:
    route: "https://example.com/search"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
    for_each:
      - endpoint: nonexistent_endpoint
        url_expr: "dom(\".link\").attr(\"href\")"
        merge_as: details
"#,
            base = r#"
id: chain-test
name: ChainTest
version: "0.1.0"
base_url: "https://example.com""#
        ),
        "nonexistent_endpoint",
    );
}

#[test]
fn for_each_legacy_concurrency_key_is_accepted_and_ignored() {
    assert_valid(&format!(
        r#"{base}
endpoints:
  search:
    route: "https://example.com/search"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
    for_each:
      - endpoint: manga_details
        url_expr: "dom(\".link\").attr(\"href\")"
        merge_as: details
        concurrency: 10
  manga_details:
    route: "https://example.com/manga/$manga_id$"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
      status:
        expr: "dom(\".status\").text()"
"#,
        base = r#"
id: chain-test
name: ChainTest
version: "0.1.0"
base_url: "https://example.com""#
    ));
}

#[test]
fn invalid_for_each_empty_merge_as() {
    assert_invalid_containing(
        &format!(
            r#"{base}
endpoints:
  search:
    route: "https://example.com/search"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
    for_each:
      - endpoint: manga_details
        url_expr: "dom(\".link\").attr(\"href\")"
        merge_as: ""
  manga_details:
    route: "https://example.com/manga/$manga_id$"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
      status:
        expr: "dom(\".status\").text()"
"#,
            base = r#"
id: chain-test
name: ChainTest
version: "0.1.0"
base_url: "https://example.com""#
        ),
        "merge_as",
    );
}

#[test]
fn valid_fetched_option_set() {
    assert_valid(
        r#"
id: fetched-opts-test
name: FetchedOptsTest
version: "0.1.0"
base_url: "https://example.com"

option_sets:
  genres:
    options_fetched_by:
      route: "https://example.com/genres"
      type: json
      container: /genres
      fields:
        name: /name
        value: /id
      cache:
        ttl: 600
        key: genres-v1

filters:
  - id: genre
    name: Genre
    type: multiselect
    options_ref: genres

endpoints:
  search:
    route: "https://example.com/search"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
"#,
    );
}

#[test]
fn fetched_option_set_missing_route_fails() {
    assert_invalid_containing(
        r#"
id: fetched-opts-test
name: FetchedOptsTest
version: "0.1.0"
base_url: "https://example.com"

option_sets:
  genres:
    options_fetched_by:
      route: ""
      type: json

endpoints:
  search:
    route: "https://example.com/search"
    fields:
      id:
        expr: "dom(\".id\").text()"
      title:
        expr: "dom(\".title\").text()"
"#,
        "route",
    );
}

#[test]
fn an_id_outside_the_documented_form_is_rejected() {
    for id in ["Fetched_Opts", "a:b", "1abc", "has space", "-lead"] {
        assert_invalid_containing(
            &format!(
                "id: \"{id}\"\nname: X\nversion: \"0.1.0\"\nbase_url: \"https://example.com\"\n"
            ),
            "must match [a-z][a-z0-9-]*",
        );
    }
}

#[test]
fn script_names_that_could_escape_or_collide_are_refused() {
    for bad in ["../x", "a-b", "a\"b", "Upper", "1st", ""] {
        let quoted = serde_json::to_string(bad).unwrap();
        let browser =
            format!("{METADATA_BASE}browser_scripts:\n  {quoted}: \"passPayload('{{}}')\"\n");
        assert_invalid_containing(&browser, "must match [a-z][a-z0-9_]*");
        let pure = format!("{METADATA_BASE}scripts:\n  pure:\n    {quoted}: \"fn f(x) {{ x }}\"\n");
        assert_invalid_containing(&pure, "must match [a-z][a-z0-9_]*");
    }
    assert_valid(&format!(
        "{METADATA_BASE}browser_scripts:\n  harvest_cipher_2: \"passPayload('{{}}')\"\n"
    ));
}

fn fetched_set(name: &str, cache_key: Option<&str>) -> String {
    let cache = cache_key
        .map(|k| format!("      cache:\n        ttl: 600\n        key: {k}\n"))
        .unwrap_or_default();
    format!(
        "  {name}:\n    options_fetched_by:\n      route: \"https://example.com/api/{name}\"\n      \
         type: json\n      container: /items\n      fields:\n        name: /label\n        \
         value: /slug\n{cache}"
    )
}

#[test]
fn fetched_option_sets_cannot_share_a_cache_key() {
    let yaml = |a: String, b: String| format!("{METADATA_BASE}option_sets:\n{a}{b}");

    assert_invalid_containing(
        &yaml(
            fetched_set("genres", Some("v1")),
            fetched_set("tags", Some("v1")),
        ),
        "cache key 'v1' is also used by",
    );
    assert_invalid_containing(
        &yaml(
            fetched_set("genres", Some("tags")),
            fetched_set("tags", None),
        ),
        "cache key 'tags' is also used by",
    );
    assert_valid(&yaml(
        fetched_set("genres", Some("genres-v1")),
        fetched_set("tags", None),
    ));
}

#[test]
fn a_filter_mapping_key_must_name_a_filter_group() {
    let yaml = |mapping_key: &str| {
        format!(
            "{METADATA_BASE}filters:\n  - id: \"genre:Action\"\n    name: Action\n    type: checkbox\n  \
             - id: \"genre:Adventure\"\n    name: Adventure\n    type: checkbox\n\
             endpoints:\n  search:\n    route: \"/s\"\n    filter_mapping:\n      \
             {mapping_key}: genre\n    fields:\n      id: 'dom(\".id\").text()'\n      \
             title: 'dom(\".t\").text()'\n"
        )
    };
    assert_valid(&yaml("genre"));
    assert_invalid_containing(&yaml("genres"), "no filter has id 'genres'");
}

#[test]
fn id_encoding_definitions_that_cannot_round_trip_are_refused() {
    let yaml = |block: &str| format!("{METADATA_BASE}id_encoding:\n  manga:\n{block}");
    assert_invalid_containing(
        &yaml("    fields: []\n    delimiter: \"|\"\n"),
        "'fields' must not be empty",
    );
    assert_invalid_containing(
        &yaml("    fields: [hid, hid]\n    delimiter: \"|\"\n"),
        "duplicate",
    );
    assert_invalid_containing(
        &yaml("    fields: [hid, slug]\n    delimiter: \"\"\n"),
        "'delimiter' must not be empty",
    );
    assert_invalid_containing(
        &yaml("    fields: [hid, \"the-slug\"]\n    delimiter: \"|\"\n"),
        "may use only letters, digits and '_'",
    );
    assert_invalid_containing(
        &yaml("    fields: [hid, \"a.b\"]\n    delimiter: \"|\"\n"),
        "may use only letters, digits and '_'",
    );
    assert_valid(&yaml("    fields: [hid, slug_2]\n    delimiter: \"|\"\n"));
}

#[test]
fn a_required_row_field_cannot_be_optional() {
    let yaml = |endpoint: &str, field: &str| {
        let plain: String = ["id", "title", "status"]
            .iter()
            .filter(|f| **f != field)
            .map(|f| format!("      {f}: 'dom(\".{f}\").text()'\n"))
            .collect();
        format!(
            "{METADATA_BASE}endpoints:\n  {endpoint}:\n    route: \"/x\"\n    fields:\n{plain}      \
             {field}:\n        expr: 'dom(\".{field}\").text()'\n        optional: true\n"
        )
    };
    for (endpoint, field) in [
        ("popular", "id"),
        ("search", "title"),
        ("manga_details", "status"),
        ("chapter_list", "id"),
    ] {
        assert_invalid_containing(
            &yaml(endpoint, field),
            &format!("endpoints.{endpoint}.fields.{field}: a required field cannot be optional"),
        );
    }
    assert_valid(&yaml("search", "cover_url"));
}

#[test]
fn a_file_without_schema_version_is_version_one() {
    let validated = validate_str(METADATA_BASE).unwrap();
    assert_eq!(validated.schema_version, 1);
}

#[test]
fn a_scaffolded_extension_states_its_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_kani-cli"))
        .args(["new", "pinned-source"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(status.success());
    let yaml = std::fs::read_to_string(dir.path().join("pinned-source.yaml")).unwrap();
    assert!(
        yaml.lines().any(|l| l == "schema_version: 1"),
        "scaffold must pin schema_version: {yaml}"
    );
}

#[test]
fn an_svg_or_html_icon_is_refused() {
    use base64::Engine;
    for markup in ["<svg xmlns='http://www.w3.org/2000/svg'/>", "<html></html>"] {
        let icon = base64::engine::general_purpose::STANDARD.encode(markup);
        assert_invalid_containing(
            &format!("{METADATA_BASE}metadata:\n  icon: \"{icon}\"\n"),
            "PNG or WebP",
        );
    }
    let webp = base64::engine::general_purpose::STANDARD.encode(b"RIFF\x10\0\0\0WEBPVP8 ");
    assert_valid(&format!("{METADATA_BASE}metadata:\n  icon: \"{webp}\"\n"));
}

#[test]
fn paging_keys_are_refused_on_endpoints_that_return_one_result() {
    let yaml = |endpoint: &str, fields: &str, key: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  {endpoint}:\n    route: \"/x\"\n    {key}\n    fields:\n{fields}"
        )
    };
    let details = "      id: 'dom(\".id\").text()'\n      title: 'dom(\".t\").text()'\n      \
                   status: 'dom(\".s\").text()'\n";
    let pages = "      url: 'self.attr(\"src\")'\n      index: 'self.index()'\n";
    for (endpoint, fields) in [("manga_details", details), ("pages", pages)] {
        for (key, name) in [
            ("has_next_page: true", "has_next_page"),
            ("total_pages: 3", "total_pages"),
            (
                "pagination:\n      offset_type: page\n      offset_param: p\n      native_page_size: 20",
                "pagination",
            ),
        ] {
            assert_invalid_containing(
                &yaml(endpoint, fields, key),
                &format!("endpoints.{endpoint}.{name}: only popular, search and chapter_list page"),
            );
        }
    }
    let list = "      id: 'dom(\".id\").text()'\n      title: 'dom(\".t\").text()'\n";
    assert_valid(&yaml("search", list, "total_pages: 3"));
}

#[test]
fn arithmetic_in_a_request_template_is_refused_but_text_templates_are_not() {
    let yaml = |route: &str, key: &str, value: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  search:\n    route: \"{route}\"\n    {key}:\n      \
             v: \"{value}\"\n    fields:\n      id: 'dom(\".id\").text()'\n      \
             title: 'dom(\".t\").text()'\n"
        )
    };
    for value in ["$page_size$ * ($page$ - 1)", "$page$+1", "($page$)"] {
        for key in ["queries", "headers"] {
            assert_invalid_containing(
                &yaml("/s", key, value),
                &format!("{key}.v: {value:?} is arithmetic"),
            );
        }
    }
    assert_invalid_containing(
        &yaml("$page$*2", "queries", "x"),
        "route: \"$page$*2\" is arithmetic",
    );
    for value in ["id-$query$", "$query$ extra", "$query$-$page$", "$query$"] {
        assert_valid(&yaml("/s/$query$/2", "queries", value));
    }
    assert_valid(&yaml("/$query$", "headers", "Bearer $query$"));
}

#[test]
fn a_number_after_a_text_placeholder_is_a_literal_suffix_not_arithmetic() {
    let pages = |route: &str, query: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  pages:\n    route: \"{route}\"\n    queries:\n      \
             v: \"{query}\"\n    container: \".p\"\n    fields:\n      index: \"index()\"\n      \
             url: 'self.attr(\"src\")'\n"
        )
    };
    assert_valid(&pages("/read/$chapter_id$-1", "$manga_id$-2"));
    assert_valid(&pages("/read/$chapter_id$", "($manga_id$)"));
    assert_invalid_containing(&pages("/read/$chapter_id$", "$page$-1"), "is arithmetic");
}

#[test]
fn base_url_and_literal_page_urls_must_be_http() {
    let with_base = |base: &str| {
        format!("id: scheme\nname: Scheme\nversion: \"0.1.0\"\nbase_url: \"{base}\"\n")
    };
    for bad in [
        "ftp://example.com",
        "file:///srv/manga",
        "example.com",
        "javascript:x",
    ] {
        assert_invalid_containing(&with_base(bad), "base_url: only http and https");
    }
    assert_valid(&with_base("https://example.com"));

    let with_page_url = |page_url: &str| {
        format!(
            "{METADATA_BASE}browser_scripts:\n  grab: |\n    passPayload(\"{{}}\");\nendpoints:\n  \
             manga_details:\n    via: browser_payload\n    page_url: \"{page_url}\"\n    \
             script: grab\n    container: \":root\"\n    fields:\n      id: '\"$manga_id$\"'\n      \
             title: 'self.ptr(\"/t\").str()'\n      status: '\"unknown\"'\n"
        )
    };
    for bad in [
        "javascript:passPayload(1)",
        "file:///m/$manga_id$",
        "ftp://h/$manga_id$",
    ] {
        assert_invalid_containing(&with_page_url(bad), "page_url: only http and https");
    }
    for ok in ["https://example.com/m/$manga_id$", "$manga_id$"] {
        assert_valid(&with_page_url(ok));
    }
}

#[test]
fn an_element_method_on_a_receiver_that_cannot_be_an_element_is_refused() {
    let yaml = |expr: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  search:\n    route: \"/s\"\n    container: \".item\"\n    \
             fields:\n      id: 'self.attr(\"data-id\")'\n      title: '{expr}'\n"
        )
    };
    for (expr, receiver) in [
        (r#"self.select("img").attr("src")"#, "a list"),
        (r#"self.select("a").text()"#, "a list"),
        (r#"self.first("a").text().first("b")"#, "a string"),
        (r#""x".attr("y")"#, "a string"),
        (r#"self.attr("n").int().text()"#, "a number"),
        (r#"self.first("a").has_class("x").text()"#, "a boolean"),
        (r#"json("/id").text()"#, "a JSON value"),
        (r#"self.ptr("/id").text()"#, "a JSON value"),
    ] {
        assert_invalid_containing(
            &yaml(expr),
            &format!(
                "endpoints.search.fields.title: this method needs an HTML element, but its receiver is {receiver}"
            ),
        );
    }
    for ok in [
        r#"self.first("img").attr("src")"#,
        r#"self.select("a").at(0).text()"#,
        r#"self.select("a").map(self.attr("href")).join(",")"#,
        r#"dom("h1").text().trim()"#,
    ] {
        assert_valid(&yaml(ok));
    }
}

#[test]
fn the_selected_chapter_sort_is_a_chapter_list_argument() {
    let yaml = |endpoint: &str, fields: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  {endpoint}:\n    route: \"/c/$manga_id$\"\n    \
             queries:\n      order: $sort$\n    fields:\n{fields}"
        )
    };
    assert_valid(&yaml(
        "chapter_list",
        "      id: 'self.attr(\"data-id\")'\n",
    ));
    assert_invalid_containing(
        &yaml(
            "manga_details",
            "      id: '\"$manga_id$\"'\n      title: 'dom(\"h1\").text()'\n      status: '\"unknown\"'\n",
        ),
        "sort",
    );
}

#[test]
fn each_chaining_phase_may_only_use_the_context_it_has() {
    let yaml = |then: &str, dedup: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  popular:\n    route: \"/p\"\n    container: \".item\"\n    \
             fields:\n      id: 'self.attr(\"data-id\")'\n      title: 'self.first(\".t\").text()'\n    \
             then:\n      - endpoint: manga_details\n{then}        merge_as: banner\n    \
             for_each:\n      - endpoint: manga_details\n        url_expr: 'self.first(\"a\").attr(\"href\")'\n        \
             merge_as: details\n{dedup}  manga_details:\n    route: \"/m/$manga_id$\"\n    container: \".m\"\n    \
             fields:\n      id: '\"$manga_id$\"'\n      title: 'self.first(\"h1\").text()'\n      status: '\"unknown\"'\n"
        )
    };
    let url = |e: &str| format!("        url_expr: '{e}'\n");
    let then_ok = url(r#"dom(".banner").attr("href")"#);
    assert_valid(&yaml(&then_ok, ""));
    for (then, needle) in [
        (
            url(r#"self.first(".banner").attr("href")"#),
            "then[0].url_expr: a then step runs once per document",
        ),
        (
            format!("{then_ok}        on_failure: 'index()'\n"),
            "then[0].on_failure: a then step runs once per document",
        ),
    ] {
        assert_invalid_containing(&yaml(&then, ""), needle);
    }
    assert_invalid_containing(
        &yaml(&then_ok, "        deduplicate_by: 'dom(\".id\").text()'\n"),
        "deduplicate_by: runs on the finished JSON row",
    );
    assert_valid(&yaml(
        &then_ok,
        "        deduplicate_by: 'self.ptr(\"/details/title\").str()'\n",
    ));
}

#[test]
fn declared_operation_limits_must_stay_within_the_hard_caps() {
    let yaml = |key: &str, value: u64| {
        format!("{METADATA_BASE}metadata:\n  rate_limit:\n    {key}: {value}\n")
    };
    for (key, cap) in [
        ("max_requests", 1024),
        ("max_response_bytes", 256 * 1024 * 1024),
        ("max_operation_seconds", 600),
    ] {
        assert_valid(&yaml(key, cap));
        assert_invalid_containing(&yaml(key, cap + 1), &format!("metadata.rate_limit.{key}"));
        assert_invalid_containing(&yaml(key, 0), &format!("metadata.rate_limit.{key}"));
        assert_valid(&yaml(key, 60));
    }
}

#[test]
fn cursor_pagination_needs_a_cursor_field_on_a_json_endpoint() {
    let yaml = |kind: &str, pagination: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  search:\n    route: \"/s\"\n    type: {kind}\n    \
             container: \"{c}\"\n    pagination:\n      native_page_size: 32\n      \
             offset_param: after\n{pagination}    fields:\n      id: 'self.attr(\"i\")'\n      \
             title: 'self.attr(\"t\")'\n",
            c = if kind == "json" { "/items" } else { ".item" }
        )
    };
    let cursor = "      offset_type: cursor\n      cursor_field: /next\n";
    assert_invalid_containing(
        &yaml("json", "      offset_type: cursor\n"),
        "pagination.cursor_field: required for offset_type: cursor",
    );
    assert_invalid_containing(
        &yaml(
            "json",
            "      offset_type: cursor\n      cursor_field: next\n",
        ),
        "must be a JSON pointer",
    );
    assert_invalid_containing(
        &yaml("html", cursor),
        "cursor pagination reads a JSON response",
    );

    let browser = |offset: &str| {
        format!(
            "{METADATA_BASE}browser_scripts:\n  grab: |\n    passPayload(\"{{}}\");\nendpoints:\n  \
             search:\n    via: browser_payload\n    page_url: \"https://example.com/s\"\n    \
             script: grab\n    type: json\n    container: \"/items\"\n    pagination:\n      \
             native_page_size: 32\n      offset_param: after\n{offset}    fields:\n      \
             id: 'self.ptr(\"/i\").str()'\n      title: 'self.ptr(\"/t\").str()'\n"
        )
    };
    assert_invalid_containing(&browser(cursor), "a direct (not browser) endpoint");
    assert_valid(&browser("      offset_type: item\n"));
    assert_valid(&browser("      offset_type: page\n"));
    assert_invalid_containing(
        &yaml(
            "json",
            "      offset_type: item\n      cursor_field: /next\n",
        ),
        "cursor_field only applies to offset_type: cursor",
    );
}

#[test]
fn a_request_body_must_be_well_formed_and_sendable() {
    let yaml = |method: &str, body: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  search:\n    route: \"/s\"\n    method: {method}\n    \
             body:\n{body}    fields:\n      id: 'self.attr(\"i\")'\n      title: 'self.attr(\"t\")'\n"
        )
    };
    let json = "      type: json\n      content:\n        q: \"$query$\"\n";
    assert_valid(&yaml("POST", json));
    for (method, body, needle) in [
        ("GET", json, "a GET request has no body"),
        (
            "POST",
            "      type: raw\n      content: \"$query$\"\n",
            "a raw body needs content_type",
        ),
        (
            "POST",
            "      type: json\n      content_type: text/plain\n      content:\n        q: 1\n",
            "content_type is set by json and form bodies",
        ),
        (
            "POST",
            "      type: form\n      content: \"q=1\"\n",
            "form content must be a map",
        ),
        (
            "POST",
            "      type: json\n      content:\n        q: \"$nope$\"\n",
            "nope",
        ),
    ] {
        assert_invalid_containing(&yaml(method, body), needle);
    }
}

#[test]
fn a_static_total_pages_under_pagination_is_counted_in_native_pages() {
    use kani_cli::yaml::model::ValidatedTotalPages;
    let yaml = |pagination: &str| {
        format!(
            "{METADATA_BASE}endpoints:\n  search:\n    route: \"/s\"\n    total_pages: 12\n{pagination}    \
             container: \".i\"\n    fields:\n      id: 'self.attr(\"id\")'\n      title: 'self.text()'\n"
        )
    };
    let paged = validate_str(&yaml(
        "    pagination:\n      native_page_size: 32\n      offset_param: offset\n      offset_type: item\n",
    ))
    .unwrap();
    assert!(
        matches!(
            paged.search.as_ref().unwrap().total_pages,
            ValidatedTotalPages::Scalar(_)
        ),
        "a scalar reaches the host, which restates it in the client's page size"
    );
    let plain = validate_str(&yaml("")).unwrap();
    assert!(matches!(
        plain.search.as_ref().unwrap().total_pages,
        ValidatedTotalPages::Static(12)
    ));
}
