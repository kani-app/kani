//! Sandboxed Rhai hooks, pure functions, and browser-script registries for extensions.

pub mod bindings;
pub mod browser_scripts;
pub mod bytes;
pub mod engine;
pub mod hook_registry;
pub mod lint;
pub mod pure_bridge;

pub use bindings::{
    HookAction, HookActionKind, ScriptableCtx, ScriptableRequest, ScriptableResponse,
    make_hook_sandbox,
};
pub use browser_scripts::BrowserScriptRegistry;
pub use bytes::Bytes;
pub use engine::make_pure_sandbox;
pub use hook_registry::{HookRegistry, HookScripts};
pub use pure_bridge::PureFunctionRegistry;

/// Whether a script calls the function `name` anywhere, as a call or a method call. Found by
/// walking the compiled AST, so the name inside a string or a comment does not count.
pub fn script_calls(script: &str, name: &str) -> Result<bool, String> {
    use rhai::{ASTNode, Expr, Stmt};
    let ast = rhai::Engine::new_raw()
        .compile(script)
        .map_err(|e| e.to_string())?;
    let mut found = false;
    ast.walk(&mut |path: &[ASTNode]| {
        if let Some(
            ASTNode::Expr(Expr::FnCall(call, _) | Expr::MethodCall(call, _))
            | ASTNode::Stmt(Stmt::FnCall(call, _)),
        ) = path.last()
            && call.name == name
        {
            found = true;
        }
        !found
    });
    Ok(found)
}

#[cfg(test)]
mod script_calls_tests {
    #![allow(clippy::unwrap_used)]
    use super::script_calls;

    #[test]
    fn a_call_is_found_wherever_it_appears_and_nowhere_else() {
        for calling in [
            "refresh_auth(\"login\")",
            "if resp.status == 401 { refresh_auth(\"login\") } else { proceed() }",
            "let x = 1;\nfn f() { refresh_auth(\"a\") }\nf()",
        ] {
            assert!(script_calls(calling, "refresh_auth").unwrap(), "{calling}");
        }
        for not_calling in [
            "proceed()",
            "let s = \"refresh_auth(\\\"x\\\")\"; proceed()",
            "// refresh_auth(\"x\")\nproceed()",
        ] {
            assert!(
                !script_calls(not_calling, "refresh_auth").unwrap(),
                "{not_calling}"
            );
        }
    }
}
