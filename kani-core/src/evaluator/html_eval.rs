use crate::evaluator::shared::{
    Env, EvalBudget, Value, blueprint_has_fetch, charge_fetch_request, eval_common_expr,
    eval_fetch_field, fetch_body, header_pair, insert_field_value, send_prepared_request,
};
use crate::wasm::StoredNode;
use kani_shared::ast::{Blueprint, Expr, OffsetType, OnFailurePolicy, SubBlueprintKind};
use scraper::{Element, Selector};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

struct PendingFetch<'a> {
    row_index: usize,
    field_name: String,
    optional: bool,
    element: &'a StoredNode,
    element_index: usize,
    env: Env,
    sub_blueprint: &'a Blueprint,
    kind: &'a SubBlueprintKind,
    on_failure: &'a OnFailurePolicy,
    request: kani_shared::ast::RequestDef,
}

async fn resolve_url_and_headers(
    state: &crate::wasm::HostState,
    doc: &StoredNode,
    element: &StoredNode,
    element_index: usize,
    env: Env,
    url_expr: &Expr,
    headers: &[(Expr, Expr)],
) -> Result<(String, Vec<(String, String)>), String> {
    let registry_arc = state.pure_fn_registry.clone();
    let registry = registry_arc.as_deref();
    let budget = Arc::clone(&state.eval_budget);
    let current = Some((element, element_index));

    let url_val = eval_html_expr(
        url_expr,
        doc,
        current,
        env.clone(),
        &state.selector_cache,
        registry,
        Arc::clone(&budget),
    )
    .await?;
    let url = match url_val {
        Value::Str(s) => s,
        _ => return Err("Fetch: url_expr must evaluate to a String".into()),
    };

    let mut resolved_headers = Vec::with_capacity(headers.len());
    for (k_expr, v_expr) in headers {
        let k = eval_html_expr(
            k_expr,
            doc,
            current,
            env.clone(),
            &state.selector_cache,
            registry,
            Arc::clone(&budget),
        )
        .await?;
        let v = eval_html_expr(
            v_expr,
            doc,
            current,
            env.clone(),
            &state.selector_cache,
            registry,
            Arc::clone(&budget),
        )
        .await?;
        resolved_headers.push(header_pair(k, v)?);
    }
    Ok((url, resolved_headers))
}

pub async fn extract_html(
    state: &mut crate::wasm::HostState,
    doc_handle: Option<i32>,
    blueprint: &Blueprint,
) -> Result<serde_json::Value, String> {
    let doc = match (doc_handle, &blueprint.request) {
        (Some(h), _) => state.html_docs.get(&h).ok_or("Invalid handle")?.clone(),
        (None, Some(req)) => fetch_and_parse_html(state, req).await?,
        (None, None) => return Err("No document source".into()),
    };
    extract_html_with_doc(state, doc, blueprint).await
}

pub async fn extract_html_str(
    state: &mut crate::wasm::HostState,
    body: &str,
    blueprint: &Blueprint,
) -> Result<serde_json::Value, String> {
    let node = crate::wasm::SendHtml::parse_document(body);
    let root_id = node
        .0
        .lock()
        .map_err(|_| "HTML document lock poisoned")?
        .0
        .root_element()
        .id();
    let doc = StoredNode {
        doc: node.0,
        node_id: root_id,
    };
    extract_html_with_doc(state, doc, blueprint).await
}

