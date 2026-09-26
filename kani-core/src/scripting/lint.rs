//! Finds calls in a compiled script that nothing defines. Rhai resolves functions only when a
//! call runs, so a misspelt host function compiles and then fails every request that reaches it.

use std::collections::BTreeSet;

use rhai::{AST, ASTNode, Engine, Expr, Stmt};

/// Language keywords that parse as calls but are never registered functions.
const KEYWORD_FUNCTIONS: &[&str] = &[
    "print",
    "debug",
    "type_of",
    "eval",
    "Fn",
    "call",
    "curry",
    "is_shared",
    "is_def_var",
    "is_def_fn",
];

/// Every function name `engine` can resolve without a namespace, standard packages included.
pub fn registered_function_names(engine: &Engine) -> BTreeSet<String> {
    engine
        .gen_fn_signatures(true)
        .iter()
        .filter_map(|signature| signature.split('(').next())
        .filter(|name| !name.contains("::"))
        .map(str::to_string)
        .collect()
}

/// Names called in `ast` that are neither registered with the engine, defined by the script,
/// nor bound to a variable or parameter (a function pointer can be called by its binding's
/// name). Matching is by name only, so a call with the wrong number of arguments passes.
pub fn unresolved_calls(registered: &BTreeSet<String>, ast: &AST) -> BTreeSet<String> {
    let mut called = BTreeSet::new();
    let mut bound: BTreeSet<String> = BTreeSet::new();
    for function in ast.iter_functions() {
        bound.insert(function.name.to_string());
        bound.extend(function.params.iter().map(|p| p.to_string()));
    }
    ast.walk(&mut |path: &[ASTNode]| {
        match path.last() {
            Some(
                ASTNode::Expr(Expr::FnCall(call, _) | Expr::MethodCall(call, _))
                | ASTNode::Stmt(Stmt::FnCall(call, _)),
            ) if call.op_token.is_none() && call.namespace.is_empty() => {
                called.insert(call.name.to_string());
            }
            Some(ASTNode::Stmt(Stmt::Var(var, ..))) => {
                bound.insert(var.0.name.to_string());
            }
            _ => {}
        }
        true
    });
    called
        .into_iter()
        .filter(|name| {
            !registered.contains(name)
                && !bound.contains(name)
                && !KEYWORD_FUNCTIONS.contains(&name.as_str())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn unresolved(src: &str) -> Vec<String> {
        let engine = crate::scripting::make_hook_sandbox();
        let ast = engine.compile(src).unwrap();
        unresolved_calls(&registered_function_names(&engine), &ast)
            .into_iter()
            .collect()
    }

    #[test]
    fn a_misspelt_host_function_is_reported() {
        assert_eq!(unresolved(r#"cache_gett("ns", "k")"#), vec!["cache_gett"]);
    }

    #[test]
    fn host_standard_and_script_functions_resolve() {
        let src = r#"
            fn helper(x) { x.len() + parse_int("1") }
            let v = cache_get("ns", "k");
            let f = |s| s.to_upper();
            f.call("a");
            print(type_of(v));
            helper("abc") + 1
        "#;
        assert_eq!(unresolved(src), Vec::<String>::new());
    }

    #[test]
    fn a_misspelt_method_inside_a_closure_is_reported() {
        assert_eq!(
            unresolved(r#"let f = |s| s.to_uper(); f.call("a")"#),
            vec!["to_uper"]
        );
    }
}
