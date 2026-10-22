//! Emit Rust builder-chain source code from an `Expr` tree.
//! The output calls `kani_shared::ast::Expr::*` constructors and chained methods.

use kani_shared::ast::{
    Blueprint, Expr, ExprArena, ExprId, IdEncoding, OnFailurePolicy, Op, PadAlign,
};

pub fn emit_expr(expr: &Expr) -> String {
    match expr {
        Expr::SelfRef => "Expr::self_ref()".into(),
        Expr::Index => "Expr::index()".into(),
        Expr::Null => "Expr::null()".into(),
        Expr::Bool(true) => "Expr::true_val()".into(),
        Expr::Bool(false) => "Expr::false_val()".into(),
        Expr::Literal(s) => format!("Expr::lit(\"{}\")", escape(s)),
        Expr::Number(n) => format!("Expr::num({})", emit_float(*n)),
        Expr::Var(name) => format!("Expr::var(\"{}\")", escape(name)),
        Expr::Dom(sel) => format!("Expr::dom(\"{}\")", escape(sel)),
        Expr::Json(ptr) => format!("Expr::json_root(\"{}\")", escape(ptr)),
        Expr::Pref(key) => format!("Expr::pref(\"{}\")", escape(key)),

        Expr::Attr { target, name } => format!("{}.attr(\"{}\")", e(target), escape(name)),
        Expr::Text { target } => format!("{}.text()", e(target)),
        Expr::InnerHtml { target } => format!("{}.inner_html()", e(target)),
        Expr::Select { target, selector } => {
            format!("{}.select(\"{}\")", e(target), escape(selector))
        }
        Expr::First { target, selector } => {
            format!("{}.first(\"{}\")", e(target), escape(selector))
        }
        Expr::HasClass { target, class } => {
            format!("{}.has_class(\"{}\")", e(target), escape(class))
        }
        Expr::Children { target } => format!("{}.children()", e(target)),
        Expr::Split { target, delimiter } => {
            format!("{}.split(\"{}\")", e(target), escape(delimiter))
        }
        Expr::At { target, index } => format!("{}.at({})", e(target), index),
        Expr::Trim { target } => format!("{}.trim()", e(target)),
        Expr::Lower { target } => format!("{}.lower()", e(target)),
        Expr::Not { target } => format!("{}.not()", e(target)),
        Expr::StringLen { target } => format!("{}.string_len()", e(target)),
        Expr::ParseFloat { target } => format!("{}.parse_float()", e(target)),
        Expr::ParseInt { target } => format!("{}.parse_int()", e(target)),
        Expr::ToString { target } => format!("{}.stringify()", e(target)),
        Expr::DateParseRfc3339 { target } => format!("{}.date_parse_rfc3339()", e(target)),
        Expr::ArrayLen { target } => format!("{}.array_len()", e(target)),
        Expr::JsonKeys { target } => format!("{}.keys()", e(target)),
        Expr::JsonFold { target } => format!("{}.json_fold()", e(target)),
        Expr::JsonStr { target } => format!("{}.str_val()", e(target)),
        Expr::JsonInt { target } => format!("{}.int_val()", e(target)),
        Expr::JsonFloat { target } => format!("{}.float_val()", e(target)),
        Expr::JsonBool { target } => format!("{}.bool_val()", e(target)),

        Expr::Replace { target, from, to } => format!(
            "{}.replace(\"{}\", \"{}\")",
            e(target),
            escape(from),
            escape(to)
        ),
        Expr::Slice {
            target,
            start,
            end: None,
        } => format!("{}.slice({}, None)", e(target), start),
        Expr::Slice {
            target,
            start,
            end: Some(end),
        } => format!("{}.slice({}, Some({}))", e(target), start, end),
        Expr::StartsWith { target, prefix } => {
            format!("{}.starts_with(\"{}\")", e(target), escape(prefix))
        }
        Expr::EndsWith { target, suffix } => {
            format!("{}.ends_with(\"{}\")", e(target), escape(suffix))
        }
        Expr::Matches { target, pattern } => {
            format!("{}.matches(\"{}\")", e(target), escape(pattern))
        }
        Expr::Capture { target, pattern } => {
            format!("{}.capture(\"{}\")", e(target), escape(pattern))
        }
        Expr::DateParse { target, format } => {
            format!("{}.date_parse(\"{}\")", e(target), escape(format))
        }
        Expr::Join { target, delimiter } => {
            format!("{}.join(\"{}\")", e(target), escape(delimiter))
        }
        Expr::JsonPtr { target, pointer } => format!("{}.ptr(\"{}\")", e(target), escape(pointer)),

        Expr::Append { target, suffix } => format!("{}.append({})", e(target), e(suffix)),
        Expr::Prepend { target, prefix } => format!("{}.prepend({})", e(target), e(prefix)),
        Expr::Fallback { target, default } => format!("{}.fallback({})", e(target), e(default)),
        Expr::Map { target, transform } => format!("{}.map({})", e(target), e(transform)),
        Expr::FlatMap { target, transform } => format!("{}.flat_map({})", e(target), e(transform)),
        Expr::Filter { target, filter } => format!("{}.filter({})", e(target), e(filter)),
        Expr::ResolveUrl { target, base } => format!("{}.resolve_url({})", e(target), e(base)),
        Expr::JsonGet { target, key } => format!("{}.get({})", e(target), e(key)),

        Expr::Lookup { target, table } => {
            let entries: Vec<String> = table
                .iter()
                .map(|(k, v)| format!("(\"{}\", \"{}\")", escape(k), escape(v)))
                .collect();
            format!("{}.lookup(vec![{}])", e(target), entries.join(", "))
        }

        Expr::Fold {
            target,
            base,
            transform,
        } => format!("{}.fold({}, {})", e(target), e(base), e(transform)),
        Expr::JsonFind { target, key, value } => {
            format!("{}.find({}, {})", e(target), e(key), e(value))
        }

        Expr::BinaryOperation { op, lhs, rhs } => {
            let method = match op {
                Op::Add => "add",
                Op::Sub => "sub",
                Op::Mul => "mul",
                Op::Div => "div",
                Op::Eq => "eq",
                Op::Ne => "ne",
                Op::Lt => "lt",
                Op::Gt => "gt",
                Op::Le => "le",
                Op::Ge => "ge",
                Op::And => "and",
                Op::Or => "or",
            };
            format!("{}.{}({})", e(lhs), method, e(rhs))
        }

        Expr::If {
            condition,
            then,
            else_,
        } => format!(
            "Expr::if_then_else({}, {}, {})",
            e(condition),
            e(then),
            e(else_)
        ),
        Expr::Let { name, value, body } => format!(
            "Expr::let_bind(\"{}\", {}, {})",
            escape(name),
            e(value),
            e(body)
        ),

        Expr::List(items) => format!("Expr::list(vec![{}])", emit_list(items)),
        Expr::Concat(parts) => format!("Expr::concat(vec![{}])", emit_list(parts)),
        Expr::Merge(lists) => format!("Expr::merge(vec![{}])", emit_list(lists)),
        Expr::JsonArray(items) => format!("Expr::json_array(vec![{}])", emit_list(items)),
        Expr::Format { template, args } => format!(
            "Expr::format(\"{}\", vec![{}])",
            escape(template),
            emit_list(args)
        ),

        Expr::SplitN {
            target,
            delimiter,
            n,
        } => format!("{}.split_n(\"{}\", {})", e(target), escape(delimiter), n),
        Expr::Take { target, n } => format!("{}.take({})", e(target), n),
        Expr::Skip { target, n } => format!("{}.skip({})", e(target), n),
        Expr::Reverse { target } => format!("{}.reverse()", e(target)),
        Expr::SortBy { target, key } => format!("{}.sort_by({})", e(target), e(key)),
        Expr::Unique { target } => format!("{}.unique()", e(target)),
        Expr::UrlEncode { target } => format!("{}.url_encode()", e(target)),
        Expr::UrlDecode { target } => format!("{}.url_decode()", e(target)),
        Expr::FormatPadded {
            target,
            width,
            fill,
            align,
        } => {
            let align_str = match align {
                PadAlign::Left => "PadAlign::Left",
                PadAlign::Right => "PadAlign::Right",
                PadAlign::Center => "PadAlign::Center",
            };
            format!(
                "{}.format_padded({}, '{}', {})",
                e(target),
                width,
                fill.escape_default(),
                align_str
            )
        }
        Expr::ScalarOverride { name } => format!("Expr::scalar(\"{}\")", escape(name)),
        Expr::EncodedField {
            subfields,
            delimiter,
            encoding,
        } => {
            let encoding_str = match encoding {
                IdEncoding::Base64Url => "kani_shared::ast::IdEncoding::Base64Url",
                IdEncoding::Base64 => "kani_shared::ast::IdEncoding::Base64",
                IdEncoding::Passthrough => "kani_shared::ast::IdEncoding::Passthrough",
                IdEncoding::Hex => "kani_shared::ast::IdEncoding::Hex",
            };
            let entries: Vec<String> = subfields
                .iter()
                .map(|(k, v)| format!("(\"{}\".to_string(), {})", escape(k), e(v)))
                .collect();
            format!(
                "Expr::encoded_field(vec![{}], \"{}\", {})",
                entries.join(", "),
                escape(delimiter),
                encoding_str
            )
        }

        Expr::Fetch {
            url_expr,
            blueprint,
            method,
            headers,
            kind,
            on_failure,
            endpoint_id,
        } => {
            use kani_shared::ast::{HttpMethod, SubBlueprintKind};
            let method_str = match method {
                HttpMethod::Get => "",
                HttpMethod::Post => ".with_method(HttpMethod::Post)",
                HttpMethod::Put => ".with_method(HttpMethod::Put)",
                HttpMethod::Delete => ".with_method(HttpMethod::Delete)",
            };
            let kind_fn = match kind {
                SubBlueprintKind::Html => "fetch_html",
                SubBlueprintKind::Json => "fetch_json",
            };
            let headers_chain: String = headers
                .iter()
                .map(|(k, v)| format!(".with_header({}, {})", e(k), e(v)))
                .collect();
            let on_failure_chain = match on_failure {
                OnFailurePolicy::Fail => String::new(),
                OnFailurePolicy::Skip => {
                    ".with_on_failure(kani_shared::ast::OnFailurePolicy::Skip)".into()
                }
                OnFailurePolicy::Use(fallback) => format!(
                    ".with_on_failure(kani_shared::ast::OnFailurePolicy::Use(Box::new({})))",
                    e(fallback)
                ),
            };
            let endpoint_id_chain = endpoint_id
                .as_ref()
                .map(|id| format!(".with_endpoint_id({id:?})"))
                .unwrap_or_default();
            format!(
                "Expr::{}({}, {}){}{}{}{}",
                kind_fn,
                e(url_expr),
                emit_blueprint_from_struct(blueprint),
                method_str,
                headers_chain,
                endpoint_id_chain,
                on_failure_chain
            )
        }
        Expr::UserFn { name, args } => {
            if args.is_empty() {
                format!("Expr::user_fn(\"{}\", vec![])", escape(name))
            } else {
                format!(
                    "Expr::user_fn(\"{}\", vec![{}])",
                    escape(name),
                    emit_list(args)
                )
            }
        }
        Expr::Arena { arena, root } => emit_arena(arena, *root),
    }
}

