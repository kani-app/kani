use crate::dsl::parse;
use crate::error::{CliError, report_custom_error, report_dsl_errors};
use kani_shared::ast::Expr;
use std::fmt;

#[derive(Debug, Clone)]
/// One depth-annotated expression in a DSL explanation.
pub struct TraceStep {
    pub depth: usize,
    pub expr_kind: String,
    pub description: String,
}

/// Pre-order explanation of a parsed DSL expression tree.
pub struct ExplainTrace {
    pub steps: Vec<TraceStep>,
}

impl ExplainTrace {
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

impl fmt::Display for ExplainTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for step in &self.steps {
            let indent = "  ".repeat(step.depth);
            writeln!(f, "{indent}[{}] {}", step.expr_kind, step.description)?;
        }
        Ok(())
    }
}

pub fn run(expression: &str) -> Result<(), CliError> {
    let trace = explain(expression)?;
    print!("{trace}");
    println!("\n{} step(s)", trace.len());
    Ok(())
}

pub fn explain(expression: &str) -> Result<ExplainTrace, CliError> {
    let parse_ast = parse(expression).map_err(|errors| {
        report_dsl_errors("<expression>", expression, None, &errors);
        CliError::Other("DSL parsing failed (see above)".to_string())
    })?;

    let expr: Expr = parse_ast
        .try_into()
        .map_err(|errs: Vec<kani_yaml::YamlError>| {
            for e in &errs {
                if let kani_yaml::YamlError::DslConversion { message, span } = e {
                    report_custom_error("<expression>", expression, message, span.clone());
                }
            }
            CliError::Other("DSL conversion failed".to_string())
        })?;

    let mut steps = Vec::new();
    collect_steps(&expr, 0, &mut steps);
    Ok(ExplainTrace { steps })
}

fn collect_steps(expr: &Expr, depth: usize, steps: &mut Vec<TraceStep>) {
    let (kind, description, children) = describe_expr(expr);
    steps.push(TraceStep {
        depth,
        expr_kind: kind,
        description,
    });
    for child in children {
        collect_steps(child, depth + 1, steps);
    }
}