async fn extract_html_with_doc(
    state: &mut crate::wasm::HostState,
    doc: StoredNode,
    blueprint: &Blueprint,
) -> Result<serde_json::Value, String> {
    state.eval_budget.reset();
    let mut env = Env::new();
    for (k, v) in &state.preferences {
        env.set(&format!("$pref:{}", k), Value::Str(v.clone()));
    }
    for binding in &blueprint.bindings {
        let val = eval_html_field(state, &binding.expr, &doc, None, env.clone()).await?;
        env.set(&binding.name, val);
    }

    let container_elements = select_all(&doc, &blueprint.container, &state.selector_cache)?;

    // A hostile source can serve an enormous listing; cap the container row count
    // at the evaluator's list-size limit rather than extracting all of it.
    let max_rows = state.eval_budget.limits.max_list_size;
    if container_elements.len() > max_rows {
        return Err(format!("limit:max_list_size:{max_rows}"));
    }

    let mut scalars = serde_json::Map::new();
    for scalar in &blueprint.scalars {
        let val = eval_html_field(state, &scalar.expr, &doc, None, env.clone()).await?;
        match val.to_json() {
            Some(v) => {
                scalars.insert(scalar.name.clone(), v.clone());
                env.set(&format!("$scalar:{}", scalar.name), Value::Json(v));
            }
            None if scalar.optional => {
                scalars.insert(scalar.name.clone(), serde_json::Value::Null);
            }
            None => return Err(format!("Required scalar '{}' produced null", scalar.name)),
        }
    }

    let can_fan_out = state.hook_registry.is_none();
    let mut results: Vec<serde_json::Map<String, serde_json::Value>> =
        Vec::with_capacity(container_elements.len());
    let mut pending: Vec<PendingFetch> = Vec::new();

    for (index, element) in container_elements.iter().enumerate() {
        let mut row = serde_json::Map::new();
        for field in &blueprint.fields {
            if can_fan_out
                && let Expr::Fetch {
                    url_expr,
                    blueprint: sub_bp,
                    method,
                    headers,
                    kind,
                    on_failure,
                    endpoint_id,
                } = &field.expr
            {
                let (url, resolved_headers) = resolve_url_and_headers(
                    state,
                    &doc,
                    element,
                    index,
                    env.clone(),
                    url_expr,
                    headers,
                )
                .await?;
                let charge_result = if blueprint_has_fetch(sub_bp) {
                    Err("Nested Expr::Fetch inside a sub-blueprint is not allowed".to_string())
                } else {
                    charge_fetch_request(state, &url, method, resolved_headers, endpoint_id.clone())
                };
                match charge_result {
                    Ok(request) => {
                        pending.push(PendingFetch {
                            row_index: index,
                            field_name: field.name.clone(),
                            optional: field.optional,
                            element,
                            element_index: index,
                            env: env.clone(),
                            sub_blueprint: sub_bp,
                            kind,
                            on_failure,
                            request,
                        });
                        row.insert(field.name.clone(), serde_json::Value::Null);
                    }
                    Err(e) => match on_failure {
                        kani_shared::ast::OnFailurePolicy::Skip => {
                            row.insert(field.name.clone(), serde_json::Value::Null);
                        }
                        kani_shared::ast::OnFailurePolicy::Fail => return Err(e),
                        kani_shared::ast::OnFailurePolicy::Use(fallback) => {
                            let registry_arc = state.pure_fn_registry.clone();
                            let registry = registry_arc.as_deref();
                            let budget = Arc::clone(&state.eval_budget);
                            let val = eval_html_expr(
                                fallback,
                                &doc,
                                Some((element, index)),
                                env.clone(),
                                &state.selector_cache,
                                registry,
                                budget,
                            )
                            .await?;
                            insert_field_value(
                                &mut row,
                                &field.name,
                                field.optional,
                                val,
                                state.eval_budget.limits.max_string_length,
                            )?;
                        }
                    },
                }
                continue;
            }
            let val = eval_html_field(
                state,
                &field.expr,
                &doc,
                Some((element, index)),
                env.clone(),
            )
            .await?;
            insert_field_value(
                &mut row,
                &field.name,
                field.optional,
                val,
                state.eval_budget.limits.max_string_length,
            )?;
        }
        results.push(row);
    }

    if !pending.is_empty() {
        let client = state.http_client.clone();
        let allowed_host = state.allowed_host.clone();
        let sends = pending.iter().map(|p| {
            send_prepared_request(client.clone(), allowed_host.clone(), p.request.clone())
        });
        let bodies: Vec<Result<String, String>> = futures::future::join_all(sends).await;

        for (p, body_result) in pending.into_iter().zip(bodies) {
            state.last_io_at = Some(std::time::Instant::now());
            let outcome: Result<Value, String> = match body_result {
                Ok(body) => {
                    let parsed = match p.kind {
                        SubBlueprintKind::Html => {
                            Box::pin(extract_html_str(state, &body, p.sub_blueprint)).await
                        }
                        SubBlueprintKind::Json => {
                            Box::pin(crate::evaluator::json_eval::extract_json_str(
                                state,
                                &body,
                                p.sub_blueprint,
                            ))
                            .await
                        }
                    };
                    parsed.map(|result| {
                        let first = result["rows"].as_array().and_then(|a| a.first()).cloned();
                        first.map(Value::Json).unwrap_or(Value::Null)
                    })
                }
                Err(e) => Err(e),
            };
            match (outcome, p.on_failure) {
                (Ok(v), _) => insert_field_value(
                    &mut results[p.row_index],
                    &p.field_name,
                    p.optional,
                    v,
                    state.eval_budget.limits.max_string_length,
                )?,
                (Err(_), OnFailurePolicy::Skip) => {
                    results[p.row_index].insert(p.field_name.clone(), serde_json::Value::Null);
                }
                (Err(e), OnFailurePolicy::Fail) => return Err(e),
                (Err(_), OnFailurePolicy::Use(fallback)) => {
                    let registry_arc = state.pure_fn_registry.clone();
                    let registry = registry_arc.as_deref();
                    let budget = Arc::clone(&state.eval_budget);
                    let val = eval_html_expr(
                        fallback,
                        &doc,
                        Some((p.element, p.element_index)),
                        p.env,
                        &state.selector_cache,
                        registry,
                        budget,
                    )
                    .await?;
                    insert_field_value(
                        &mut results[p.row_index],
                        &p.field_name,
                        p.optional,
                        val,
                        state.eval_budget.limits.max_string_length,
                    )?
                }
            }
        }
    }

    let results: Vec<serde_json::Value> =
        results.into_iter().map(serde_json::Value::Object).collect();
    Ok(serde_json::json!({ "rows": results, "scalars": scalars }))
}

