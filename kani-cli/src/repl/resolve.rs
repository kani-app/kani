use crate::error::CliError;
use crate::yaml::model::{ValidatedEndpoint, ValidatedExtension};
use kani_shared::types::{ActiveFilter, FilterState};

/// The request an endpoint would actually send, before hooks run.
pub struct ResolvedRequest {
    pub request: kani_shared::ast::RequestDef,
    pub filters: Vec<ActiveFilter>,
}

impl ResolvedRequest {
    /// The URL with query parameters appended, as it would go on the wire.
    pub fn url(&self) -> String {
        if self.request.queries.is_empty() {
            return self.request.url.clone();
        }
        let query = self
            .request
            .queries
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        format!("{}?{}", self.request.url, query)
    }
}

/// Percent-encodes a query component, leaving `[` and `]` as sources write them.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'[' | b']' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Each filter's declared default, as the host would apply it when the caller
/// supplies none. A request built without these does not match what the server
/// sends, which is how a wrong filter shape reaches production unnoticed.
pub fn default_filters(ext: &ValidatedExtension) -> Vec<ActiveFilter> {
    use crate::yaml::schema::FilterDefault;

    ext.filters
        .iter()
        .filter_map(|f| {
            let state = match f.default.as_ref()? {
                FilterDefault::Bool(b) => FilterState::Checkbox(*b),
                FilterDefault::Option { name, value } => FilterState::Selection {
                    name: name.clone(),
                    value: value.clone(),
                },
                FilterDefault::Text(t) => FilterState::TextInput(t.clone()),
            };
            Some(ActiveFilter {
                filter_name: f.id.clone(),
                state,
            })
        })
        .collect()
}

/// Parse `name=value` overrides. A comma in the value makes it a multiselect,
/// matching how the UI submits repeated options.
pub fn parse_filter_overrides(overrides: &[String]) -> Result<Vec<ActiveFilter>, CliError> {
    overrides
        .iter()
        .map(|raw| {
            let (name, value) = raw.split_once('=').ok_or_else(|| {
                CliError::Other(format!("filter override {raw:?} is not name=value"))
            })?;
            let state = match value {
                "true" | "false" => FilterState::Checkbox(value == "true"),
                v if v.contains(',') => {
                    FilterState::Multiselect(v.split(',').map(str::to_owned).collect())
                }
                v => FilterState::Selection {
                    name: v.to_owned(),
                    value: v.to_owned(),
                },
            };
            Ok(ActiveFilter {
                filter_name: name.to_owned(),
                state,
            })
        })
        .collect()
}

/// Build the request for an endpoint exactly as the running host would, so a
/// dry run and a recording cannot disagree about what goes on the wire.
pub fn resolve(
    ext: &ValidatedExtension,
    ep: &ValidatedEndpoint,
    endpoint_name: &str,
    args: &[String],
    filter_overrides: &[String],
) -> Result<ResolvedRequest, CliError> {
    let route = substitute_route(&ep.route, args)?;
    let url = format!("{}{}", ext.base_url.trim_end_matches('/'), route);

    let mut queries: Vec<(String, String)> = ep
        .queries
        .iter()
        .map(|qe| {
            let value = match &qe.value {
                crate::yaml::model::QueryValue::Static(v) => v.clone(),
                crate::yaml::model::QueryValue::Arg(name) => arg_value(args, name),
            };
            (qe.key.clone(), value)
        })
        .collect();

    let mut filters = default_filters(ext);
    for override_filter in parse_filter_overrides(filter_overrides)? {
        filters.retain(|f| f.filter_name != override_filter.filter_name);
        filters.push(override_filter);
    }

    queries.extend(kani_yaml::apply_filters(
        &ep.filter_mapping,
        ep.filter_format.as_ref(),
        &filters,
    ));

    if let Some(p) = &ep.pagination {
        let page: usize = arg_value(args, "page").parse().unwrap_or(1);
        let offset = match p.offset_type {
            crate::yaml::schema::YamlOffsetType::Page => page.max(1) + p.page_start as usize - 1,
            crate::yaml::schema::YamlOffsetType::Item => {
                page.saturating_sub(1) * p.native_page_size.max(1)
            }
        };
        queries.retain(|(k, _)| k != &p.offset_param);
        queries.push((p.offset_param.clone(), offset.to_string()));
    }

    Ok(ResolvedRequest {
        request: kani_shared::ast::RequestDef {
            url,
            method: ep.method.clone(),
            headers: ep.headers.clone().into_iter().collect(),
            queries,
            endpoint_id: Some(endpoint_name.to_owned()),
        },
        filters,
    })
}

fn arg_value(args: &[String], name: &str) -> String {
    args.iter()
        .find(|a| a.starts_with(&format!("{name}=")))
        .and_then(|a| a.split_once('=').map(|x| x.1.to_owned()))
        .unwrap_or_default()
}

fn substitute_route(route: &str, args: &[String]) -> Result<String, CliError> {
    let mut result = route.to_owned();
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            result = result.replace(&format!("${key}$"), value);
        }
    }
    if result.contains('$') {
        return Err(CliError::Other(format!(
            "route {route:?} has unresolved placeholders; provide them as key=value args"
        )));
    }
    Ok(result)
}

/// Run the source's `pre_request` hook over a resolved request, so a dry run can
/// show what actually reaches the wire rather than what was declared.
pub fn apply_pre_request(
    state: &kani_core::wasm::HostState,
    request: &kani_shared::ast::RequestDef,
) -> Result<kani_shared::ast::RequestDef, CliError> {
    let Some(registry) = state.hook_registry.clone() else {
        return Ok(request.clone());
    };

    let mut scriptable = kani_core::scripting::ScriptableRequest {
        method: request.method.clone(),
        url: request.url.clone(),
        headers: request.headers.clone(),
        queries: request.queries.clone(),
        body: None,
        endpoint_id: request.endpoint_id.clone(),
    };

    let ctx = kani_core::scripting::ScriptableCtx {
        cache_backend: state.ext_cache.clone(),
        cache_namespace: state.ext_cache_namespace.clone(),
        prefs: state.preferences.clone(),
        v8_process: Some(state.v8_process.clone()),
        http: Some(state.http_client.clone()),
        browser_scripts: state.browser_scripts.clone(),
        browser_profile_key: Some(state.browser_profile_key.clone()),
        allowed_host: state.allowed_host.clone(),
        cache_namespaces: std::sync::Arc::default(),
    };

    tokio::task::block_in_place(|| registry.run_pre_request(&mut scriptable, ctx))
        .map_err(CliError::Other)?;

    Ok(kani_shared::ast::RequestDef {
        url: scriptable.url,
        method: scriptable.method,
        headers: scriptable.headers,
        queries: scriptable.queries,
        endpoint_id: scriptable.endpoint_id,
    })
}
