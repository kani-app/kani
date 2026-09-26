use std::collections::HashMap;

use rhai::{AST, Dynamic, Scope};

use super::bindings::{
    HookAction, HookActionKind, ScriptableCtx, ScriptableRequest, ScriptableResponse,
    make_hook_sandbox,
};

#[derive(Default)]
pub struct HookScripts {
    /// Function definitions prepended to every hook body, so `pre_request` and
    /// `on_status` can share logic instead of repeating it.
    pub shared: std::collections::BTreeMap<String, String>,
    pub pre_request: Option<String>,
    pub on_status: std::collections::BTreeMap<String, String>,
    pub endpoint_pre_request: std::collections::BTreeMap<String, String>,
    pub endpoint_on_status:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// Cache namespaces the hooks may use; any other namespace is refused.
    pub cache: std::collections::BTreeMap<String, kani_shared::CacheNamespaceLimits>,
}

impl HookScripts {
    pub fn is_empty(&self) -> bool {
        self.pre_request.is_none()
            && self.on_status.is_empty()
            && self.endpoint_pre_request.is_empty()
            && self.endpoint_on_status.is_empty()
    }
}

#[derive(Debug)]
pub struct HookRegistry {
    engine: rhai::Engine,
    global_pre_request: Option<AST>,
    global_on_status: HashMap<String, AST>,
    endpoint_pre_request: HashMap<String, AST>,
    endpoint_on_status: HashMap<String, HashMap<String, AST>>,
    cache_namespaces:
        std::sync::Arc<std::collections::BTreeMap<String, kani_shared::CacheNamespaceLimits>>,
}

impl HookRegistry {
    pub fn is_empty(&self) -> bool {
        self.global_pre_request.is_none()
            && self.global_on_status.is_empty()
            && self.endpoint_pre_request.is_empty()
            && self.endpoint_on_status.is_empty()
    }

    fn prelude(scripts: &HookScripts) -> String {
        scripts
            .shared
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn compile(scripts: &HookScripts) -> Result<Self, String> {
        let engine = make_hook_sandbox();
        let prelude = Self::prelude(scripts);
        let with_prelude = |src: &str| {
            if prelude.is_empty() {
                src.to_string()
            } else {
                format!("{prelude}\n{src}")
            }
        };

        let global_pre_request = scripts
            .pre_request
            .as_deref()
            .map(|src| {
                engine
                    .compile(with_prelude(src))
                    .map_err(|e| format!("pre_request compile error: {e}"))
            })
            .transpose()?;

        let mut global_on_status = HashMap::new();
        for (key, src) in &scripts.on_status {
            let ast = engine
                .compile(with_prelude(src))
                .map_err(|e| format!("on_status[{key}] compile error: {e}"))?;
            global_on_status.insert(key.clone(), ast);
        }

        let mut endpoint_pre_request = HashMap::new();
        for (endpoint_id, src) in &scripts.endpoint_pre_request {
            let ast = engine
                .compile(with_prelude(src))
                .map_err(|e| format!("endpoint '{endpoint_id}' pre_request compile error: {e}"))?;
            endpoint_pre_request.insert(endpoint_id.clone(), ast);
        }

        let mut endpoint_on_status: HashMap<String, HashMap<String, AST>> = HashMap::new();
        for (endpoint_id, status_map) in &scripts.endpoint_on_status {
            let mut map = HashMap::new();
            for (key, src) in status_map {
                let ast = engine.compile(with_prelude(src)).map_err(|e| {
                    format!("endpoint '{endpoint_id}' on_status[{key}] compile error: {e}")
                })?;
                map.insert(key.clone(), ast);
            }
            endpoint_on_status.insert(endpoint_id.clone(), map);
        }

        Ok(Self {
            engine,
            cache_namespaces: std::sync::Arc::new(scripts.cache.clone()),
            global_pre_request,
            global_on_status,
            endpoint_pre_request,
            endpoint_on_status,
        })
    }

    pub fn run_pre_request(
        &self,
        req: &mut ScriptableRequest,
        ctx: ScriptableCtx,
    ) -> Result<HookAction, String> {
        let ctx = ScriptableCtx {
            cache_namespaces: std::sync::Arc::clone(&self.cache_namespaces),
            ..ctx
        };
        let endpoint_id = req.endpoint_id.as_deref().unwrap_or("");
        let ast = self
            .endpoint_pre_request
            .get(endpoint_id)
            .or_else(|| parent_endpoint(endpoint_id).and_then(|p| self.endpoint_pre_request.get(p)))
            .or(self.global_pre_request.as_ref());

        let Some(ast) = ast else {
            return Ok(HookAction {
                kind: HookActionKind::Proceed,
            });
        };

        let mut scope = Scope::new();
        scope.push_dynamic("req", Dynamic::from(req.clone()));
        scope.push_dynamic("ctx", Dynamic::from(ctx));

        let result = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, ast)
            .map_err(|e| format!("pre_request hook error: {e}"))?;

        if let Some(mutated) = scope.get_value::<ScriptableRequest>("req") {
            *req = mutated;
        }

        Ok(result.try_cast::<HookAction>().unwrap_or(HookAction {
            kind: HookActionKind::Proceed,
        }))
    }