async fn eval_html_field(
    state: &mut crate::wasm::HostState,
    expr: &Expr,
    doc: &StoredNode,
    current: Option<(&StoredNode, usize)>,
    env: Env,
) -> Result<Value, String> {
    let registry_arc = state.pure_fn_registry.clone();
    let registry = registry_arc.as_deref();
    let budget = Arc::clone(&state.eval_budget);
    if let Expr::Fetch {
        url_expr,
        blueprint: sub_bp,
        method,
        headers,
        kind,
        on_failure,
        endpoint_id,
    } = expr
    {
        let url_val = eval_html_expr(
            url_expr,
            doc,
            current,
            env.clone(),
            &state.selector_cache,
            registry,
            Arc::clone(&budget),
        )
        .await?;
        let url = match url_val {
            Value::Str(s) => s,
            _ => return Err("Fetch: url_expr must evaluate to a String".into()),
        };
        let mut resolved_headers = Vec::with_capacity(headers.len());
        for (k_expr, v_expr) in headers {
            let k = eval_html_expr(
                k_expr,
                doc,
                current,
                env.clone(),
                &state.selector_cache,
                registry,
                Arc::clone(&budget),
            )
            .await?;
            let v = eval_html_expr(
                v_expr,
                doc,
                current,
                env.clone(),
                &state.selector_cache,
                registry,
                Arc::clone(&budget),
            )
            .await?;
            resolved_headers.push(header_pair(k, v)?);
        }
        let result = eval_fetch_field(
            state,
            &url,
            method,
            resolved_headers,
            sub_bp,
            kind,
            endpoint_id.clone(),
        )
        .await;
        match (result, on_failure) {
            (Ok(v), _) => Ok(v),
            (Err(_), kani_shared::ast::OnFailurePolicy::Skip) => Ok(Value::Null),
            (Err(e), kani_shared::ast::OnFailurePolicy::Fail) => Err(e),
            (Err(_), kani_shared::ast::OnFailurePolicy::Use(fallback)) => {
                eval_html_expr(
                    fallback,
                    doc,
                    current,
                    env,
                    &state.selector_cache,
                    registry,
                    budget,
                )
                .await
            }
        }
    } else {
        eval_html_expr(
            expr,
            doc,
            current,
            env,
            &state.selector_cache,
            registry,
            budget,
        )
        .await
    }
}

