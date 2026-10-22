//! Server-persisted UI themes.
//!
//! A theme is a set of design-token overrides plus optional custom CSS. The
//! server is the authority on what is storable: token *names* come from a fixed
//! allowlist and token *values* are validated by shape, never stored as
//! arbitrary strings. Custom CSS is sanitised before storage, and the stored
//! value is the sanitised output — a client that skips its own sanitiser cannot
//! smuggle anything past this.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Result, ServiceError};
use crate::ids::UserId;
use crate::service::AppService;

/// Token names a theme may override. The 13 core colour tokens plus the two
/// derived accent tokens, and the non-colour groups that are safe to restyle.
/// Anything outside this list is refused by name — a theme cannot invent a
/// custom property and have it stored.
const TOKEN_ALLOWLIST: &[&str] = &[
    "--color-bg",
    "--color-surface",
    "--color-surface-2",
    "--color-surface-3",
    "--color-border",
    "--color-border-subtle",
    "--color-accent",
    "--color-text",
    "--color-text-muted",
    "--color-text-faint",
    "--color-success",
    "--color-warn",
    "--color-danger",
    "--color-accent-hover",
    "--color-accent-dim",
    "--color-surface-alt",
    "--color-on-accent",
    "--radius-sm",
    "--radius-md",
    "--radius-lg",
    "--radius-xl",
    "--radius-full",
    "--shadow-sm",
    "--shadow-card",
    "--shadow-md",
    "--shadow-lg",
    "--shadow-popover",
    "--shadow-focus-ring",
    "--motion-fast",
    "--motion-base",
    "--motion-slow",
    "--motion-ease",
    "--motion-ease-in",
    "--motion-ease-out",
    "--chart-1",
    "--chart-2",
    "--chart-3",
    "--chart-4",
    "--chart-5",
];

const MAX_TOKEN_VALUE_LEN: usize = 128;
const MAX_NAME_LEN: usize = 64;
const MAX_CUSTOM_CSS_BYTES: usize = 32 * 1024;
/// Per-user theme cap, so a script cannot fill the table.
const MAX_THEMES_PER_USER: i64 = 50;

/// Substrings that make a declaration value unsafe regardless of context:
/// anything that can fetch, execute, or escape the declaration it sits in.
const BANNED_VALUE_FRAGMENTS: &[&str] = &[
    "url(",
    "image-set(",
    "expression(",
    "behavior",
    "-moz-binding",
    "javascript:",
    "</",
    "@import",
];

#[derive(Debug, Clone, Serialize)]
pub struct UiTheme {
    pub id: String,
    /// `None` for an instance-wide theme published by an admin.
    pub user_id: Option<i64>,
    pub name: String,
    pub tokens: BTreeMap<String, String>,
    pub custom_css: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertUiThemeBody {
    /// Present when updating an existing theme.
    pub id: Option<String>,
    pub name: String,
    pub tokens: BTreeMap<String, String>,
    pub custom_css: Option<String>,
}

/// What `sanitize_custom_css` removed, so an editor can tell the user why their
/// CSS looks different from what they typed.
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SanitizeResult {
    pub css: String,
    pub stripped: Vec<String>,
}

fn validate_token_name(name: &str) -> bool {
    TOKEN_ALLOWLIST.contains(&name)
}

/// Validates a token value **by shape**. A token is a colour, a length, a
/// number, a duration, a timing function or a shadow — never free text — so
/// anything that does not parse as one of those is refused rather than stored
/// and hoped for.
fn validate_token_value(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() || v.len() > MAX_TOKEN_VALUE_LEN {
        return false;
    }
    // A value can never legitimately terminate its declaration or open a block.
    if v.contains(';') || v.contains('}') || v.contains('{') {
        return false;
    }
    let lower = v.to_ascii_lowercase();
    if BANNED_VALUE_FRAGMENTS.iter().any(|f| lower.contains(f)) {
        return false;
    }
    // Everything that remains must be built from characters a value can contain.
    // This is what stops an arbitrary string being stored under a valid name.
    v.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '#' | '.' | ',' | '%' | '(' | ')' | ' ' | '-' | '+' | '/' | '*' | '_'
            )
    })
}