    pub(crate) fn run_on_status(
        &self,
        req: &ScriptableRequest,
        resp: &mut ScriptableResponse,
        ctx: ScriptableCtx,
    ) -> Result<HookAction, String> {
        let ctx = ScriptableCtx {
            cache_namespaces: std::sync::Arc::clone(&self.cache_namespaces),
            ..ctx
        };
        let endpoint_id = req.endpoint_id.as_deref().unwrap_or("");
        let status = resp.status as u16;

        let ast = self
            .find_on_status_ast(endpoint_id, status)
            .or_else(|| {
                parent_endpoint(endpoint_id).and_then(|p| self.find_on_status_ast(p, status))
            })
            .or_else(|| self.find_on_status_ast("", status));

        let Some(ast) = ast else {
            return Ok(HookAction {
                kind: HookActionKind::Proceed,
            });
        };

        let mut scope = Scope::new();
        scope.push_dynamic("req", Dynamic::from(req.clone()));
        scope.push_dynamic("resp", Dynamic::from(resp.clone()));
        scope.push_dynamic("ctx", Dynamic::from(ctx));

        let result = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, ast)
            .map_err(|e| format!("on_status hook error: {e}"))?;

        if let Some(mutated) = scope.get_value::<ScriptableResponse>("resp") {
            *resp = mutated;
        }

        Ok(result.try_cast::<HookAction>().unwrap_or(HookAction {
            kind: HookActionKind::Proceed,
        }))
    }

    fn find_on_status_ast(&self, endpoint_id: &str, status: u16) -> Option<&AST> {
        let map = if endpoint_id.is_empty() {
            &self.global_on_status
        } else {
            self.endpoint_on_status.get(endpoint_id)?
        };
        let exact = status.to_string();
        let class = format!("{}xx", status / 100);
        map.get(&exact)
            .or_else(|| map.get(&class))
            .or_else(|| map.get("default"))
    }
}