pub async fn extract_html_paginated(
    state: &mut crate::wasm::HostState,
    page: i32,
    page_size: i32,
    blueprint: &Blueprint,
) -> Result<serde_json::Value, String> {
    let pagination = blueprint
        .pagination
        .as_ref()
        .ok_or("paginated_extract_html called on blueprint without PaginationConfig")?;

    let native_size = pagination.native_page_size;
    let global_start = ((page - 1).max(0) as usize) * (page_size as usize);
    let first_chunk_offset = (global_start / native_size) * native_size;
    let offset_in_first_chunk = global_start % native_size;
    let mut remaining = page_size as usize;
    let mut current_chunk_offset = first_chunk_offset;
    let mut all_rows: Vec<serde_json::Value> = Vec::new();
    let has_next_page;
    let mut last_scalars = serde_json::Map::new();

    loop {
        let mut chunk_bp = blueprint.clone();
        if let Some(req) = &mut chunk_bp.request {
            let offset_value = match &pagination.offset_type {
                OffsetType::ItemOffset => current_chunk_offset.to_string(),
                OffsetType::PageNumber { start } => {
                    (current_chunk_offset / native_size + *start as usize).to_string()
                }
                OffsetType::CursorToken { .. } => {
                    return Err(
                        "CursorToken pagination is not supported for HTML extraction".into(),
                    );
                }
            };
            req.queries.retain(|(k, _)| k != &pagination.offset_param);
            req.queries
                .push((pagination.offset_param.clone(), offset_value));
        }

        let chunk_result = extract_html(state, None, &chunk_bp).await?;

        let empty = vec![];
        let rows = chunk_result["rows"].as_array().unwrap_or(&empty);
        let chunk_len = rows.len();

        let skip = if current_chunk_offset == first_chunk_offset {
            offset_in_first_chunk
        } else {
            0
        };
        let available = chunk_len.saturating_sub(skip);
        let to_take = available.min(remaining);

        all_rows.extend_from_slice(&rows[skip..skip + to_take]);
        remaining -= to_take;

        if let Some(map) = chunk_result["scalars"].as_object() {
            last_scalars = map.clone();
        }
        let scalar_hnp = chunk_result["scalars"]["has_next_page"].as_bool();
        let chunk_full = chunk_len >= native_size;

        if remaining == 0 {
            has_next_page = scalar_hnp.unwrap_or(chunk_full);
            break;
        }
        if chunk_len == 0 || !chunk_full || scalar_hnp == Some(false) {
            has_next_page = false;
            break;
        }

        current_chunk_offset += native_size;
    }

    let mut scalars = last_scalars;
    scalars.insert("has_next_page".into(), serde_json::json!(has_next_page));
    crate::evaluator::json_eval::rescale_total_pages(&mut scalars, native_size, page_size as usize);

    Ok(serde_json::json!({ "rows": all_rows, "scalars": scalars }))
}

