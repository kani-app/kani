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
        let sends = pending
            .iter()
            .map(|p| send_prepared_request(client.clone(), p.request.clone()));
        let bodies: Vec<Result<String, String>> = futures::future::join_all(sends).await;

        for (p, body_result) in pending.into_iter().zip(bodies) {
            state.last_io_at = Some(std::time::Instant::now());
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
        keep.push(seen.insert(key_of(&value)));
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

            Expr::ArrayLen { target } => {
                eval_json_expr(target, doc, current, env, registry, budget)
                    .await
                    .and_then(|v| v.into_json("array_len"))
                    .map(|v| Value::Int(v.as_array().map(|a| a.len() as i64).unwrap_or(0)))
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

            Expr::JsonArray(items) => {
                let mut arr = Vec::with_capacity(items.len());
                for item in items {
                    let v = eval_json_expr(
                        item,
                        doc,
                        current,
                        env.clone(),
                        registry,
                        Arc::clone(&budget),
                    )
                    .await?;
                    arr.push(v.to_json().unwrap_or(serde_json::Value::Null));
                }
                Ok(Value::Json(serde_json::Value::Array(arr)))
            }

            Expr::JsonFold { target } => {
                let items = eval_json_expr(target, doc, current, env, registry, budget)
                    .await
                    .and_then(|v| v.into_list("json_fold"))?;
                let mut merged: Option<serde_json::Value> = None;
                for item in items {
                    let v = item.into_json("json_fold")?;
                    if v.is_null() {
                        continue;
                    }
                    merged = Some(match merged {
                        None => v,
                        Some(acc) => json_merge_two(acc, v)?,
                    });
                }
                Ok(merged.map(Value::Json).unwrap_or(Value::Null))
            }

            _ => Err(format!(
                "Unhandled expression in JSON evaluator: {:?}",
                expression
            )),
        }
    })
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

    let native_size = pagination.native_page_size;
    let global_start = ((page - 1).max(0) as usize) * (page_size as usize);
    let first_chunk_offset = (global_start / native_size) * native_size;
    let offset_in_first_chunk = global_start % native_size;
    let mut remaining = page_size as usize;
    let mut current_chunk_offset = first_chunk_offset;
    let mut all_rows: Vec<serde_json::Value> = Vec::new();
    let has_next_page;

    let mut cursor: Option<String> = None;
    let mut last_scalars = serde_json::Map::new();

    loop {
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
                OffsetType::CursorToken { .. } => {
                    if let Some(ref c) = cursor {
                        req.queries.retain(|(k, _)| k != &pagination.offset_param);
                        req.queries
                            .push((pagination.offset_param.clone(), c.clone()));
                    }
                }
            }
        }

        let chunk_result = extract_json(state, None, &chunk_bp).await?;
        if let Some(map) = chunk_result["scalars"].as_object() {
            last_scalars = map.clone();
        }

        let empty = vec![];
        let rows = chunk_result["rows"].as_array().unwrap_or(&empty);
        let chunk_len = rows.len();

        let skip = if matches!(pagination.offset_type, OffsetType::CursorToken { .. }) {
            0
        } else if current_chunk_offset == first_chunk_offset {
            offset_in_first_chunk
        } else {
            0
        };
        let available = chunk_len.saturating_sub(skip);
        let to_take = available.min(remaining);

        all_rows.extend_from_slice(&rows[skip..skip + to_take]);
        remaining -= to_take;

        if let OffsetType::CursorToken { next_cursor_field } = &pagination.offset_type {
            let next = chunk_result["scalars"][next_cursor_field.as_str()]
                .as_str()
                .map(str::to_owned);
            let scalar_hnp = chunk_result["scalars"]["has_next_page"].as_bool();
            if remaining == 0 {
                has_next_page = scalar_hnp.unwrap_or_else(|| next.is_some());
                break;
            }
            if chunk_len == 0 || next.is_none() || scalar_hnp == Some(false) {
                has_next_page = false;
                break;
            }
            cursor = next;
            continue;
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
    rescale_total_pages(&mut scalars, native_size, page_size as usize);

    Ok(serde_json::json!({ "rows": all_rows, "scalars": scalars }))
}

/// A source counts pages at its own chunk size; the client asked for pages of
/// `page_size`. Without this the strip shows the source's page count, which is
/// wrong by the ratio between the two. A `total_items` scalar gives the exact
/// answer; the page-count fallback overestimates by up to one native chunk.
/// Test seam for the page-count rescale, which is otherwise reachable only
/// through a live paginated fetch.
pub fn rescale_total_pages_for_test(
    scalars: &mut serde_json::Map<String, serde_json::Value>,
    native_size: usize,
    page_size: usize,
) {
    rescale_total_pages(scalars, native_size, page_size)
}

pub(crate) fn rescale_total_pages(
    scalars: &mut serde_json::Map<String, serde_json::Value>,
    native_size: usize,
    page_size: usize,
) {
    if page_size == 0 {
        return;
    }
    let scalar = |name: &str| scalars.get(name).and_then(serde_json::Value::as_i64);

    let items = match scalar("total_items") {
        Some(total) => total.max(0) as usize,
        None => {
            if native_size == page_size {
                return;
            }
            let Some(native_pages) = scalar("total_pages") else {
                return;
            };
            (native_pages.max(0) as usize).saturating_mul(native_size)
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

fn json_merge_two(a: serde_json::Value, b: serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::Value as J;
    match (a, b) {
        (J::Object(mut ma), J::Object(mb)) => {
            for (k, v) in mb {
                ma.insert(k, v);
            }
            Ok(J::Object(ma))
        }
        (J::Array(mut va), J::Array(vb)) => {
            va.extend(vb);
            Ok(J::Array(va))
        }
        (a, b) => Err(format!(
            "json_merge: cannot merge {} with {}",
            type_str(&a),
            type_str(&b)
        )),
    }
}

fn type_str(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