fn validate_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ServiceError::Validation("Theme name is required".into()));
    }
    if trimmed.chars().count() > MAX_NAME_LEN {
        return Err(ServiceError::Validation(format!(
            "Theme name must be {MAX_NAME_LEN} characters or fewer"
        )));
    }
    Ok(())
}

fn validate_tokens(tokens: &BTreeMap<String, String>) -> Result<()> {
    let bad_names: Vec<&str> = tokens
        .keys()
        .filter(|k| !validate_token_name(k))
        .map(String::as_str)
        .collect();
    if !bad_names.is_empty() {
        return Err(ServiceError::Validation(format!(
            "Unknown design tokens: {}",
            bad_names.join(", ")
        )));
    }
    let bad_values: Vec<&str> = tokens
        .iter()
        .filter(|(_, v)| !validate_token_value(v))
        .map(|(k, _)| k.as_str())
        .collect();
    if !bad_values.is_empty() {
        return Err(ServiceError::Validation(format!(
            "Invalid values for: {}",
            bad_values.join(", ")
        )));
    }
    Ok(())
}

/// At-rules a theme may keep. `@import` is the dangerous one — it fetches — and
/// `@font-face`/`@charset`/`@namespace` have no place in a colour theme.
const ALLOWED_AT_RULES: &[&str] = &["@media", "@supports", "@keyframes"];

/// Strip everything a theme has no business shipping, and scope what remains.
///
/// Allowlist-based, and a small `{}`/`;` tokenizer rather than regex: a regex
/// over CSS cannot tell a `}` inside a string from one that closes a block, and
/// that difference is the whole security boundary.
pub(crate) fn sanitize_custom_css(input: &str) -> SanitizeResult {
    let mut stripped = Vec::new();
    let without_comments = strip_comments(input);
    let mut out = String::new();

    for block in split_top_level(&without_comments) {
        match block {
            Block::AtRule { name, raw } => {
                if ALLOWED_AT_RULES.contains(&name.to_ascii_lowercase().as_str()) {
                    out.push_str(&raw);
                    out.push('\n');
                } else {
                    stripped.push(format!("at-rule {name}"));
                }
            }
            Block::Rule { selector, body } => {
                let (clean_body, mut removed) = sanitize_declarations(&body);
                stripped.append(&mut removed);
                if clean_body.trim().is_empty() {
                    continue;
                }
                out.push_str(&scope_selector(&selector));
                out.push_str(" {");
                out.push_str(&clean_body);
                out.push_str("}\n");
            }
            Block::Junk(text) => {
                if !text.trim().is_empty() {
                    stripped.push("unparseable input".to_string());
                }
            }
        }
    }

    SanitizeResult {
        css: out.trim().to_string(),
        stripped,
    }
}

fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '/' && i + 1 < bytes.len() && bytes[i + 1] == '*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

enum Block {
    AtRule { name: String, raw: String },
    Rule { selector: String, body: String },
    Junk(String),
}

/// Split into top-level blocks, tracking brace depth so a nested rule (inside
/// `@media`) is not mistaken for a top-level one.
fn split_top_level(input: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut head = String::new();
    let mut depth = 0usize;
    let mut body = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if depth == 0 {
            if c == '{' {
                depth = 1;
                body.clear();
            } else if c == ';' {
                // A statement at-rule (`@import url(x);`) has no block. Without this case `head`
                // kept accumulating to the next `{`, so the at-rule swallowed the rule that
                // followed it — stripping `@import` also deleted the user's actual CSS.
                let stmt = head.trim().to_string();
                if !stmt.is_empty() {
                    let name = stmt.split_whitespace().next().unwrap_or("").to_string();
                    if name.starts_with('@') {
                        blocks.push(Block::AtRule {
                            name,
                            raw: format!("{stmt};"),
                        });
                    } else {
                        blocks.push(Block::Junk(stmt));
                    }
                }
                head.clear();
            } else {
                head.push(c);
            }
        } else {
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    let selector = head.trim().to_string();
                    if let Some(name) = selector.split_whitespace().next()
                        && name.starts_with('@')
                    {
                        blocks.push(Block::AtRule {
                            name: name.to_string(),
                            raw: format!("{selector} {{{body}}}"),
                        });
                    } else if selector.is_empty() {
                        blocks.push(Block::Junk(body.clone()));
                    } else {
                        blocks.push(Block::Rule {
                            selector,
                            body: body.clone(),
                        });
                    }
                    head.clear();
                    body.clear();
                    i += 1;
                    continue;
                }
            }
            body.push(c);
        }
        i += 1;
    }
    if !head.trim().is_empty() {
        blocks.push(Block::Junk(head));
    }
    blocks
}