/// Evaluates an expression in an HTML document context.
///
/// Returns a boxed future (rather than `async fn`) so recursive calls through
/// `eval_common_expr` don't produce an infinitely-sized state machine.
fn eval_html_expr<'a>(
    expression: &'a Expr,
    doc: &'a StoredNode,
    current: Option<(&'a StoredNode, usize)>,
    env: Env,
    cache: &'a Mutex<HashMap<String, Arc<Selector>>>,
    registry: Option<&'a crate::scripting::PureFunctionRegistry>,
    budget: Arc<EvalBudget>,
) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>> {
    Box::pin(async move {
        if let Expr::Arena { arena, root } = expression {
            if let Some(result) =
                crate::evaluator::shared::eval_flat_arena(arena, *root, &env, &budget)
            {
                return result;
            }
            tokio::task::yield_now().await;
            let materialized = arena.node_expr(*root)?;
            let mut env = env;
            env.set(
                crate::evaluator::shared::ARENA_ENV_MARKER,
                Value::Bool(true),
            );
            return eval_html_expr(&materialized, doc, current, env, cache, registry, budget).await;
        }
        budget.charge_step()?;
        let _depth_guard = if env
            .get(crate::evaluator::shared::ARENA_ENV_MARKER)
            .is_some()
        {
            None
        } else {
            Some(budget.enter_depth()?)
        };

        if let Some(result) = eval_common_expr(
            expression,
            env.clone(),
            &|e, env| eval_html_expr(e, doc, current, env, cache, registry, Arc::clone(&budget)),
            registry,
            budget.limits,
        )
        .await
        {
            return result;
        }

        match expression {
            Expr::SelfRef => current
                .map(|(n, _)| Value::HtmlElement {
                    doc: n.doc.clone(),
                    node_id: n.node_id,
                })
                .ok_or_else(|| "SelfRef used outside of a container loop".into()),

            Expr::Index => current
                .map(|(_, i)| Value::Int(i as i64))
                .ok_or_else(|| "Index used outside of a container loop".into()),

            Expr::Dom(selector) => {
                let sel = get_or_cache_selector(cache, selector)?;
                let guard = doc.doc.lock().map_err(|_| "HTML document lock poisoned")?;
                Ok(guard
                    .0
                    .select(&sel)
                    .next()
                    .map_or(Value::Null, |el| Value::HtmlElement {
                        doc: doc.doc.clone(),
                        node_id: el.id(),
                    }))
            }

            Expr::Attr { target, name } => {
                match eval_html_expr(
                    target,
                    doc,
                    current,
                    env,
                    cache,
                    registry,
                    Arc::clone(&budget),
                )
                .await?
                .into_html_element("attr")?
                {
                    None => Ok(Value::Null),
                    Some(node) => node.with_element(|el| {
                        Ok(el
                            .attr(name)
                            .map(|a| Value::Str(a.to_string()))
                            .unwrap_or(Value::Null))
                    }),
                }
            }

            Expr::Text { target } => {
                match eval_html_expr(
                    target,
                    doc,
                    current,
                    env,
                    cache,
                    registry,
                    Arc::clone(&budget),
                )
                .await?
                .into_html_element("text")?
                {
                    None => Ok(Value::Null),
                    Some(node) => node.with_element(|el| Ok(Value::Str(el.text().collect()))),
                }
            }

            Expr::InnerHtml { target } => {
                match eval_html_expr(
                    target,
                    doc,
                    current,
                    env,
                    cache,
                    registry,
                    Arc::clone(&budget),
                )
                .await?
                .into_html_element("inner_html")?
                {
                    None => Ok(Value::Null),
                    Some(node) => node.with_element(|el| Ok(Value::Str(el.inner_html()))),
                }
            }

            Expr::Select { target, selector } => {
                match eval_html_expr(
                    target,
                    doc,
                    current,
                    env,
                    cache,
                    registry,
                    Arc::clone(&budget),
                )
                .await?
                .into_html_element("select")?
                {
                    None => Ok(Value::Null),
                    Some(node) => {
                        let sel = get_or_cache_selector(cache, selector)?;
                        node.with_element(|el| {
                            Ok(Value::List(
                                el.select(&sel)
                                    .map(|e| Value::HtmlElement {
                                        doc: node.doc.clone(),
                                        node_id: e.id(),
                                    })
                                    .collect(),
                            ))
                        })
                    }
                }
            }

            Expr::First { target, selector } => {
                match eval_html_expr(
                    target,
                    doc,
                    current,
                    env,
                    cache,
                    registry,
                    Arc::clone(&budget),
                )
                .await?
                .into_html_element("first")?
                {
                    None => Ok(Value::Null),
                    Some(node) => {
                        let sel = get_or_cache_selector(cache, selector)?;
                        node.with_element(|el| {
                            Ok(el
                                .select(&sel)
                                .next()
                                .map_or(Value::Null, |e| Value::HtmlElement {
                                    doc: node.doc.clone(),
                                    node_id: e.id(),
                                }))
                        })
                    }
                }
            }

            Expr::HasClass { target, class } => {
                match eval_html_expr(
                    target,
                    doc,
                    current,
                    env,
                    cache,
                    registry,
                    Arc::clone(&budget),
                )
                .await?
                .into_html_element("has_class")?
                {
                    None => Ok(Value::Null),
                    Some(node) => node.with_element(|el| {
                        Ok(Value::Bool(el.has_class(
                            &class.as_str().into(),
                            scraper::CaseSensitivity::CaseSensitive,
                        )))
                    }),
                }
            }

            Expr::Children { target } => {
                match eval_html_expr(target, doc, current, env, cache, registry, budget)
                    .await?
                    .into_html_element("children")?
                {
                    None => Ok(Value::Null),
                    Some(node) => node.with_element(|el| {
                        Ok(Value::List(
                            el.children()
                                .filter_map(scraper::ElementRef::wrap)
                                .map(|child| Value::HtmlElement {
                                    doc: node.doc.clone(),
                                    node_id: child.id(),
                                })
                                .collect(),
                        ))
                    }),
                }
            }

            _ => Err(format!(
                "Unhandled expression in HTML evaluator: {:?}",
                expression
            )),
        }
    })
}

