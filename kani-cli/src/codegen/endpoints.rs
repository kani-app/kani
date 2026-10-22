//! Emit per-endpoint MangaExtension trait method bodies.

use super::blueprint::{
    emit_blueprint_bytes, emit_blueprint_chain, emit_blueprint_chain_no_request,
};
use super::request::emit_request_block;
use crate::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedHnp, ValidatedPopular,
    ValidatedTotalPages,
};
use crate::yaml::schema::{EndpointVia, ResponseType, YamlIdEncoding};

/// Emits `let` bindings that decode each composite ID referenced by this
/// endpoint's route/queries (e.g. `$manga.hid$`) into local `&str`s
/// (`manga_hid`) before the request is built.
fn emit_composite_id_decode_prologue(ep: &ValidatedEndpoint) -> String {
    let mut out = String::new();
    for decode in &ep.composite_id_decodes {
        if decode.referenced_fields.is_empty() {
            continue;
        }
        let encoding_str = match decode.encoding {
            YamlIdEncoding::Base64Url => "kani_shared::ast::IdEncoding::Base64Url",
            YamlIdEncoding::Base64 => "kani_shared::ast::IdEncoding::Base64",
            YamlIdEncoding::Passthrough => "kani_shared::ast::IdEncoding::Passthrough",
            YamlIdEncoding::Hex => "kani_shared::ast::IdEncoding::Hex",
        };
        let field_names = decode
            .fields
            .iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let local = format!("__{}_decoded", decode.role);
        out.push_str(&format!(
            "let {local} = kani_shared::encoding::decode_composite({arg}, {delim:?}, &{encoding_str}, &[{field_names}]).map_err(|e| kani_shared::ExtensionError::parse(format!(\"{arg} {{:?}} is not a valid {role} id for this source: {{e}}\", {arg})))?;\n",
            arg = decode.fn_arg,
            role = decode.role,
            delim = decode.delimiter,
        ));
        for (idx, field) in decode.fields.iter().enumerate() {
            if !decode.referenced_fields.iter().any(|f| f == field) {
                continue;
            }
            out.push_str(&format!(
                "let {role}_{field} = {local}[{idx}].1.as_str();\n",
                role = decode.role,
            ));
        }
    }
    out
}

/// Returns `Some(code)` when `ep.via` is `BrowserPayload`, where `code`
/// captures the page payload via the browser runtime, parses it as JSON, and
/// extracts rows through the same Blueprint (`bp`) the caller has in scope.
fn try_emit_browser_fetch(ep: &ValidatedEndpoint) -> Option<String> {
    let EndpointVia::BrowserPayload = ep.via?;
    let page_url = ep.page_url.as_deref().unwrap_or("");
    let script_name = ep.script_name.as_deref().unwrap_or("");
    let const_name = format!(
        "SCRIPT_{}",
        script_name.to_uppercase().replace(['-', '.'], "_")
    );
    let rust_page_url = page_url
        .replace("$manga_id$", "{manga_id}")
        .replace("$chapter_id$", "{chapter_id}");
    let page_url_expr = if rust_page_url.contains('{') {
        format!("&format!(\"{rust_page_url}\")")
    } else {
        format!("\"{}\"", rust_page_url)
    };
    let timeout = ep.timeout_ms;
    let auto_scroll = ep.auto_scroll;
    Some(format!(
        "let _payload = kani_shared::host_abi::v8_context::capture_page_payload_configured({page_url_expr}, {const_name}, {timeout}, {auto_scroll})?;\n\
         let _json = kani_shared::host_abi::JsonHandle::parse(_payload.as_bytes())?;\n\
         let rows = extract::json(Some(_json.raw_handle()), &bp)?;"
    ))
}