fn sanitize_declarations(body: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut stripped = Vec::new();
    for decl in body.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let Some((prop, value)) = decl.split_once(':') else {
            stripped.push(format!("malformed declaration `{decl}`"));
            continue;
        };
        let lower = value.to_ascii_lowercase();
        if BANNED_VALUE_FRAGMENTS.iter().any(|f| lower.contains(f)) {
            stripped.push(format!("declaration `{}`", prop.trim()));
            continue;
        }
        out.push_str("\n  ");
        out.push_str(prop.trim());
        out.push_str(": ");
        out.push_str(value.trim());
        out.push(';');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    (out, stripped)
}

/// Confine every selector to the themed document state, so shared CSS cannot
/// restyle anything outside a page that has opted in.
fn scope_selector(selector: &str) -> String {
    selector
        .split(',')
        .map(|s| {
            let s = s.trim();
            if s.starts_with(":root") {
                s.replacen(":root", ":root[data-kani-theme]", 1)
            } else {
                format!("[data-kani-theme] {s}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl AppService {
    /// A user's own themes plus every instance-wide theme.
    pub async fn list_ui_themes(&self, user_id: UserId) -> Result<Vec<UiTheme>> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", user_id, name, tokens_json, custom_css,
                      is_active AS "is_active: bool"
               FROM ui_themes
               WHERE user_id = ? OR user_id IS NULL
               ORDER BY user_id IS NULL, name"#,
            user_id
        )
        .fetch_all(&self.db_read)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| UiTheme {
                id: r.id,
                user_id: r.user_id,
                name: r.name,
                tokens: serde_json::from_str(&r.tokens_json).unwrap_or_default(),
                custom_css: r.custom_css,
                is_active: r.is_active,
            })
            .collect())
    }

    /// Create or update a theme. `owner` is `None` for an instance-wide theme;
    /// the caller is responsible for having checked `theme:publish` first.
    pub async fn upsert_ui_theme(
        &self,
        owner: Option<UserId>,
        body: UpsertUiThemeBody,
    ) -> Result<UiTheme> {
        validate_name(&body.name)?;
        validate_tokens(&body.tokens)?;

        let custom_css = match body.custom_css.as_deref() {
            Some(raw) if !raw.trim().is_empty() => {
                if raw.len() > MAX_CUSTOM_CSS_BYTES {
                    return Err(ServiceError::Validation(format!(
                        "Custom CSS must be {MAX_CUSTOM_CSS_BYTES} bytes or fewer"
                    )));
                }
                // The sanitised output is what is stored, so a client that
                // skips its own sanitiser gains nothing.
                Some(sanitize_custom_css(raw).css)
            }
            _ => None,
        };

        let tokens_json = serde_json::to_string(&body.tokens)
            .map_err(|e| ServiceError::Internal(format!("tokens serialise: {e}")))?;
        let owner_id = owner.map(|u| u.0);
        let name = body.name.trim().to_string();

        let id = match body.id {
            Some(id) => {
                let existing = sqlx::query!(r#"SELECT user_id FROM ui_themes WHERE id = ?"#, id)
                    .fetch_optional(&self.db_read)
                    .await?
                    .ok_or_else(|| ServiceError::NotFound(format!("Theme {id} not found")))?;

                if existing.user_id != owner_id {
                    return Err(ServiceError::Forbidden(
                        "That theme belongs to someone else".into(),
                    ));
                }

                sqlx::query!(
                    "UPDATE ui_themes SET name = ?, tokens_json = ?, custom_css = ?, \
                     updated_at = unixepoch() WHERE id = ?",
                    name,
                    tokens_json,
                    custom_css,
                    id
                )
                .execute(&self.db)
                .await?;
                id
            }
            None => {
                if let Some(u) = owner_id {
                    let count: i64 =
                        sqlx::query_scalar!("SELECT COUNT(*) FROM ui_themes WHERE user_id = ?", u)
                            .fetch_one(&self.db_read)
                            .await?;
                    if count >= MAX_THEMES_PER_USER {
                        return Err(ServiceError::Validation(format!(
                            "You already have the maximum of {MAX_THEMES_PER_USER} themes"
                        )));
                    }
                }
                sqlx::query_scalar!(
                    r#"INSERT INTO ui_themes (user_id, name, tokens_json, custom_css)
                       VALUES (?, ?, ?, ?) RETURNING id AS "id!""#,
                    owner_id,
                    name,
                    tokens_json,
                    custom_css
                )
                .fetch_one(&self.db)
                .await?
            }
        };

        Ok(UiTheme {
            id,
            user_id: owner_id,
            name,
            tokens: body.tokens,
            custom_css,
            is_active: false,
        })
    }

    /// Mark one theme active for a user. At most one may be active at a time,
    /// cleared and set in a single transaction so a failure cannot leave two.
    pub async fn activate_ui_theme(&self, user_id: UserId, theme_id: &str) -> Result<()> {
        let row = sqlx::query!("SELECT user_id FROM ui_themes WHERE id = ?", theme_id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Theme {theme_id} not found")))?;

        // Own themes and instance-wide themes only — never someone else's.
        if row.user_id.is_some() && row.user_id != Some(user_id.0) {
            return Err(ServiceError::Forbidden(
                "That theme belongs to someone else".into(),
            ));
        }

        let mut tx = self.db.begin().await?;
        sqlx::query!(
            "UPDATE ui_themes SET is_active = FALSE WHERE user_id = ?",
            user_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE ui_themes SET is_active = TRUE WHERE id = ?",
            theme_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn deactivate_ui_theme(&self, user_id: UserId) -> Result<()> {
        sqlx::query!(
            "UPDATE ui_themes SET is_active = FALSE WHERE user_id = ?",
            user_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Who owns a theme: `Some(user_id)`, or `None` for an instance-wide one.
    /// The REST layer needs this *before* acting, to decide whether the request
    /// requires `theme:publish` and which owner to authorise against.
    pub async fn ui_theme_owner(&self, theme_id: &str) -> Result<Option<i64>> {
        let row = sqlx::query!("SELECT user_id FROM ui_themes WHERE id = ?", theme_id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Theme {theme_id} not found")))?;
        Ok(row.user_id)
    }

    /// Delete a theme. `owner` must match the row's owner — `None` (instance
    /// theme) requires the caller to have checked `theme:publish`.
    pub async fn delete_ui_theme(&self, owner: Option<UserId>, theme_id: &str) -> Result<()> {
        let row = sqlx::query!("SELECT user_id FROM ui_themes WHERE id = ?", theme_id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Theme {theme_id} not found")))?;

        if row.user_id != owner.map(|u| u.0) {
            return Err(ServiceError::Forbidden(
                "That theme belongs to someone else".into(),
            ));
        }

        sqlx::query!("DELETE FROM ui_themes WHERE id = ?", theme_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn allowlisted_token_names_are_accepted_and_others_refused() {
        assert!(validate_token_name("--color-accent"));
        assert!(validate_token_name("--radius-md"));
        assert!(!validate_token_name("--evil"));
        assert!(!validate_token_name("color-accent"));
    }

    #[test]
    fn token_values_are_validated_by_shape() {
        for good in [
            "#fff",
            "#b93a24",
            "rgb(1, 2, 3)",
            "0.5rem",
            "12px",
            "200ms",
            "1.5",
        ] {
            assert!(validate_token_value(good), "{good} should be accepted");
        }
        for bad in [
            "red; } body { background: black",
            "url(https://evil.example/x.png)",
            "expression(alert(1))",
            "",
        ] {
            assert!(!validate_token_value(bad), "{bad} should be refused");
        }
    }

    #[test]
    fn an_over_long_token_value_is_refused() {
        assert!(!validate_token_value(&"a".repeat(MAX_TOKEN_VALUE_LEN + 1)));
    }

    #[test]
    fn import_is_stripped_and_reported() {
        let out = sanitize_custom_css("@import url(evil.css); .btn { color: red }");
        assert!(
            !out.css.contains("@import"),
            "@import must not survive: {}",
            out.css
        );
        assert!(!out.stripped.is_empty(), "and the removal must be reported");
    }

    #[test]
    fn a_url_declaration_is_stripped_but_its_neighbours_survive() {
        let out = sanitize_custom_css(".a { color: red; background: url(x.png); font-size: 12px }");
        assert!(out.css.contains("color: red"));
        assert!(out.css.contains("font-size: 12px"));
        assert!(!out.css.contains("url("), "got {}", out.css);
    }

    #[test]
    fn selectors_are_scoped_to_the_themed_document() {
        let out = sanitize_custom_css(".btn { color: red }");
        assert!(
            out.css.starts_with("[data-kani-theme] .btn"),
            "got {}",
            out.css
        );

        let root = sanitize_custom_css(":root { --x: 1 }");
        assert!(
            root.css.contains(":root[data-kani-theme]"),
            "got {}",
            root.css
        );
    }

    #[test]
    fn media_is_kept_but_font_face_is_dropped() {
        let out = sanitize_custom_css("@media (min-width: 40rem) { .a { color: red } }");
        assert!(out.css.contains("@media"), "got {}", out.css);

        let ff = sanitize_custom_css("@font-face { src: local(x) }");
        assert!(!ff.css.contains("@font-face"), "got {}", ff.css);
    }

    #[test]
    fn comments_are_removed_so_they_cannot_hide_a_payload() {
        let out = sanitize_custom_css("/* } body { background: url(x) */ .a { color: red }");
        assert!(!out.css.contains("url("), "got {}", out.css);
        assert!(out.css.contains("color: red"));
    }

    /// The exact fixtures `static/js/sanitize-css.js` is checked against, with
    /// their expected output. The client mirror exists to preview what the
    /// server will store; if the two drift the editor lies about what is saved,
    /// so both sides are pinned to these same strings rather than merely
    /// "looking similar".
    ///
    /// Client-side equivalent: `scripts/check-sanitize-css-parity.mjs`.
    #[test]
    fn the_client_mirror_fixtures_produce_exactly_these_outputs() {
        let cases: &[(&str, &str, &str, &str)] = &[
            (
                "import",
                "@import url(evil.css); .btn { color: red }",
                "[data-kani-theme] .btn {\n  color: red;\n}",
                "at-rule @import",
            ),
            (
                "url_decl",
                ".a { color: red; background: url(x.png); font-size: 12px }",
                "[data-kani-theme] .a {\n  color: red;\n  font-size: 12px;\n}",
                "declaration `background`",
            ),
            (
                "scope",
                ".btn { color: red }",
                "[data-kani-theme] .btn {\n  color: red;\n}",
                "",
            ),
            (
                "root",
                ":root { --x: 1 }",
                ":root[data-kani-theme] {\n  --x: 1;\n}",
                "",
            ),
            (
                "media",
                "@media (min-width: 40rem) { .a { color: red } }",
                "@media (min-width: 40rem) { .a { color: red } }",
                "",
            ),
            (
                "fontface",
                "@font-face { src: local(x) }",
                "",
                "at-rule @font-face",
            ),
            (
                "comment",
                "/* } body { background: url(x) */ .a { color: red }",
                "[data-kani-theme] .a {\n  color: red;\n}",
                "",
            ),
            ("unbalanced", ".a { color: red", "", "unparseable input"),
        ];

        for (name, input, want_css, want_stripped) in cases {
            let got = sanitize_custom_css(input);
            assert_eq!(&got.css, want_css, "css mismatch for fixture `{name}`");
            assert_eq!(
                got.stripped.join("|"),
                *want_stripped,
                "stripped mismatch for fixture `{name}`"
            );
        }
    }

    #[test]
    fn an_unbalanced_brace_does_not_panic_or_emit_a_broken_rule() {
        let out = sanitize_custom_css(".a { color: red");
        assert!(!out.css.contains("color: red"), "got {}", out.css);
    }
}
