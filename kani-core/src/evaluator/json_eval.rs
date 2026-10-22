use crate::evaluator::shared::{
    Env, EvalBudget, Value, blueprint_has_fetch, charge_fetch_request, eval_common_expr,
    eval_fetch_field, fetch_body, header_pair, insert_field_value, send_prepared_request,
};
use kani_shared::ast::{Blueprint, Expr, OffsetType, OnFailurePolicy, SubBlueprintKind};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

struct PendingFetch<'a> {
    row_index: usize,
    field_name: String,
    optional: bool,
    item: &'a serde_json::Value,
    item_index: usize,
    env: Env,
    sub_blueprint: &'a Blueprint,
    kind: &'a SubBlueprintKind,
    on_failure: &'a OnFailurePolicy,
    request: kani_shared::ast::RequestDef,
}

async fn resolve_url_and_headers(
    state: &crate::wasm::HostState,
    doc: &serde_json::Value,
    item: &serde_json::Value,
    item_index: usize,
    env: Env,
    url_expr: &Expr,
    headers: &[(Expr, Expr)],
) -> Result<(String, Vec<(String, String)>), String> {
    let registry_arc = state.pure_fn_registry.clone();
    let registry = registry_arc.as_deref();
    let budget = Arc::clone(&state.eval_budget);
    let current = Some((item, item_index));

    let url_val = eval_json_expr(
        url_expr,
        doc,
        current,
        env.clone(),
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
        let k = eval_json_expr(
            k_expr,
            doc,
            current,
            env.clone(),
            registry,
            Arc::clone(&budget),
        )
        .await?;
        let v = eval_json_expr(
            v_expr,
            doc,
            current,
            env.clone(),
            registry,
            Arc::clone(&budget),
        )
        .await?;
        resolved_headers.push(header_pair(k, v)?);
    }
    Ok((url, resolved_headers))
}

pub async fn extract_json(
    state: &mut crate::wasm::HostState,
    doc_handle: Option<i32>,
    blueprint: &Blueprint,
) -> Result<serde_json::Value, String> {
    let doc = match (doc_handle, &blueprint.request) {
        (Some(h), _) => state.json_docs.get(&h).ok_or("Invalid handle")?.clone(),
        (None, Some(req)) => fetch_and_parse_json(state, req).await?,
        (None, None) => return Err("No document source".into()),
    };
    extract_json_with_doc(state, doc, blueprint).await
}

pub async fn extract_json_str(
    state: &mut crate::wasm::HostState,
    body: &str,
    blueprint: &Blueprint,
) -> Result<serde_json::Value, String> {
    let doc: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("JSON parse error: {}", e))?;
    extract_json_with_doc(state, doc, blueprint).await
}

