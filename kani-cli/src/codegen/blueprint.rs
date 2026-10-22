//! Emit Rust source for a BlueprintBuilder chain from a ValidatedEndpoint.

use super::expr::emit_expr;
use crate::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedHnp, ValidatedTotalPages,
};
use kani_yaml::{build_blueprint_core, make_fetch_expr};

pub(crate) fn emit_blueprint_chain(
    ep: &ValidatedEndpoint,
    ext: &ValidatedExtension,
    parent_endpoint_name: &str,
) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "let bp = BlueprintBuilder::new(\"{}\")",
        ep.container
    ));

    lines.push("    .request(req)".into());

    for b in &ep.bindings {
        lines.push(format!("    .bind(\"{}\", {})", b.name, emit_expr(&b.expr)));
    }

    for step in &ep.then_steps {
        if let Some(sub_ep) = ext.endpoint_by_name(&step.endpoint_name) {
            let endpoint_id = Some(format!("{parent_endpoint_name}/{}", step.merge_as));
            let fetch = make_fetch_expr(&step.url_expr, sub_ep, &step.on_failure, endpoint_id);
            lines.push(format!(
                "    .bind({:?}, {})",
                kani_yaml::then_binding_name(&step.merge_as),
                emit_expr(&fetch)
            ));
        }
    }

    for f in &ep.fields {
        match &f.source {
            FieldSource::Blueprint(expr) => {
                let method = if f.optional { "field_opt" } else { "field" };
                lines.push(format!(
                    "    .{method}(\"{}\", {})",
                    f.name,
                    emit_expr(expr)
                ));
            }
            FieldSource::FnArg(_) => {}
        }
    }

    for step in &ep.for_each_steps {
        if let Some(sub_ep) = ext.endpoint_by_name(&step.endpoint_name) {
            let endpoint_id = Some(format!("{parent_endpoint_name}/{}", step.merge_as));
            let fetch = make_fetch_expr(&step.url_expr, sub_ep, &step.on_failure, endpoint_id);
            lines.push(format!(
                "    .field(\"{}\", {})",
                step.merge_as,
                emit_expr(&fetch)
            ));
        }
    }

    for s in &ep.scalars {
        match &s.source {
            FieldSource::Blueprint(expr) => {
                let method = if s.optional { "scalar_opt" } else { "scalar" };
                lines.push(format!(
                    "    .{method}(\"{}\", {})",
                    s.name,
                    emit_expr(expr)
                ));
            }
            FieldSource::FnArg(_) => {}
        }
    }

    if let ValidatedHnp::Scalar(expr) = &ep.has_next_page {
        lines.push(format!(
            "    .scalar(\"has_next_page\", {})",
            emit_expr(expr)
        ));
    }

    if let ValidatedTotalPages::Scalar(expr) = &ep.total_pages {
        lines.push(format!(
            "    .scalar_opt(\"total_pages\", {})",
            emit_expr(expr)
        ));
    }

    if let Some(pag) = &ep.pagination {
        let (offset, cursor_scalar) = kani_yaml::pagination_lowering(pag);
        if let Some(expr) = cursor_scalar {
            lines.push(format!(
                "    .scalar_opt({:?}, {})",
                kani_yaml::CURSOR_SCALAR,
                emit_expr(&expr)
            ));
        }
        let offset_type = match offset {
            kani_shared::ast::OffsetType::ItemOffset => "OffsetType::ItemOffset".into(),
            kani_shared::ast::OffsetType::PageNumber { start } => {
                format!("OffsetType::PageNumber {{ start: {start} }}")
            }
            kani_shared::ast::OffsetType::CursorToken { next_cursor_field } => {
                format!(
                    "OffsetType::CursorToken {{ next_cursor_field: {next_cursor_field:?}.into() }}"
                )
            }
        };
        lines.push(format!(
            "    .paginated({}, {:?}, {})",
            pag.native_page_size, pag.offset_param, offset_type
        ));
    }

    lines.push("    .build();".into());
    lines.join("\n")
}

/// Emit only the `BlueprintBuilder::new(...)...build()` without the `.request()` line.
pub(crate) fn emit_blueprint_chain_no_request(
    ep: &ValidatedEndpoint,
    ext: &ValidatedExtension,
    parent_endpoint_name: &str,
) -> String {
    let src = emit_blueprint_chain(ep, ext, parent_endpoint_name);
    src.lines()
        .filter(|l| !l.trim().starts_with(".request(req)"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a Blueprint from a ValidatedEndpoint at codegen time (no request attached),
/// serialize it to postcard bytes, and return a Rust `const` declaration.
///
/// Delegates the actual field/scalar/bindings/pagination assembly to
/// `kani_yaml::build_blueprint_core`, the same builder the interpreted tier uses to
/// construct a live `Blueprint` — see `kani-yaml/src/lib.rs` for the shared logic.
pub fn emit_blueprint_bytes(
    ep: &ValidatedEndpoint,
    ext: &ValidatedExtension,
    parent_endpoint_name: &str,
) -> String {
    let bytes = build_blueprint_core(ep, ext, parent_endpoint_name)
        .build()
        .to_bytes();
    let hex: Vec<String> = bytes.iter().map(|b| format!("0x{b:02x}")).collect();
    format!("const BP: &[u8] = &[{}];", hex.join(", "))
}
