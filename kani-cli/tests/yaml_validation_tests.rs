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
fn valid_cache_all_fields_and_scopes() {
    assert_valid(&format!(
        r#"{}
cache:
  search_results:
    scope: extension
    ttl: 1800
    max_entries: 200
    key_template: "search:{{query}}:{{page}}"
  user_prefs:
    scope: user
    ttl: 60
  install_state:
    scope: installation
"#,
        CACHE_BASE
    ));
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
fn invalid_cache_key_template_empty() {
    assert_invalid_containing(
        &format!(
            r#"{}
cache:
  bad_template:
    key_template: ""
"#,
            CACHE_BASE
        ),
        "'key_template' must not be empty",
    );
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
        "do not match a supported",
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
        "exceeding the",
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
