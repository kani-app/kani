use std::sync::Arc;

use super::{SourceBackend, wasm_source::WasmSource, yaml_source::YamlSource};

#[allow(clippy::too_many_arguments)]
pub fn build_wasm_source(
    engine: wasmtime::Engine,
    instance_pre: kani_core::wasm::KaniExtensionPre<kani_core::wasm::HostState>,
    smart_client: kani_core::http::SmartClient,
    base_url: Option<String>,
    unrestricted_http: bool,
    browser_enabled: bool,
    preferences: std::collections::HashMap<String, String>,
    ext_cache: Arc<dyn kani_core::cache::CacheBackend>,
    ext_cache_namespace: String,
    pure_fn_registry: Option<Arc<kani_core::scripting::PureFunctionRegistry>>,
    hook_registry: Option<Arc<kani_core::scripting::HookRegistry>>,
    limits: impl Into<kani_core::budget::SourceLimits>,
) -> SourceBackend {
    SourceBackend::Wasm(Box::new(WasmSource::new(
        engine,
        instance_pre,
        smart_client,
        base_url,
        unrestricted_http,
        browser_enabled,
        25,
        preferences,
        ext_cache,
        ext_cache_namespace,
        pure_fn_registry,
        hook_registry,
        limits,
    )))
}

pub fn build_yaml_source(
    config: Arc<kani_yaml::ValidatedExtension>,
    http: kani_core::http::SmartClient,
    cache: Arc<dyn kani_core::cache::CacheBackend>,
    cache_namespace: String,
    preferences: std::collections::HashMap<String, String>,
    browser_enabled: bool,
) -> SourceBackend {
    SourceBackend::Yaml(Box::new(YamlSource::new(
        config,
        http,
        cache,
        cache_namespace,
        preferences,
        browser_enabled,
    )))
}
