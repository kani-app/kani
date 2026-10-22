use crate::dsl::parse;
use crate::error::{CliError, report_custom_error, report_dsl_errors};
use kani_shared::ast::Expr;
use std::collections::BTreeMap;
use std::path::Path;

pub fn run(expression: &str, scripts_path: Option<&Path>) -> Result<(), CliError> {
    let pure_scripts = load_pure_scripts(scripts_path)?;

    let parse_ast = parse(expression).map_err(|errors| {
        report_dsl_errors("<stdin>", expression, None, &errors);
        CliError::Other("DSL parsing failed (see above)".to_string())
    })?;

    let ast_raw: Result<Expr, Vec<kani_yaml::YamlError>> = parse_ast.try_into();

    if let Err(item) = ast_raw {
        for error in item {
            match error {
                kani_yaml::YamlError::DslConversion { message, span } => {
                    report_custom_error("<stdin>", expression, &message, span);
                }
                kani_yaml::YamlError::DslParse {
                    field_path,
                    expression,
                    errors,
                } => report_dsl_errors("<stdin>", &expression, Some(&field_path), &errors),
                e => eprintln!("error when validating: {e}"),
            }
        }
        return Err(CliError::Other("Validation Error (see above)".to_string()));
    }

    let expr: Expr = ast_raw.expect("checked Err case and returned early above");

    if !pure_scripts.is_empty() {
        check_user_fns(&expr, &pure_scripts);
    }

    println!("{expr:#?}");
    Ok(())
}

fn load_pure_scripts(path: Option<&Path>) -> Result<BTreeMap<String, String>, CliError> {
    let Some(path) = path else {
        return Ok(BTreeMap::new());
    };

    let src = std::fs::read_to_string(path)
        .map_err(|e| CliError::Other(format!("--scripts: cannot read {}: {e}", path.display())))?;

    let ext: crate::yaml::schema::YamlExtension = serde_yaml::from_str(&src).map_err(|e| {
        CliError::Other(format!(
            "--scripts: YAML parse error in {}: {e}",
            path.display()
        ))
    })?;

    let scripts = ext.scripts.pure;

    let engine = crate::yaml::validate::make_validation_sandbox();
    for (name, body) in &scripts {
        if let Err(e) = engine.compile(body) {
            return Err(CliError::Other(format!(
                "--scripts: scripts.pure.{name}: {e}"
            )));
        }
    }

    Ok(scripts)
}

fn check_user_fns(expr: &Expr, scripts: &BTreeMap<String, String>) {
    let mut names = Vec::new();
    collect_user_fn_names(expr, &mut names);
    for name in &names {
        if !scripts.contains_key(name.as_str()) {
            eprintln!("warning: .user.{name}() is not defined in the scripts file");
        }
    }
    if !names.is_empty() {
        let defined: Vec<_> = scripts.keys().collect();
        println!("user functions in expression: {names:?}");
        println!("defined in scripts file:      {defined:?}");
    }
}

fn collect_user_fn_names(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::UserFn { name, args } => {
            out.push(name.clone());
            for arg in args {
                collect_user_fn_names(arg, out);
            }
        }
        Expr::BinaryOperation { lhs, rhs, .. } => {
            collect_user_fn_names(lhs, out);
            collect_user_fn_names(rhs, out);
        }
        Expr::Attr { target, .. }
        | Expr::Text { target }
        | Expr::InnerHtml { target }
        | Expr::Select { target, .. }
        | Expr::First { target, .. }
        | Expr::Split { target, .. }
        | Expr::At { target, .. }
        | Expr::Replace { target, .. }
        | Expr::Trim { target }
        | Expr::Lower { target }
        | Expr::Matches { target, .. }
        | Expr::Capture { target, .. }
        | Expr::ParseFloat { target }
        | Expr::ParseInt { target }
        | Expr::JsonPtr { target, .. }
        | Expr::JsonStr { target }
        | Expr::JsonInt { target }
        | Expr::JsonFloat { target }
        | Expr::JsonBool { target }
        | Expr::ArrayLen { target }
        | Expr::JsonKeys { target }
        | Expr::HasClass { target, .. }
        | Expr::Children { target }
        | Expr::StartsWith { target, .. }
        | Expr::EndsWith { target, .. }
        | Expr::Slice { target, .. }
        | Expr::Lookup { target, .. }
        | Expr::DateParse { target, .. }
        | Expr::DateParseRfc3339 { target }
        | Expr::ToString { target }
        | Expr::Join { target, .. }
        | Expr::JsonFold { target }
        | Expr::Not { target }
        | Expr::StringLen { target }
        | Expr::SplitN { target, .. }
        | Expr::Take { target, .. }
        | Expr::Skip { target, .. }
        | Expr::Reverse { target }
        | Expr::Unique { target }
        | Expr::UrlEncode { target }
        | Expr::UrlDecode { target }
        | Expr::FormatPadded { target, .. } => {
            collect_user_fn_names(target, out);
        }
        Expr::Prepend { target, prefix }
        | Expr::Append {
            target,
            suffix: prefix,
        } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(prefix, out);
        }
        Expr::Fallback { target, default } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(default, out);
        }
        Expr::Map { target, transform } | Expr::FlatMap { target, transform } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(transform, out);
        }
        Expr::Filter { target, filter } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(filter, out);
        }
        Expr::Fold {
            target,
            transform,
            base,
        } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(transform, out);
            collect_user_fn_names(base, out);
        }
        Expr::Let { value, body, .. } => {
            collect_user_fn_names(value, out);
            collect_user_fn_names(body, out);
        }
        Expr::If {
            condition,
            then,
            else_,
        } => {
            collect_user_fn_names(condition, out);
            collect_user_fn_names(then, out);
            collect_user_fn_names(else_, out);
        }
        Expr::ResolveUrl { target, base } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(base, out);
        }
        Expr::JsonGet { target, key } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(key, out);
        }
        Expr::JsonFind { target, key, value } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(key, out);
            collect_user_fn_names(value, out);
        }
        Expr::SortBy { target, key } => {
            collect_user_fn_names(target, out);
            collect_user_fn_names(key, out);
        }
        Expr::Concat(items) | Expr::List(items) | Expr::JsonArray(items) | Expr::Merge(items) => {
            for item in items {
                collect_user_fn_names(item, out);
            }
        }
        Expr::Format { args, .. } => {
            for arg in args {
                collect_user_fn_names(arg, out);
            }
        }
        Expr::EncodedField { subfields, .. } => {
            for (_, expr) in subfields {
                collect_user_fn_names(expr, out);
            }
        }
        Expr::Fetch {
            url_expr, headers, ..
        } => {
            collect_user_fn_names(url_expr, out);
            for (k, v) in headers {
                collect_user_fn_names(k, out);
                collect_user_fn_names(v, out);
            }
        }
        Expr::Arena { arena, .. } => {
            for node in &arena.nodes {
                if let kani_shared::ast::ExprNode::UserFn { name, .. } = node {
                    out.push(name.clone());
                }
            }
        }
        Expr::SelfRef
        | Expr::Dom(_)
        | Expr::Json(_)
        | Expr::Var(_)
        | Expr::Literal(_)
        | Expr::Number(_)
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Index
        | Expr::Pref(_)
        | Expr::ScalarOverride { .. } => {}
    }
}