pub(crate) fn emit_browser_script_statics(
    scripts: &std::collections::BTreeMap<String, String>,
) -> String {
    scripts
        .keys()
        .map(|name| {
            let const_name = format!("SCRIPT_{}", name.to_uppercase().replace(['-', '.'], "_"));
            format!("static {const_name}: &str = include_str!(\"scripts/{name}.js\");")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn emit_popular(
    popular: &ValidatedPopular,
    ext: &ValidatedExtension,
    embedded_bytes: bool,
) -> String {
    match popular {
        ValidatedPopular::Delegated {
            delegate_to,
            empty_without_filters,
        } => {
            let guard = if *empty_without_filters {
                "    if filters.is_empty() {\n        return Ok(MangaList { manga: vec![], has_next_page: false, total_pages: None });\n    }\n"
            } else {
                ""
            };
            let delegate_call = match delegate_to.as_str() {
                "search" => "self.search_manga(\"\", page, page_size, filters)",
                other => &format!("self.{other}(page, page_size, filters)"),
            };
            format!(
                "fn get_popular_manga(&self, page: i32, page_size: i32, filters: &[ActiveFilter]) -> ExtensionResult<MangaList> {{\n\
                 {guard}\
                 {delegate_call}\n\
                 }}"
            )
        }
        ValidatedPopular::Full(ep) => emit_manga_list_method(
            "get_popular_manga",
            "page: i32, page_size: i32, filters: &[ActiveFilter]",
            ep,
            ext,
            embedded_bytes,
            "popular",
        ),
    }
}

pub(crate) fn emit_search(
    ep: &ValidatedEndpoint,
    ext: &ValidatedExtension,
    embedded_bytes: bool,
) -> String {
    emit_manga_list_method(
        "search_manga",
        "query: &str, page: i32, page_size: i32, filters: &[ActiveFilter]",
        ep,
        ext,
        embedded_bytes,
        "search",
    )
}

pub(crate) fn emit_manga_details(
    ep: &ValidatedEndpoint,
    ext: &ValidatedExtension,
    embedded_bytes: bool,
) -> String {
    let decode_prologue = emit_composite_id_decode_prologue(ep);
    // The shared unpacker reads row 0 and maps its fields; it returns exactly the
    // method's `ExtensionResult<MangaInfo>`, so the body ends with the call.
    let unpack = format!(
        "kani_shared::unpack::unpack_manga_info(&rows, {})",
        emit_fn_args(ep)
    );
    let bp_chain = emit_blueprint_chain(ep, ext, "manga_details");

    if let Some(browser_fetch) = try_emit_browser_fetch(ep) {
        let bp_chain = emit_blueprint_chain_no_request(ep, ext, "manga_details");
        return format!(
            "fn get_manga_details(&self, manga_id: &str) -> ExtensionResult<MangaInfo> {{\n\
             {decode_prologue}\
             {bp_chain}\n\
             {browser_fetch}\n\
             {unpack}\n\
             }}"
        );
    }

    let req_block = emit_request_block(
        &ep.route,
        &ep.method,
        &ep.headers,
        &ep.queries,
        &[],
        None,
        Some("manga_details"),
        ep.body.as_ref(),
    );

    if embedded_bytes {
        let bp_bytes = emit_blueprint_bytes(ep, ext, "manga_details");
        let fetch = match ep.response_type {
            ResponseType::Html => {
                "let _doc = req.send_html()?;\nlet rows = extract_raw::html(Some(_doc.handle()), BP)?;"
            }
            ResponseType::Json => {
                "let _json = req.send_json_handle()?;\nlet rows = extract_raw::json(Some(_json.raw_handle()), BP)?;"
            }
        };
        format!(
            "fn get_manga_details(&self, manga_id: &str) -> ExtensionResult<MangaInfo> {{\n\
             {decode_prologue}\
             {bp_bytes}\n\
             {req_block}\n\
             {fetch}\n\
             {unpack}\n\
             }}"
        )
    } else {
        let extract_call = match ep.response_type {
            ResponseType::Json => "extract::json(None, &bp)?",
            ResponseType::Html => "extract::html(None, &bp)?",
        };
        format!(
            "fn get_manga_details(&self, manga_id: &str) -> ExtensionResult<MangaInfo> {{\n\
             {decode_prologue}\
             {req_block}\n\
             {bp_chain}\n\
             let rows = {extract_call};\n\
             {unpack}\n\
             }}"
        )
    }
}

fn emit_chapter_list_args(ep: &ValidatedEndpoint) -> String {
    let mut referenced = kani_shared::request::template_parts(&ep.route).1;
    referenced.extend(ep.body.iter().flat_map(|b| b.placeholders()));
    for q in &ep.queries {
        match &q.value {
            crate::yaml::model::QueryValue::Arg(name) => referenced.push(name.clone()),
            crate::yaml::model::QueryValue::Template(t) => {
                referenced.extend(kani_shared::request::template_parts(t).1)
            }
            crate::yaml::model::QueryValue::Static(_) => {}
        }
    }
    let uses = |name: &str| referenced.iter().any(|v| v == name);
    let mut lines = vec!["let _ = (page, page_size, &sort);".to_string()];
    if uses("page_size") || ep.pagination.is_some() {
        lines.push("let page_size = page_size.unwrap_or(100);".to_string());
    }
    if uses("sort") {
        lines.push("let sort = sort.unwrap_or_default();".to_string());
    }
    lines.join("\n") + "\n"
}

pub(crate) fn emit_chapter_list(
    ep: &ValidatedEndpoint,
    ext: &ValidatedExtension,
    embedded_bytes: bool,
) -> String {
    let decode_prologue = emit_composite_id_decode_prologue(ep);
    let arg_prologue = emit_chapter_list_args(ep);
    let req_block = emit_request_block(
        &ep.route,
        &ep.method,
        &ep.headers,
        &ep.queries,
        &[],
        None,
        Some("chapter_list"),
        ep.body.as_ref(),
    );

    let unpack = format!(
        "Ok(kani_shared::unpack::unpack_chapter_list(&rows, {}, {}, {}).logged(\"chapter_list\"))",
        emit_hnp_spec(&ep.has_next_page),
        emit_total_pages_spec(&ep.total_pages),
        emit_fn_args(ep),
    );

    if let Some(browser_fetch) = try_emit_browser_fetch(ep) {
        let bp_chain = emit_blueprint_chain_no_request(ep, ext, "chapter_list");
        return format!(
            "fn get_chapter_list(&self, manga_id: &str, page: i32, page_size: Option<i32>, sort: Option<String>) -> ExtensionResult<ChapterList> {{\n\
             {arg_prologue}\
             {decode_prologue}\
             {bp_chain}\n\
             {browser_fetch}\n\
             {unpack}\n\
             }}"
        );
    }

    if embedded_bytes {
        let bp_bytes = emit_blueprint_bytes(ep, ext, "chapter_list");
        let fetch = emit_raw_fetch(ep);
        format!(
            "fn get_chapter_list(&self, manga_id: &str, page: i32, page_size: Option<i32>, sort: Option<String>) -> ExtensionResult<ChapterList> {{\n\
             {arg_prologue}\
             {decode_prologue}\
             {bp_bytes}\n\
             {req_block}\n\
             {fetch}\n\
             {unpack}\n\
             }}"
        )
    } else {
        let bp_chain = emit_blueprint_chain(ep, ext, "chapter_list");
        let extract_call = emit_extract_call(ep);
        format!(
            "fn get_chapter_list(&self, manga_id: &str, page: i32, page_size: Option<i32>, sort: Option<String>) -> ExtensionResult<ChapterList> {{\n\
             {arg_prologue}\
             {decode_prologue}\
             {req_block}\n\
             {bp_chain}\n\
             let rows = {extract_call};\n\
             {unpack}\n\
             }}"
        )
    }
}

pub(crate) fn emit_pages(
    ep: &ValidatedEndpoint,
    ext: &ValidatedExtension,
    embedded_bytes: bool,
) -> String {
    let decode_prologue = emit_composite_id_decode_prologue(ep);
    let req_block = emit_request_block(
        &ep.route,
        &ep.method,
        &ep.headers,
        &ep.queries,
        &[],
        None,
        Some("pages"),
        ep.body.as_ref(),
    );

    let unpack = format!(
        "Ok(kani_shared::unpack::unpack_pages(&rows, {}))",
        emit_fn_args(ep)
    );

    // Un-underscore `manga_id` when the pages route actually uses it (most sources key pages on
    // chapter_id alone, but a route like `/manga/$manga_id$/chapter/$chapter_id$` needs it) or a
    // composite decode consumes it.
    let manga_param = if ep.route.contains("$manga_id$")
        || ep.composite_id_decodes.iter().any(|d| d.role == "manga")
    {
        "manga_id"
    } else {
        "_manga_id"
    };

    if let Some(browser_fetch) = try_emit_browser_fetch(ep) {
        let bp_chain = emit_blueprint_chain_no_request(ep, ext, "pages");
        return format!(
            "fn get_pages(&self, {manga_param}: &str, chapter_id: &str) -> ExtensionResult<Chapter> {{\n\
             {decode_prologue}\
             {bp_chain}\n\
             {browser_fetch}\n\
             {unpack}\n\
             }}"
        );
    }

    if embedded_bytes {
        let bp_bytes = emit_blueprint_bytes(ep, ext, "pages");
        let fetch = match ep.response_type {
            ResponseType::Html => {
                "let _doc = req.send_html()?;\nlet rows = extract_raw::html(Some(_doc.handle()), BP)?;"
            }
            ResponseType::Json => {
                "let _json = req.send_json_handle()?;\nlet rows = extract_raw::json(Some(_json.raw_handle()), BP)?;"
            }
        };
        format!(
            "fn get_pages(&self, {manga_param}: &str, chapter_id: &str) -> ExtensionResult<Chapter> {{\n\
             {decode_prologue}\
             {bp_bytes}\n\
             {req_block}\n\
             {fetch}\n\
             {unpack}\n\
             }}"
        )
    } else {
        let bp_chain = emit_blueprint_chain(ep, ext, "pages");
        let extract_call = match ep.response_type {
            ResponseType::Json => "extract::json(None, &bp)?",
            ResponseType::Html => "extract::html(None, &bp)?",
        };
        format!(
            "fn get_pages(&self, {manga_param}: &str, chapter_id: &str) -> ExtensionResult<Chapter> {{\n\
             {decode_prologue}\
             {req_block}\n\
             {bp_chain}\n\
             let rows = {extract_call};\n\
             {unpack}\n\
             }}"
        )
    }
}

fn emit_manga_list_method(
    method_name: &str,
    params: &str,
    ep: &ValidatedEndpoint,
    ext: &ValidatedExtension,
    embedded_bytes: bool,
    endpoint_id: &str,
) -> String {
    let req_block = emit_request_block(
        &ep.route,
        &ep.method,
        &ep.headers,
        &ep.queries,
        &ep.filter_mapping,
        ep.filter_format.as_ref(),
        Some(endpoint_id),
        ep.body.as_ref(),
    );
    let unpack = format!(
        "Ok(kani_shared::unpack::unpack_manga_list(&rows, {}, {}, {}).logged({endpoint_id:?}))",
        emit_hnp_spec(&ep.has_next_page),
        emit_total_pages_spec(&ep.total_pages),
        emit_fn_args(ep),
    );

    if let Some(browser_fetch) = try_emit_browser_fetch(ep) {
        let bp_chain = emit_blueprint_chain_no_request(ep, ext, endpoint_id);
        return format!(
            "fn {method_name}(&self, {params}) -> ExtensionResult<MangaList> {{\n\
             {bp_chain}\n\
             {browser_fetch}\n\
             {unpack}\n\
             }}"
        );
    }

    if embedded_bytes {
        let bp_bytes = emit_blueprint_bytes(ep, ext, endpoint_id);
        let fetch = emit_raw_fetch(ep);
        format!(
            "fn {method_name}(&self, {params}) -> ExtensionResult<MangaList> {{\n\
             {bp_bytes}\n\
             {req_block}\n\
             {fetch}\n\
             {unpack}\n\
             }}"
        )
    } else {
        let bp_chain = emit_blueprint_chain(ep, ext, endpoint_id);
        let extract_call = emit_extract_call(ep);
        format!(
            "fn {method_name}(&self, {params}) -> ExtensionResult<MangaList> {{\n\
             {req_block}\n\
             {bp_chain}\n\
             let rows = {extract_call};\n\
             {unpack}\n\
             }}"
        )
    }
}

/// Emit the `FnArgs` slice for the shared unpacker: fields whose value is a
/// method argument (`id: "$manga_id$"`) rather than extracted. The guest can't
/// inject them into its handle the way the interpreter injects into its Value, so
/// it hands them to the unpacker directly.
fn emit_fn_args(ep: &ValidatedEndpoint) -> String {
    let pairs: Vec<String> = ep
        .fields
        .iter()
        .filter_map(|f| match &f.source {
            FieldSource::FnArg(argname) => Some(format!("(\"{}\", {})", f.name, argname)),
            FieldSource::Blueprint(_) => None,
        })
        .collect();
    format!("&[{}]", pairs.join(", "))
}

fn emit_hnp_spec(hnp: &ValidatedHnp) -> String {
    match hnp {
        ValidatedHnp::Static(b) => format!("kani_shared::unpack::HasNextPage::Static({b})"),
        ValidatedHnp::Scalar(_) => "kani_shared::unpack::HasNextPage::FromScalar".to_string(),
        ValidatedHnp::Default => "kani_shared::unpack::HasNextPage::Derived { page }".to_string(),
    }
}

fn emit_extract_call(ep: &ValidatedEndpoint) -> String {
    let kind = match ep.response_type {
        ResponseType::Json => "json",
        ResponseType::Html => "html",
    };
    if ep.pagination.is_some() {
        format!("extract::paginated_{kind}(page, page_size, &bp)?")
    } else {
        format!("extract::{kind}(None, &bp)?")
    }
}

fn emit_raw_fetch(ep: &ValidatedEndpoint) -> String {
    match (ep.pagination.is_some(), &ep.response_type) {
        (true, ResponseType::Html) => {
            "let rows = extract_raw::paginated_html(page, page_size, req, BP)?;".to_string()
        }
        (true, ResponseType::Json) => {
            "let rows = extract_raw::paginated_json(page, page_size, req, BP)?;".to_string()
        }
        (false, ResponseType::Html) => {
            "let _doc = req.send_html()?;\nlet rows = extract_raw::html(Some(_doc.handle()), BP)?;"
                .to_string()
        }
        (false, ResponseType::Json) => {
            "let _json = req.send_json_handle()?;\nlet rows = extract_raw::json(Some(_json.raw_handle()), BP)?;"
                .to_string()
        }
    }
}

fn emit_total_pages_spec(tp: &ValidatedTotalPages) -> String {
    match tp {
        ValidatedTotalPages::Static(n) => {
            format!("kani_shared::unpack::TotalPages::Static({n})")
        }
        ValidatedTotalPages::None => "kani_shared::unpack::TotalPages::None".to_string(),
        ValidatedTotalPages::Scalar(_) => "kani_shared::unpack::TotalPages::FromScalar".to_string(),
    }
}