async fn extract_json_with_doc(
    state: &mut crate::wasm::HostState,
    doc: serde_json::Value,
    blueprint: &Blueprint,
) -> Result<serde_json::Value, String> {
    state.eval_budget.reset();
    let mut env = Env::new();
    for (k, v) in &state.preferences {
        env.set(&format!("$pref:{}", k), Value::Str(v.clone()));
    }
    for binding in &blueprint.bindings {
        let val = eval_json_field(state, &binding.expr, &doc, None, env.clone()).await?;
        env.set(&binding.name, val);
    }

    let container_val = if blueprint.container.is_empty() {
        &doc
    } else {
        doc.pointer(&blueprint.container)
            .ok_or_else(|| format!("Container '{}' not found in document", blueprint.container))?
    };

    let items: Vec<&serde_json::Value> = match container_val.as_array() {
        Some(arr) => arr.iter().collect(),
        None => vec![container_val],
    };

    // A hostile source can serve an enormous listing; cap the container row count
    // at the evaluator's list-size limit rather than extracting all of it.
    let max_rows = state.eval_budget.limits.max_list_size;
    if items.len() > max_rows {
        return Err(format!("limit:max_list_size:{max_rows}"));
    }

    let mut scalars = serde_json::Map::new();
    for scalar in &blueprint.scalars {
        let val = eval_json_field(state, &scalar.expr, &doc, None, env.clone()).await?;
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
        Vec::with_capacity(items.len());
    let mut pending: Vec<PendingFetch> = Vec::new();

    for (index, item) in items.iter().enumerate() {
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
                    item,
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
                            item,
                            item_index: index,
                            env: env.clone(),
                            sub_blueprint: sub_bp,
                            kind,
                            on_failure,
                            request,
                        });
                        row.insert(field.name.clone(), serde_json::Value::Null);
                    }
                    Err(e) if crate::budget::is_budget_exceeded(&e) => return Err(e),
                    Err(e) => match on_failure {
                        kani_shared::ast::OnFailurePolicy::Skip => {
                            row.insert(field.name.clone(), serde_json::Value::Null);
                        }
                        kani_shared::ast::OnFailurePolicy::Fail => return Err(e),
                        kani_shared::ast::OnFailurePolicy::Use(fallback) => {
                            let registry_arc = state.pure_fn_registry.clone();
                            let registry = registry_arc.as_deref();
                            let budget = Arc::clone(&state.eval_budget);
                            let val = eval_json_expr(
                                fallback,
                                &doc,
                                Some((item, index)),
                                env.clone(),
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
            let val =
                eval_json_field(state, &field.expr, &doc, Some((item, index)), env.clone()).await?;
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
            let body_result =
                body_result.and_then(|body| state.charge_response_bytes(body.len()).map(|()| body));
            let outcome: Result<Value, String> = match body_result {
                Ok(body) => {
                    let parsed = match p.kind {
                        SubBlueprintKind::Html => {
                            Box::pin(crate::evaluator::html_eval::extract_html_str(
                                state,
                                &body,
                                p.sub_blueprint,
                            ))
                            .await
                        }
                        SubBlueprintKind::Json => {
                            Box::pin(extract_json_str(state, &body, p.sub_blueprint)).await
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
                (Err(e), _) if crate::budget::is_budget_exceeded(&e) => return Err(e),
                (Err(_), OnFailurePolicy::Skip) => {
                    results[p.row_index].insert(p.field_name.clone(), serde_json::Value::Null);
                }
                (Err(e), OnFailurePolicy::Fail) => return Err(e),
                (Err(_), OnFailurePolicy::Use(fallback)) => {
                    let registry_arc = state.pure_fn_registry.clone();
                    let registry = registry_arc.as_deref();
                    let budget = Arc::clone(&state.eval_budget);
                    let val = eval_json_expr(
                        fallback,
                        &doc,
                        Some((p.item, p.item_index)),
                        p.env,
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

async fn eval_json_field(
    state: &mut crate::wasm::HostState,
    expr: &Expr,
    doc: &serde_json::Value,
    current: Option<(&serde_json::Value, usize)>,
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
        let url_val = eval_json_expr(
            url_expr,
            doc,
            current,
            env.clone(),
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
            let k = eval_json_expr(
                k_expr,
                doc,
                current,
                env.clone(),
                registry,
                Arc::clone(&budget),
            )
            .await?;
            let v = eval_json_expr(
                v_expr,
                doc,
                current,
                env.clone(),
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
            (Err(e), _) if crate::budget::is_budget_exceeded(&e) => Err(e),
            (Err(_), kani_shared::ast::OnFailurePolicy::Skip) => Ok(Value::Null),
            (Err(e), kani_shared::ast::OnFailurePolicy::Fail) => Err(e),
            (Err(_), kani_shared::ast::OnFailurePolicy::Use(fallback)) => {
                eval_json_expr(fallback, doc, current, env, registry, budget).await
            }
        }
    } else {
        eval_json_expr(expr, doc, current, env, registry, budget).await
    }
}

/// Drops rows whose `key` expression repeats an earlier row's value, keeping the
/// first occurrence and the original order.
///
/// Runs over an extraction result rather than inside the blueprint, because the
/// key is only meaningful once a row's `for_each` sub-fetch has merged into it.
/// That also keeps it off the guest ABI: the expression stays in the validated
/// YAML model and never reaches a serialized `Blueprint`.
pub async fn deduplicate_rows(result: &mut serde_json::Value, key: &Expr) -> Result<(), String> {
    let Some(rows) = result.get_mut("rows").and_then(|r| r.as_array_mut()) else {
        return Ok(());
    };

    // Hashed rather than compared pairwise: `Expr::Unique` scans a Vec, which is
    // fine for a field's values and quadratic over a page of rows.
    fn key_of(value: &Value) -> String {
        match value {
            Value::Str(s) => format!("s:{s}"),
            Value::Int(i) => format!("i:{i}"),
            Value::Num(n) => format!("n:{n}"),
            Value::Bool(b) => format!("b:{b}"),
            Value::Null => "null".to_string(),
            Value::Json(j) => format!("j:{j}"),
            other => format!("d:{other:?}"),
        }
    }

    let budget = Arc::new(EvalBudget::new());
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut keep = Vec::with_capacity(rows.len());

    for row in rows.iter() {
        let value = eval_json_expr(
            key,
            row,
            Some((row, 0)),
            Env::new(),
            None,
            Arc::clone(&budget),
        )
        .await?;
        keep.push(matches!(value, Value::Null) || seen.insert(key_of(&value)));
    }

    let mut iter = keep.into_iter();
    rows.retain(|_| iter.next().unwrap_or(true));
    Ok(())
}

fn eval_json_expr<'a>(
    expression: &'a Expr,
    doc: &'a serde_json::Value,
    current: Option<(&'a serde_json::Value, usize)>,
    env: Env,
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
            return eval_json_expr(&materialized, doc, current, env, registry, budget).await;
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
            &|e, env| eval_json_expr(e, doc, current, env, registry, Arc::clone(&budget)),
            registry,
            budget.limits,
        )
        .await
        {
            return result;
        }

        match expression {
            Expr::Json(pointer) => Ok(doc
                .pointer(pointer)
                .map(|v| Value::Json(v.clone()))
                .unwrap_or(Value::Null)),

            Expr::SelfRef => current
                .map(|(n, _)| Value::Json((*n).clone()))
                .ok_or_else(|| "SelfRef used outside of a container loop".into()),

            Expr::Index => current
                .map(|(_, i)| Value::Int(i as i64))
                .ok_or_else(|| "Index used outside of a container loop".into()),

            Expr::JsonPtr { target, pointer } => {
                eval_json_expr(target, doc, current, env, registry, budget)
                    .await
                    .and_then(|v| v.into_json("ptr"))
                    .map(|v| {
                        v.pointer(pointer)
                            .map(|v| Value::Json(v.clone()))
                            .unwrap_or(Value::Null)
                    })
            }

            Expr::JsonStr { target } => eval_json_expr(target, doc, current, env, registry, budget)
                .await
                .and_then(|v| v.into_json("str"))
                .map(|v| {
                    v.as_str()
                        .map(|s| Value::Str(s.to_owned()))
                        .unwrap_or(Value::Null)
                }),

            Expr::JsonInt { target } => eval_json_expr(target, doc, current, env, registry, budget)
                .await
                .and_then(|v| v.into_json("int"))
                .map(|v| v.as_i64().map(Value::Int).unwrap_or(Value::Null)),

            Expr::JsonFloat { target } => {
                eval_json_expr(target, doc, current, env, registry, budget)
                    .await
                    .and_then(|v| v.into_json("float"))
                    .map(|v| v.as_f64().map(Value::Num).unwrap_or(Value::Null))
            }

            Expr::JsonBool { target } => {
                eval_json_expr(target, doc, current, env, registry, budget)
                    .await
                    .and_then(|v| v.into_json("bool"))
                    .map(|v| v.as_bool().map(Value::Bool).unwrap_or(Value::Null))
            }

            Expr::JsonKeys { target } => {
                eval_json_expr(target, doc, current, env, registry, budget)
                    .await
                    .and_then(|v| v.into_json("keys"))
                    .map(|v| {
                        Value::List(
                            v.as_object()
                                .map(|o| o.keys().map(|k| Value::Str(k.clone())).collect())
                                .unwrap_or_default(),
                        )
                    })
            }

            Expr::JsonGet { target, key } => {
                let val = eval_json_expr(
                    target,
                    doc,
                    current,
                    env.clone(),
                    registry,
                    Arc::clone(&budget),
                )
                .await
                .and_then(|v| v.into_json("get"))?;
                let key_str = eval_json_expr(key, doc, current, env, registry, budget)
                    .await
                    .and_then(|v| v.into_str("get"))?;
                Ok(val
                    .get(&key_str)
                    .map(|v| Value::Json(v.clone()))
                    .unwrap_or(Value::Null))
            }

            Expr::JsonFind { target, key, value } => {
                let arr = eval_json_expr(
                    target,
                    doc,
                    current,
                    env.clone(),
                    registry,
                    Arc::clone(&budget),
                )
                .await
                .and_then(|v| v.into_json("find"))?;
                let key_str = eval_json_expr(
                    key,
                    doc,
                    current,
                    env.clone(),
                    registry,
                    Arc::clone(&budget),
                )
                .await
                .and_then(|v| v.into_str("find"))?;
                let val_str = eval_json_expr(value, doc, current, env, registry, budget)
                    .await
                    .and_then(|v| v.into_str("find"))?;
                Ok(arr
                    .as_array()
                    .and_then(|items| {
                        items.iter().find(|item| {
                            item.get(&key_str).and_then(|v| v.as_str()) == Some(val_str.as_str())
                        })
                    })
                    .map(|v| Value::Json(v.clone()))
                    .unwrap_or(Value::Null))
            }

            _ => Err(format!(
                "Unhandled expression in JSON evaluator: {:?}",
                expression
            )),
        }
    })
}

async fn extract_json_cursor_paginated(
    state: &mut crate::wasm::HostState,
    page: i32,
    page_size: i32,
    blueprint: &Blueprint,
    offset_param: &str,
    next_cursor_field: &str,
    native_size: usize,
) -> Result<serde_json::Value, String> {
    let mut end = None;
    let slice_start = ((page - 1).max(0) as usize) * (page_size.max(0) as usize);
    let slice_end = slice_start + page_size.max(0) as usize;
    let mut chunk_start = 0usize;
    let mut cursor: Option<String> = None;
    let mut followed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut rows_out: Vec<serde_json::Value> = Vec::new();
    let mut last_scalars = serde_json::Map::new();

    let has_next_page = loop {
        let mut chunk_bp = blueprint.clone();
        if let (Some(req), Some(c)) = (&mut chunk_bp.request, &cursor) {
            req.queries.retain(|(k, _)| k != offset_param);
            req.queries.push((offset_param.to_string(), c.clone()));
        }
        let chunk = extract_json(state, None, &chunk_bp).await?;
        if let Some(map) = chunk["scalars"].as_object() {
            last_scalars = map.clone();
        }
        let empty = Vec::new();
        let rows = chunk["rows"].as_array().unwrap_or(&empty);
        let chunk_end = chunk_start + rows.len();
        let (from, to) = (slice_start.max(chunk_start), slice_end.min(chunk_end));
        if from < to {
            rows_out.extend_from_slice(&rows[from - chunk_start..to - chunk_start]);
        }

        let next = match &chunk["scalars"][next_cursor_field] {
            serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        };
        if let Some(c) = &cursor {
            followed.insert(c.clone());
        }
        let upstream_has_more = !rows.is_empty()
            && chunk["scalars"]["has_next_page"].as_bool() != Some(false)
            && next.as_ref().is_some_and(|n| !followed.contains(n));
        if !upstream_has_more {
            end = Some(chunk_end);
        }

        if chunk_end >= slice_end {
            break chunk_end > slice_end || upstream_has_more;
        }
        if !upstream_has_more {
            break false;
        }
        chunk_start = chunk_end;
        cursor = next;
    };

    let mut scalars = last_scalars;
    scalars.remove(next_cursor_field);
    scalars.insert("has_next_page".into(), serde_json::json!(has_next_page));
    rescale_total_pages(&mut scalars, native_size, page_size.max(0) as usize, end);
    Ok(serde_json::json!({ "rows": rows_out, "scalars": scalars }))
}

pub async fn extract_json_paginated(
    state: &mut crate::wasm::HostState,
    page: i32,
    page_size: i32,
    blueprint: &Blueprint,
) -> Result<serde_json::Value, String> {
    let pagination = blueprint
        .pagination
        .as_ref()
        .ok_or("paginated_extract_json called on blueprint without PaginationConfig")?;

    if let OffsetType::CursorToken { next_cursor_field } = &pagination.offset_type {
        return extract_json_cursor_paginated(
            state,
            page,
            page_size,
            blueprint,
            &pagination.offset_param,
            next_cursor_field,
            pagination.native_page_size,
        )
        .await;
    }

    let native_size = pagination.native_page_size;
    let mut walk = ChunkWalk::new(page, page_size, native_size);
    let mut all_rows: Vec<serde_json::Value> = Vec::new();
    let has_next_page;

    let mut last_scalars = serde_json::Map::new();

    loop {
        let current_chunk_offset = walk.chunk_offset;
        let mut chunk_bp = blueprint.clone();
        if let Some(req) = &mut chunk_bp.request {
            match &pagination.offset_type {
                OffsetType::ItemOffset => {
                    let offset_value = current_chunk_offset.to_string();
                    req.queries.retain(|(k, _)| k != &pagination.offset_param);
                    req.queries
                        .push((pagination.offset_param.clone(), offset_value));
                }
                OffsetType::PageNumber { start } => {
                    let offset_value =
                        (current_chunk_offset / native_size + *start as usize).to_string();
                    req.queries.retain(|(k, _)| k != &pagination.offset_param);
                    req.queries
                        .push((pagination.offset_param.clone(), offset_value));
                }
                OffsetType::CursorToken { .. } => {}
            }
        }

        let chunk_result = extract_json(state, None, &chunk_bp).await?;
        if let Some(map) = chunk_result["scalars"].as_object() {
            last_scalars = map.clone();
        }

        let empty = vec![];
        let rows = chunk_result["rows"].as_array().unwrap_or(&empty);
        let scalar_hnp = chunk_result["scalars"]["has_next_page"].as_bool();
        let (taken, done) = walk.take(rows, scalar_hnp);
        all_rows.extend_from_slice(taken);
        if let Some(next) = done {
            has_next_page = next;
            break;
        }
    }

    let mut scalars = last_scalars;
    scalars.insert("has_next_page".into(), serde_json::json!(has_next_page));
    rescale_total_pages(&mut scalars, native_size, page_size as usize, walk.end);

    Ok(serde_json::json!({ "rows": all_rows, "scalars": scalars }))
}

/// One client page's walk over a source's fixed-size chunks: which chunk to fetch next, which of
/// its rows belong to the page, and, once a chunk comes back short, how many items the source has.
pub struct ChunkWalk {
    native_size: usize,
    skip: usize,
    remaining: usize,
    /// Item offset of the next chunk to fetch.
    pub chunk_offset: usize,
    /// The source's exact item count, known once a chunk shorter than `native_size` was fetched.
    pub end: Option<usize>,
}

impl ChunkWalk {
    /// Starts the walk for 1-based `page` of `page_size` items.
    pub fn new(page: i32, page_size: i32, native_size: usize) -> Self {
        let native_size = native_size.max(1);
        let start = ((page - 1).max(0) as usize) * (page_size.max(0) as usize);
        Self {
            native_size,
            skip: start % native_size,
            remaining: page_size.max(0) as usize,
            chunk_offset: (start / native_size) * native_size,
            end: None,
        }
    }

    /// Takes this page's share of the chunk just fetched. Returns whether another page exists
    /// once the walk is over, or `None` when the next chunk is needed.
    pub fn take<'a, T>(
        &mut self,
        rows: &'a [T],
        declared_next: Option<bool>,
    ) -> (&'a [T], Option<bool>) {
        let skip = std::mem::take(&mut self.skip).min(rows.len());
        let taken = &rows[skip..skip + (rows.len() - skip).min(self.remaining)];
        self.remaining -= taken.len();
        let full = rows.len() >= self.native_size;
        if !full && declared_next != Some(true) {
            self.end = Some(self.chunk_offset + rows.len());
        }
        let left_over = skip + taken.len() < rows.len();
        if self.remaining == 0 {
            return (taken, Some(left_over || declared_next.unwrap_or(full)));
        }
        if rows.is_empty() || !full || declared_next == Some(false) {
            return (taken, Some(false));
        }
        self.chunk_offset += self.native_size;
        (taken, None)
    }
}

/// Test seam for the page-count rescale, which is otherwise reachable only
/// through a live paginated fetch.
pub fn rescale_total_pages_for_test(
    scalars: &mut serde_json::Map<String, serde_json::Value>,
    native_size: usize,
    page_size: usize,
) {
    rescale_total_pages(scalars, native_size, page_size, None)
}

/// Restates the source's `total_pages` (counted in `native_size` chunks) in pages of
/// `page_size`. A `total_items` scalar, or an `end` seen by a short chunk, is exact; otherwise the
/// count is an upper bound, since the last native page may be short.
pub fn rescale_total_pages(
    scalars: &mut serde_json::Map<String, serde_json::Value>,
    native_size: usize,
    page_size: usize,
    end: Option<usize>,
) {
    if page_size == 0 {
        return;
    }
    let scalar = |name: &str| scalars.get(name).and_then(serde_json::Value::as_i64);
    let native_pages = scalar("total_pages");

    let items = match (scalar("total_items"), end) {
        (Some(total), _) => total.max(0) as usize,
        (None, _) if native_pages.is_none() => return,
        (None, Some(end)) => end,
        (None, None) => {
            if native_size == page_size {
                return;
            }
            (native_pages.unwrap_or(0).max(0) as usize).saturating_mul(native_size)
        }
    };

    scalars.insert(
        "total_pages".into(),
        serde_json::json!(items.div_ceil(page_size)),
    );
}

async fn fetch_and_parse_json(
    state: &mut crate::wasm::HostState,
    req: &kani_shared::ast::RequestDef,
) -> Result<serde_json::Value, String> {
    let body = fetch_body(state, req).await?;
    serde_json::from_str(&body).map_err(|e| format!("JSON parse error: {}", e))
}