/// Emit a `BlueprintBuilder::new(...)...build()` expression from a Blueprint struct.
pub(crate) fn emit_blueprint_from_struct(bp: &Blueprint) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "BlueprintBuilder::new(\"{}\")",
        escape(&bp.container)
    ));
    for b in &bp.bindings {
        lines.push(format!(
            "    .bind(\"{}\", {})",
            escape(&b.name),
            e(&b.expr)
        ));
    }
    for f in &bp.fields {
        let method = if f.optional { "field_opt" } else { "field" };
        lines.push(format!(
            "    .{method}(\"{}\", {})",
            escape(&f.name),
            e(&f.expr)
        ));
    }
    for s in &bp.scalars {
        let method = if s.optional { "scalar_opt" } else { "scalar" };
        lines.push(format!(
            "    .{method}(\"{}\", {})",
            escape(&s.name),
            e(&s.expr)
        ));
    }
    if let Some(pag) = &bp.pagination {
        let offset_type_code = match &pag.offset_type {
            kani_shared::ast::OffsetType::ItemOffset => "OffsetType::ItemOffset".to_string(),
            kani_shared::ast::OffsetType::PageNumber { start } => {
                format!("OffsetType::PageNumber {{ start: {} }}", start)
            }
            kani_shared::ast::OffsetType::CursorToken { next_cursor_field } => format!(
                "OffsetType::CursorToken {{ next_cursor_field: \"{}\".into() }}",
                escape(next_cursor_field)
            ),
        };
        lines.push(format!(
            "    .paginated({}, \"{}\", {})",
            pag.native_page_size,
            escape(&pag.offset_param),
            offset_type_code
        ));
    }
    lines.push("    .build()".into());
    lines.join("\n")
}

fn e(expr: &Expr) -> String {
    emit_expr(expr)
}

fn emit_list(items: &[Expr]) -> String {
    items.iter().map(emit_expr).collect::<Vec<_>>().join(", ")
}

fn emit_arena(arena: &ExprArena, root: ExprId) -> String {
    if let Err(error) = arena.validate(root) {
        return format!("compile_error!({error:?})");
    }
    let bytes = postcard::to_allocvec(&(arena, root)).expect("arena serialization");
    let bytes = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("Expr::arena_from_bytes(&[{bytes}])")
}

fn emit_float(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{n:.1}")
    } else {
        format!("{n}")
    }
}

pub fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