fn describe_expr(expr: &Expr) -> (String, String, Vec<&Expr>) {
    match expr {
        Expr::SelfRef => ("Self".into(), "current element".into(), vec![]),
        Expr::Index => ("Index".into(), "loop index (0-based)".into(), vec![]),
        Expr::Literal(s) => ("Literal".into(), format!("{s:?}"), vec![]),
        Expr::Number(n) => ("Number".into(), n.to_string(), vec![]),
        Expr::Bool(b) => ("Bool".into(), b.to_string(), vec![]),
        Expr::Null => ("Null".into(), "null value".into(), vec![]),
        Expr::Var(name) => ("Var".into(), format!("${name}"), vec![]),
        Expr::Pref(key) => ("Pref".into(), format!("preference {key:?}"), vec![]),

        Expr::Dom(selector) => (
            "Dom".into(),
            format!("document root → {selector:?}"),
            vec![],
        ),
        Expr::Json(pointer) => ("Json".into(), format!("JSON pointer {pointer:?}"), vec![]),

        Expr::BinaryOperation { op, lhs, rhs } => (
            "BinaryOp".into(),
            format!("{op:?}"),
            vec![lhs.as_ref(), rhs.as_ref()],
        ),

        Expr::Attr { target, name } => (
            "Attr".into(),
            format!("get attribute {name:?}"),
            vec![target.as_ref()],
        ),
        Expr::Text { target } => (
            "Text".into(),
            "extract text content".into(),
            vec![target.as_ref()],
        ),
        Expr::InnerHtml { target } => (
            "InnerHtml".into(),
            "extract inner HTML".into(),
            vec![target.as_ref()],
        ),
        Expr::Select { target, selector } => (
            "Select".into(),
            format!("select all matching {selector:?}"),
            vec![target.as_ref()],
        ),
        Expr::First { target, selector } => (
            "First".into(),
            if selector.is_empty() {
                "first child element".into()
            } else {
                format!("first child matching {selector:?}")
            },
            vec![target.as_ref()],
        ),
        Expr::HasClass { target, class } => (
            "HasClass".into(),
            format!("has CSS class {class:?}"),
            vec![target.as_ref()],
        ),
        Expr::Children { target } => (
            "Children".into(),
            "direct child elements".into(),
            vec![target.as_ref()],
        ),

        Expr::Split { target, delimiter } => (
            "Split".into(),
            format!("split on {delimiter:?}"),
            vec![target.as_ref()],
        ),
        Expr::At { target, index } => (
            "At".into(),
            format!("item at index {index}"),
            vec![target.as_ref()],
        ),
        Expr::Replace { target, from, to } => (
            "Replace".into(),
            format!("replace {from:?} with {to:?}"),
            vec![target.as_ref()],
        ),
        Expr::Trim { target } => (
            "Trim".into(),
            "trim leading/trailing whitespace".into(),
            vec![target.as_ref()],
        ),
        Expr::Prepend { target, prefix } => (
            "Prepend".into(),
            "prepend string".into(),
            vec![target.as_ref(), prefix.as_ref()],
        ),
        Expr::Append { target, suffix } => (
            "Append".into(),
            "append string".into(),
            vec![target.as_ref(), suffix.as_ref()],
        ),
        Expr::Lower { target } => (
            "Lower".into(),
            "convert to lowercase".into(),
            vec![target.as_ref()],
        ),
        Expr::Matches { target, pattern } => (
            "Matches".into(),
            format!("regex match {pattern:?}"),
            vec![target.as_ref()],
        ),
        Expr::Capture { target, pattern } => (
            "Capture".into(),
            format!("regex capture groups from {pattern:?}"),
            vec![target.as_ref()],
        ),
        Expr::StartsWith { target, prefix } => (
            "StartsWith".into(),
            format!("starts with {prefix:?}"),
            vec![target.as_ref()],
        ),
        Expr::EndsWith { target, suffix } => (
            "EndsWith".into(),
            format!("ends with {suffix:?}"),
            vec![target.as_ref()],
        ),
        Expr::Slice { target, start, end } => (
            "Slice".into(),
            format!("slice [{start}..{end:?}]"),
            vec![target.as_ref()],
        ),
        Expr::StringLen { target } => (
            "StringLen".into(),
            "character count".into(),
            vec![target.as_ref()],
        ),
        Expr::ToString { target } => (
            "ToString".into(),
            "convert to string".into(),
            vec![target.as_ref()],
        ),
        Expr::Not { target } => (
            "Not".into(),
            "boolean negation".into(),
            vec![target.as_ref()],
        ),

        Expr::ParseFloat { target } => (
            "ParseFloat".into(),
            "parse as f64".into(),
            vec![target.as_ref()],
        ),
        Expr::ParseInt { target } => (
            "ParseInt".into(),
            "parse as i64".into(),
            vec![target.as_ref()],
        ),

        Expr::JsonPtr { target, pointer } => (
            "JsonPtr".into(),
            format!("JSON pointer {pointer:?}"),
            vec![target.as_ref()],
        ),
        Expr::JsonStr { target } => (
            "JsonStr".into(),
            "extract as string".into(),
            vec![target.as_ref()],
        ),
        Expr::JsonInt { target } => (
            "JsonInt".into(),
            "extract as integer".into(),
            vec![target.as_ref()],
        ),
        Expr::JsonFloat { target } => (
            "JsonFloat".into(),
            "extract as float".into(),
            vec![target.as_ref()],
        ),
        Expr::JsonBool { target } => (
            "JsonBool".into(),
            "extract as boolean".into(),
            vec![target.as_ref()],
        ),
        Expr::ArrayLen { target } => (
            "ArrayLen".into(),
            "JSON array length".into(),
            vec![target.as_ref()],
        ),
        Expr::JsonKeys { target } => (
            "JsonKeys".into(),
            "JSON object keys".into(),
            vec![target.as_ref()],
        ),
        Expr::JsonGet { target, key } => (
            "JsonGet".into(),
            "dynamic JSON field access".into(),
            vec![target.as_ref(), key.as_ref()],
        ),
        Expr::JsonFind { target, key, value } => (
            "JsonFind".into(),
            "find JSON item where key=value".into(),
            vec![target.as_ref(), key.as_ref(), value.as_ref()],
        ),
        Expr::JsonFold { target } => (
            "JsonFold".into(),
            "reduce JSON array by merge".into(),
            vec![target.as_ref()],
        ),

        Expr::Join { target, delimiter } => (
            "Join".into(),
            format!("join list with {delimiter:?}"),
            vec![target.as_ref()],
        ),
        Expr::ResolveUrl { target, base } => (
            "ResolveUrl".into(),
            "resolve relative URL against base".into(),
            vec![target.as_ref(), base.as_ref()],
        ),

        Expr::Fallback { target, default } => (
            "Fallback".into(),
            "use default if null".into(),
            vec![target.as_ref(), default.as_ref()],
        ),
        Expr::Lookup { target, table } => (
            "Lookup".into(),
            format!("lookup table ({} entries)", table.len()),
            vec![target.as_ref()],
        ),

        Expr::Map { target, transform } => (
            "Map".into(),
            "transform each item".into(),
            vec![target.as_ref(), transform.as_ref()],
        ),
        Expr::FlatMap { target, transform } => (
            "FlatMap".into(),
            "flat-map each item".into(),
            vec![target.as_ref(), transform.as_ref()],
        ),
        Expr::Filter { target, filter } => (
            "Filter".into(),
            "filter items by predicate".into(),
            vec![target.as_ref(), filter.as_ref()],
        ),
        Expr::Fold {
            target,
            transform,
            base,
        } => (
            "Fold".into(),
            "left-fold over list".into(),
            vec![target.as_ref(), transform.as_ref(), base.as_ref()],
        ),

        Expr::Concat(parts) => (
            "Concat".into(),
            format!("concatenate {} parts", parts.len()),
            parts.iter().collect(),
        ),
        Expr::Merge(parts) => (
            "Merge".into(),
            format!("merge {} lists", parts.len()),
            parts.iter().collect(),
        ),
        Expr::List(items) => (
            "List".into(),
            format!("list of {} expressions", items.len()),
            items.iter().collect(),
        ),
        Expr::JsonArray(items) => (
            "JsonArray".into(),
            format!("JSON array of {} items", items.len()),
            items.iter().collect(),
        ),

        Expr::DateParse { target, format } => (
            "DateParse".into(),
            format!("parse date with format {format:?}"),
            vec![target.as_ref()],
        ),
        Expr::DateParseRfc3339 { target } => (
            "DateParseRfc3339".into(),
            "parse RFC3339 date".into(),
            vec![target.as_ref()],
        ),

        Expr::If {
            condition,
            then,
            else_,
        } => (
            "If".into(),
            "conditional branch".into(),
            vec![condition.as_ref(), then.as_ref(), else_.as_ref()],
        ),
        Expr::Let { name, value, body } => (
            "Let".into(),
            format!("bind ${name}"),
            vec![value.as_ref(), body.as_ref()],
        ),

        Expr::Format { template, args } => (
            "Format".into(),
            format!("format template {template:?}"),
            args.iter().collect(),
        ),

        Expr::Fetch {
            url_expr,
            method,
            kind,
            ..
        } => (
            "Fetch".into(),
            format!("{method:?} sub-fetch ({kind:?})"),
            vec![url_expr.as_ref()],
        ),

        Expr::SplitN {
            target,
            delimiter,
            n,
        } => (
            "SplitN".into(),
            format!("split on {delimiter:?} into at most {n} parts"),
            vec![target.as_ref()],
        ),
        Expr::Take { target, n } => (
            "Take".into(),
            format!("first {n} items"),
            vec![target.as_ref()],
        ),
        Expr::Skip { target, n } => (
            "Skip".into(),
            format!("drop first {n} items"),
            vec![target.as_ref()],
        ),
        Expr::Reverse { target } => (
            "Reverse".into(),
            "reverse list".into(),
            vec![target.as_ref()],
        ),
        Expr::SortBy { target, key } => (
            "SortBy".into(),
            "sort list by key expression".into(),
            vec![target.as_ref(), key.as_ref()],
        ),
        Expr::Unique { target } => (
            "Unique".into(),
            "remove duplicate elements (first occurrence kept)".into(),
            vec![target.as_ref()],
        ),
        Expr::UrlEncode { target } => (
            "UrlEncode".into(),
            "percent-encode for URL query".into(),
            vec![target.as_ref()],
        ),
        Expr::UrlDecode { target } => (
            "UrlDecode".into(),
            "decode percent-encoded string".into(),
            vec![target.as_ref()],
        ),
        Expr::FormatPadded {
            target,
            width,
            fill,
            align,
        } => (
            "FormatPadded".into(),
            format!("pad to width {width} fill={fill:?} align={align:?}"),
            vec![target.as_ref()],
        ),
        Expr::ScalarOverride { name } => {
            ("ScalarOverride".into(), format!("scalar {name:?}"), vec![])
        }

        Expr::EncodedField {
            subfields,
            delimiter,
            encoding,
        } => (
            "EncodedField".into(),
            format!(
                "composite id ({} fields, delim={delimiter:?}, enc={encoding:?})",
                subfields.len()
            ),
            subfields.iter().map(|(_, e)| e.as_ref()).collect(),
        ),
        Expr::UserFn { name, args } => (
            "UserFn".into(),
            format!(".user.{name}() — {} arg(s)", args.len()),
            args.iter().collect(),
        ),
        Expr::Arena { arena, root } => (
            "Arena".into(),
            format!("{} flat nodes, root {}", arena.nodes.len(), root.0),
            vec![],
        ),
    }
}