fn parent_endpoint(endpoint_id: &str) -> Option<&str> {
    endpoint_id
        .split_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::sync::Arc;

    fn dummy_ctx() -> ScriptableCtx {
        ScriptableCtx {
            cache_backend: Arc::new(crate::cache::InMemoryCache::new()),
            cache_namespace: "test".to_string(),
            prefs: HashMap::new(),
            v8_process: None,
            http: None,
            browser_scripts: None,
            browser_profile_key: None,
            allowed_host: crate::wasm::AllowedHost::MetadataOnly,
            cache_namespaces: std::sync::Arc::default(),
        }
    }

    fn dummy_req(endpoint_id: Option<&str>) -> ScriptableRequest {
        ScriptableRequest {
            method: "GET".to_string(),
            url: "https://example.com/".to_string(),
            headers: Vec::new(),
            queries: Vec::new(),
            body: None,
            endpoint_id: endpoint_id.map(str::to_string),
        }
    }

    fn dummy_resp(status: i64) -> ScriptableResponse {
        ScriptableResponse {
            status,
            headers: Vec::new(),
            body: String::new(),
        }
    }

    #[test]
    fn compile_valid_pre_request() {
        let scripts = HookScripts {
            pre_request: Some(r#"req.set_header("X-Test", "value"); proceed()"#.to_string()),
            ..Default::default()
        };
        assert!(HookRegistry::compile(&scripts).is_ok());
    }

    #[test]
    fn compile_syntax_error_rejected() {
        let scripts = HookScripts {
            pre_request: Some("req.set_header( // missing paren".to_string()),
            ..Default::default()
        };
        let err = HookRegistry::compile(&scripts).unwrap_err();
        assert!(
            err.contains("pre_request"),
            "error must mention pre_request: {err}"
        );
    }

    #[test]
    fn hook_action_constructors_in_rhai() {
        use super::super::bindings::make_hook_sandbox;
        let engine = make_hook_sandbox();
        let action: HookAction = engine.eval("proceed()").unwrap();
        assert!(matches!(action.kind, HookActionKind::Proceed));
        let action: HookAction = engine.eval("retry()").unwrap();
        assert!(matches!(action.kind, HookActionKind::Retry));
        let action: HookAction = engine.eval("retry_after(30)").unwrap();
        assert!(matches!(
            action.kind,
            HookActionKind::RetryAfter { seconds: 30 }
        ));
        let action: HookAction = engine
            .eval(r#"fail("rate_limited", "too many requests")"#)
            .unwrap();
        assert!(matches!(action.kind, HookActionKind::Fail { .. }));
    }

    #[tokio::test]
    async fn pre_request_mutates_request() {
        let scripts = HookScripts {
            pre_request: Some(r#"req.set_header("X-Signed", "yes"); proceed()"#.to_string()),
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let mut req = dummy_req(None);
        let (action, req) = tokio::task::spawn_blocking(move || {
            let action = registry.run_pre_request(&mut req, dummy_ctx());
            (action, req)
        })
        .await
        .unwrap();
        let action = action.unwrap();
        assert!(matches!(action.kind, HookActionKind::Proceed));
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "X-Signed" && v == "yes"),
            "mutation must propagate back: {:?}",
            req.headers
        );
    }

    #[test]
    fn on_status_exact_match() {
        let mut on_status = std::collections::BTreeMap::new();
        on_status.insert("401".to_string(), "retry()".to_string());
        let scripts = HookScripts {
            on_status,
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let req = dummy_req(None);
        let mut resp = dummy_resp(401);
        let action = registry
            .run_on_status(&req, &mut resp, dummy_ctx())
            .unwrap();
        assert!(matches!(action.kind, HookActionKind::Retry));
    }

    #[test]
    fn on_status_class_match() {
        let mut on_status = std::collections::BTreeMap::new();
        on_status.insert("5xx".to_string(), "retry_after(10)".to_string());
        let scripts = HookScripts {
            on_status,
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let req = dummy_req(None);
        let mut resp = dummy_resp(503);
        let action = registry
            .run_on_status(&req, &mut resp, dummy_ctx())
            .unwrap();
        assert!(matches!(
            action.kind,
            HookActionKind::RetryAfter { seconds: 10 }
        ));
    }

    #[test]
    fn on_status_default_fallback() {
        let mut on_status = std::collections::BTreeMap::new();
        on_status.insert(
            "default".to_string(),
            r#"fail("unexpected", "status")"#.to_string(),
        );
        let scripts = HookScripts {
            on_status,
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let req = dummy_req(None);
        let mut resp = dummy_resp(429);
        let action = registry
            .run_on_status(&req, &mut resp, dummy_ctx())
            .unwrap();
        assert!(matches!(action.kind, HookActionKind::Fail { .. }));
    }

    #[test]
    fn no_matching_hook_proceeds() {
        let registry = HookRegistry::compile(&HookScripts::default()).unwrap();
        let req = dummy_req(None);
        let mut resp = dummy_resp(401);
        let action = registry
            .run_on_status(&req, &mut resp, dummy_ctx())
            .unwrap();
        assert!(matches!(action.kind, HookActionKind::Proceed));
    }

    /// A signing hook must be able to read the query parameters it has to cover,
    /// and append the signature it computes from them.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_hook_can_read_and_append_query_parameters() {
        let scripts = HookScripts {
            pre_request: Some(
                r#"
                let parts = [];
                for pair in req.queries { parts.push(pair[0] + "=" + pair[1]); }
                parts.sort(|a, b| if a < b { -1 } else if a > b { 1 } else { 0 });
                req.push_query("_", parts.reduce(|sum, p| if sum == () { p } else { sum + "&" + p }));
                proceed()
                "#
                .to_string(),
            ),
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let mut req = dummy_req(None);
        req.queries = vec![
            ("page".to_string(), "1".to_string()),
            ("limit".to_string(), "28".to_string()),
        ];

        tokio::task::block_in_place(|| registry.run_pre_request(&mut req, dummy_ctx())).unwrap();

        let signed = req
            .queries
            .iter()
            .find(|(k, _)| k == "_")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            signed,
            Some("limit=28&page=1"),
            "hook could not read queries and append a signature: {:?}",
            req.queries
        );
    }

    /// A source whose payloads arrive wrapped or encoded must be able to unwrap
    /// them before the blueprint sees the body.
    /// Signing and decryption hooks share the same cached material, so the code
    /// that parses it should exist once rather than in each hook body.
    /// A source that signs its requests needs a canonical query string: sorted,
    /// with repeated keys given explicit indices. Both matter — a server that
    /// recomputes the signature will reject anything else.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_hook_can_build_a_sorted_canonical_query_string() {
        let shared: std::collections::BTreeMap<String, String> =
            [("canonical".to_string(), "fn canonical_query(path, queries) {\n    let flat = [];\n    let counters = #{};\n    for pair in queries {\n        let k = pair[0];\n        if k.ends_with(\"[]\") {\n            let base = k.sub_string(0, k.len() - 2);\n            let n = if counters.contains(base) { counters[base] } else { 0 };\n            counters[base] = n + 1;\n            flat.push(base + \"[\" + n + \"]=\" + pair[1]);\n        } else {\n            flat.push(k + \"=\" + pair[1]);\n        }\n    }\n    flat.sort(|a, b| if a < b { -1 } else if a > b { 1 } else { 0 });\n    let qs = \"\";\n    for part in flat { qs = if qs == \"\" { part } else { qs + \"&\" + part }; }\n    if qs == \"\" { path } else { path + \"?\" + qs }\n}".to_string())]
                .into_iter()
                .collect();
        let scripts = HookScripts {
            shared,
            pre_request: Some(
                r#"req.set_header("X-Canonical", canonical_query("/items", req.queries)); proceed()"#
                    .to_string(),
            ),
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let mut req = dummy_req(None);
        // Deliberately unsorted, with one key repeated.
        req.queries = vec![
            ("sort[name]".into(), "desc".into()),
            ("page".into(), "1".into()),
            ("limit".into(), "28".into()),
            ("tag[]".into(), "alpha".into()),
            ("tag[]".into(), "beta".into()),
            ("keyword".into(), "test".into()),
        ];

        tokio::task::block_in_place(|| registry.run_pre_request(&mut req, dummy_ctx())).unwrap();

        let got = req
            .headers
            .iter()
            .find(|(k, _)| k == "X-Canonical")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            got,
            Some("/items?keyword=test&limit=28&page=1&sort[name]=desc&tag[0]=alpha&tag[1]=beta"),
            "canonical form is not sorted with indexed repeats"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_hook_can_call_a_shared_function() {
        let scripts = HookScripts {
            shared: [(
                "seed_for".to_string(),
                "fn seed_for(round) { [189, 133, 32][round] }".to_string(),
            )]
            .into_iter()
            .collect(),
            pre_request: Some(
                r#"req.set_header("X-Seed", "" + seed_for(0)); proceed()"#.to_string(),
            ),
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let mut req = dummy_req(None);

        tokio::task::block_in_place(|| registry.run_pre_request(&mut req, dummy_ctx())).unwrap();

        let seen = req
            .headers
            .iter()
            .find(|(k, _)| k == "X-Seed")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            seen,
            Some("189"),
            "hook could not call a shared pure function: {:?}",
            req.headers
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_hook_can_rewrite_the_response_body() {
        let scripts = HookScripts {
            on_status: [(
                "2xx".to_string(),
                r#"resp.body = bytes_to_utf8(bytes_from_base64url(resp.body)); proceed()"#
                    .to_string(),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let req = dummy_req(None);
        let mut resp = dummy_resp(200);
        resp.body = "eyJvayI6dHJ1ZX0".to_string();

        let action =
            tokio::task::block_in_place(|| registry.run_on_status(&req, &mut resp, dummy_ctx()))
                .unwrap();

        assert!(matches!(action.kind, HookActionKind::Proceed));
        assert_eq!(
            resp.body, r#"{"ok":true}"#,
            "hook could not rewrite the response body"
        );
    }

    #[test]
    fn endpoint_specific_hook_takes_precedence() {
        let mut ep_pre = std::collections::BTreeMap::new();
        ep_pre.insert(
            "search".to_string(),
            r#"req.set_header("X-Ep", "search"); proceed()"#.to_string(),
        );
        let scripts = HookScripts {
            pre_request: Some(r#"req.set_header("X-Ep", "global"); proceed()"#.to_string()),
            endpoint_pre_request: ep_pre,
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        let mut req = dummy_req(Some("search"));
        tokio::task::block_in_place(|| registry.run_pre_request(&mut req, dummy_ctx())).unwrap();
        let header_val = req
            .headers
            .iter()
            .find(|(k, _)| k == "X-Ep")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            header_val,
            Some("search"),
            "endpoint hook must win: {:?}",
            req.headers
        );
    }

    fn inheriting_registry() -> HookRegistry {
        let mut ep_pre = std::collections::BTreeMap::new();
        ep_pre.insert(
            "manga_details".to_string(),
            r#"req.set_header("X-Hook", "parent"); proceed()"#.to_string(),
        );
        let mut ep_status = std::collections::BTreeMap::new();
        ep_status.insert(
            "manga_details".to_string(),
            std::collections::BTreeMap::from([(
                "401".to_string(),
                r#"resp.body = "parent"; proceed()"#.to_string(),
            )]),
        );
        let scripts = HookScripts {
            pre_request: Some(r#"req.set_header("X-Hook", "global"); proceed()"#.to_string()),
            on_status: std::collections::BTreeMap::from([(
                "default".to_string(),
                r#"resp.body = "global"; proceed()"#.to_string(),
            )]),
            endpoint_pre_request: ep_pre,
            endpoint_on_status: ep_status,
            ..Default::default()
        };
        HookRegistry::compile(&scripts).unwrap()
    }

    #[test]
    fn sub_fetch_pre_request_inherits_its_parent_endpoint_hook() {
        let registry = inheriting_registry();
        for (endpoint_id, expected) in [
            (Some("manga_details/chapters"), "parent"),
            (Some("manga_details"), "parent"),
            (Some("search/details"), "global"),
            (Some("/orphan"), "global"),
            (None, "global"),
        ] {
            let mut req = dummy_req(endpoint_id);
            tokio::task::block_in_place(|| registry.run_pre_request(&mut req, dummy_ctx()))
                .unwrap();
            let header = req
                .headers
                .iter()
                .find(|(k, _)| k == "X-Hook")
                .map(|(_, v)| v.as_str());
            assert_eq!(header, Some(expected), "endpoint_id {endpoint_id:?}");
        }
    }

    #[test]
    fn sub_fetch_on_status_inherits_its_parent_endpoint_hook() {
        let registry = inheriting_registry();
        for (endpoint_id, status, expected) in [
            ("manga_details/chapters", 401, "parent"),
            ("manga_details/chapters", 500, "global"),
            ("search/details", 401, "global"),
        ] {
            let req = dummy_req(Some(endpoint_id));
            let mut resp = dummy_resp(status);
            tokio::task::block_in_place(|| registry.run_on_status(&req, &mut resp, dummy_ctx()))
                .unwrap();
            assert_eq!(resp.body, expected, "{endpoint_id} {status}");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hooks_may_use_only_declared_cache_namespaces() {
        let hook = |ns: &str| HookScripts {
            pre_request: Some(format!(r#"ctx.cache_put("{ns}", "k", "v", 60); proceed()"#)),
            cache: std::collections::BTreeMap::from([(
                "auth".to_string(),
                kani_shared::CacheNamespaceLimits {
                    ttl_seconds: 3600,
                    max_entries: None,
                },
            )]),
            ..Default::default()
        };
        let declared = HookRegistry::compile(&hook("auth")).unwrap();
        let mut req = dummy_req(None);
        assert!(declared.run_pre_request(&mut req, dummy_ctx()).is_ok());

        let undeclared = HookRegistry::compile(&hook("other")).unwrap();
        let err = undeclared
            .run_pre_request(&mut req, dummy_ctx())
            .unwrap_err();
        assert!(err.contains("not declared"), "got: {err}");
    }
}