fn get_or_cache_selector(
    cache: &Mutex<HashMap<String, Arc<Selector>>>,
    selector: &str,
) -> Result<Arc<Selector>, String> {
    {
        let guard = cache
            .lock()
            .map_err(|_| "selector cache lock poisoned".to_string())?;
        if let Some(sel) = guard.get(selector) {
            return Ok(sel.clone());
        }
    }
    let parsed = Selector::parse(selector)
        .map_err(|e| format!("Invalid CSS selector '{}': {:?}", selector, e))?;
    let arc_sel = Arc::new(parsed);
    cache
        .lock()
        .map_err(|_| "selector cache lock poisoned".to_string())?
        .insert(selector.to_owned(), arc_sel.clone());
    Ok(arc_sel)
}

async fn fetch_and_parse_html(
    state: &mut crate::wasm::HostState,
    req: &kani_shared::ast::RequestDef,
) -> Result<StoredNode, String> {
    let html_str = fetch_body(state, req).await?;
    let node = crate::wasm::SendHtml::parse_document(&html_str);
    let root_id = node
        .0
        .lock()
        .map_err(|_| "HTML document lock poisoned")?
        .0
        .root_element()
        .id();
    Ok(StoredNode {
        doc: node.0,
        node_id: root_id,
    })
}

fn select_all(
    node: &StoredNode,
    container: &str,
    cache: &Mutex<HashMap<String, Arc<Selector>>>,
) -> Result<Vec<StoredNode>, String> {
    if container == ":root" {
        return Ok(vec![node.clone()]);
    }

    let sel = get_or_cache_selector(cache, container)?;
    let guard = node.doc.lock().map_err(|_| "HTML document lock poisoned")?;
    let tree_node = guard
        .0
        .tree
        .get(node.node_id)
        .ok_or("HTML node no longer present in document tree")?;
    Ok(match scraper::ElementRef::wrap(tree_node) {
        Some(el) => el
            .select(&sel)
            .map(|e| StoredNode {
                doc: node.doc.clone(),
                node_id: e.id(),
            })
            .collect(),
        None => vec![],
    })
}
