use crate::utilities::parse_date_flexible;
use crate::wasm::{SafeHtml, StoredNode};
use ego_tree::NodeId;
use kani_shared::ast::{Expr, ExprArena, ExprLeaf, ExprNode, Op, PadAlign};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub(super) const MAX_EVAL_ITERATIONS: u32 = 100_000;
pub(super) const MAX_EVAL_DEPTH: u32 = 50;
pub(super) const MAX_LIST_SIZE: usize = 10_000;

/// Marker on an evaluator error that carries an HTTP status the caller should
/// classify (`__http_status__:429:120`). Canonical definition lives in
/// `kani_shared` so both backends decode it identically; re-exported here for
/// the evaluator that produces it.
pub use kani_shared::extension::HTTP_STATUS_ERR_PREFIX;
pub(super) const MAX_STRING_LENGTH: usize = 1_000_000;
pub(super) const ARENA_ENV_MARKER: &str = "\0kani:arena";

/// Overridable ceilings for the declarative evaluator. Production uses
/// [`EvalLimits::default`] (the `MAX_*` consts); tests shrink them via
/// `EvalBudget::with_limits` to trip a cap without a giant fixture.
#[derive(Debug, Clone, Copy)]
pub struct EvalLimits {
    pub max_iterations: u32,
    pub max_depth: u32,
    pub max_list_size: usize,
    pub max_string_length: usize,
}

impl Default for EvalLimits {
    fn default() -> Self {
        Self {
            max_iterations: MAX_EVAL_ITERATIONS,
            max_depth: MAX_EVAL_DEPTH,
            max_list_size: MAX_LIST_SIZE,
            max_string_length: MAX_STRING_LENGTH,
        }
    }
}

#[derive(Debug)]
pub struct EvalBudget {
    pub steps_remaining: AtomicU32,
    pub depth_current: AtomicU32,
    pub limits: EvalLimits,
}

impl Default for EvalBudget {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalBudget {
    pub fn new() -> Self {
        Self::with_limits(EvalLimits::default())
    }

    pub(crate) fn with_limits(limits: EvalLimits) -> Self {
        Self {
            steps_remaining: AtomicU32::new(limits.max_iterations),
            depth_current: AtomicU32::new(0),
            limits,
        }
    }

    pub fn reset(&self) {
        self.steps_remaining
            .store(self.limits.max_iterations, Ordering::Relaxed);
        self.depth_current.store(0, Ordering::Relaxed);
    }

    pub(crate) fn charge_step(&self) -> Result<(), String> {
        let prev = self
            .steps_remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                if n > 0 { Some(n - 1) } else { None }
            });
        if prev.is_err() {
            Err(format!(
                "limit:max_iterations:{}",
                self.limits.max_iterations
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn enter_depth(self: &Arc<Self>) -> Result<DepthGuard, String> {
        let d = self.depth_current.fetch_add(1, Ordering::Relaxed);
        if d >= self.limits.max_depth {
            self.depth_current.fetch_sub(1, Ordering::Relaxed);
            Err(format!("limit:max_depth:{}", self.limits.max_depth))
        } else {
            Ok(DepthGuard {
                budget: Arc::clone(self),
            })
        }
    }
}

#[derive(Debug)]
pub struct DepthGuard {
    budget: Arc<EvalBudget>,
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        self.budget.depth_current.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub enum Value {
    Str(String),
    Num(f64),
    Int(i64),
    Bool(bool),
    List(Vec<Value>),
    Null,
    HtmlElement {
        doc: Arc<Mutex<SafeHtml>>,
        node_id: NodeId,
    },
    Json(serde_json::Value),
}

pub(super) fn eval_flat_arena(
    arena: &ExprArena,
    root: kani_shared::ast::ExprId,
    env: &Env,
    budget: &Arc<EvalBudget>,
) -> Option<Result<Value, String>> {
    let mut reachable = vec![false; arena.nodes.len()];
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let node = arena.nodes.get(id.0 as usize)?;
        if std::mem::replace(&mut reachable[id.0 as usize], true) {
            continue;
        }
        pending.extend(node.children());
    }
    if arena.nodes.iter().enumerate().any(|(index, node)| {
        reachable[index]
            && !matches!(
                node,
                ExprNode::Leaf(
                    ExprLeaf::Literal(_)
                        | ExprLeaf::Number(_)
                        | ExprLeaf::Null
                        | ExprLeaf::Bool(_)
                        | ExprLeaf::Var(_)
                ) | ExprNode::Unary {
                    op: kani_shared::ast::UnaryExprOp::Trim,
                    ..
                } | ExprNode::BinaryOperation {
                    op: Op::Add
                        | Op::Sub
                        | Op::Mul
                        | Op::Div
                        | Op::Eq
                        | Op::Ne
                        | Op::Lt
                        | Op::Gt
                        | Op::Le
                        | Op::Ge,
                    ..
                }
            )
    }) {
        return None;
    }

    Some((|| {
        arena.validate(root)?;
        let mut values = vec![None::<Value>; arena.nodes.len()];
        for (index, node) in arena.nodes.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            budget.charge_step()?;
            let value = match node {
                ExprNode::Leaf(ExprLeaf::Literal(value)) => Value::Str(value.clone()),
                ExprNode::Leaf(ExprLeaf::Number(value)) => Value::Num(*value),
                ExprNode::Leaf(ExprLeaf::Null) => Value::Null,
                ExprNode::Leaf(ExprLeaf::Bool(value)) => Value::Bool(*value),
                ExprNode::Leaf(ExprLeaf::Var(name)) => env
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("Undefined variable '{name}'"))?,
                ExprNode::Unary {
                    op: kani_shared::ast::UnaryExprOp::Trim,
                    target,
                } => values
                    .get(target.0 as usize)
                    .and_then(Option::as_ref)
                    .cloned()
                    .ok_or_else(|| "invalid arena trim target".to_string())?
                    .map_str("trim", |value| Ok(Value::Str(value.trim().to_owned())))?,
                ExprNode::BinaryOperation { op, lhs, rhs } => {
                    let lhs = values
                        .get(lhs.0 as usize)
                        .and_then(Option::as_ref)
                        .cloned()
                        .ok_or_else(|| "invalid arena left operand".to_string())?;
                    let rhs = values
                        .get(rhs.0 as usize)
                        .and_then(Option::as_ref)
                        .cloned()
                        .ok_or_else(|| "invalid arena right operand".to_string())?;
                    match op {
                        Op::Add
                        | Op::Sub
                        | Op::Mul
                        | Op::Div
                        | Op::Lt
                        | Op::Gt
                        | Op::Le
                        | Op::Ge => numeric_op(op, lhs, rhs)?,
                        Op::Eq => Value::Bool(lhs == rhs),
                        Op::Ne => Value::Bool(lhs != rhs),
                        Op::And | Op::Or => unreachable!("filtered before evaluation"),
                    }
                }
                _ => unreachable!("filtered before evaluation"),
            };
            values[index] = Some(value);
        }
        values
            .get(root.0 as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or_else(|| "expression arena root is missing".to_string())
    })())
}

impl Value {
    pub fn to_json(&self) -> Option<serde_json::Value> {
        match self {
            Value::Json(json) => Some(json.clone()),
            Value::Str(s) => Some(serde_json::Value::String(s.clone())),
            Value::Bool(b) => Some(serde_json::Value::Bool(*b)),
            Value::Int(i) => Some(serde_json::Value::Number((*i).into())),
            Value::Num(f) => serde_json::Number::from_f64(*f).map(serde_json::Value::Number),
            Value::List(items) => Some(serde_json::Value::Array(
                items
                    .iter()
                    .map(|v| v.to_json().unwrap_or(serde_json::Value::Null))
                    .collect(),
            )),
            Value::Null => None,
            Value::HtmlElement { .. } => None,
        }
    }

    pub(crate) fn into_str(self, op: &str) -> Result<String, String> {
        match self {
            Value::Str(s) => Ok(s),
            _ => Err(format!("{}: expected string value", op)),
        }
    }

    /// Apply `f` to the inner string, propagating `Null` as `Null`.
    /// Returns `Err` if the value is neither `Str` nor `Null`.
    pub(crate) fn map_str<F>(self, op: &str, f: F) -> Result<Value, String>
    where
        F: FnOnce(String) -> Result<Value, String>,
    {
        match self {
            Value::Null => Ok(Value::Null),
            Value::Str(s) => f(s),
            _ => Err(format!("{}: expected string or null", op)),
        }
    }

    pub(crate) fn into_list(self, op: &str) -> Result<Vec<Value>, String> {
        match self {
            Value::List(v) => Ok(v),
            Value::Json(serde_json::Value::Array(arr)) => {
                Ok(arr.into_iter().map(Value::Json).collect())
            }
            _ => Err(format!("{}: expected a List", op)),
        }
    }

    pub(crate) fn into_json(self, op: &str) -> Result<serde_json::Value, String> {
        match self {
            Value::Json(v) => Ok(v),
            Value::Null => Ok(serde_json::Value::Null),
            _ => Err(format!("{}: expected Json value", op)),
        }
    }

    /// Returns `Ok(Some(StoredNode))` for an HTML element, `Ok(None)` for Null, or an error otherwise.
    pub(crate) fn into_html_element(self, op: &str) -> Result<Option<StoredNode>, String> {
        match self {
            Value::HtmlElement { doc, node_id } => Ok(Some(StoredNode { doc, node_id })),
            Value::Null => Ok(None),
            _ => Err(format!("{}: expected HTML element", op)),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Json(a), Value::Json(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            // Two HtmlElement handles stay unequal: comparing DOM nodes is a
            // question about identity that nothing here needs answered.
            _ => false,
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Str(s) => write!(f, "Str({:?})", s),
            Value::Num(n) => write!(f, "Num({})", n),
            Value::Int(i) => write!(f, "Int({})", i),
            Value::Bool(b) => write!(f, "Bool({})", b),
            Value::List(l) => write!(f, "List({:?})", l),
            Value::Null => write!(f, "Null"),
            Value::HtmlElement { node_id, .. } => write!(f, "HtmlElement({:?})", node_id),
            Value::Json(j) => write!(f, "Json({})", j),
        }
    }
}

/// Arc-backed environment: clone is O(1); mutation (set) triggers copy-on-write.
#[derive(Clone, Default)]
pub struct Env(Arc<HashMap<String, Value>>);

impl Env {
    pub fn new() -> Self {
        Env(Arc::new(HashMap::new()))
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }
    pub fn set(&mut self, key: &str, value: Value) {
        Arc::make_mut(&mut self.0).insert(key.to_owned(), value);
    }
}

/// Evaluates expression arms that are common to both the HTML and JSON evaluators.
/// Returns `Some(result)` if the arm was handled, `None` if evaluator-specific.
///
/// `recurse` takes `(&'a Expr, Env)` so callers can move `env` on the final call instead of
/// always cloning. With the Arc-backed `Env`, all other clones are O(1) regardless.
pub(super) async fn eval_common_expr<'a, F>(
    expr: &'a Expr,
    env: Env,
    recurse: &F,
    registry: Option<&'a crate::scripting::PureFunctionRegistry>,
    limits: EvalLimits,
) -> Option<Result<Value, String>>
where
    F: Fn(&'a Expr, Env) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>,
{
    match expr {
        Expr::Literal(s) => Some(Ok(Value::Str(s.clone()))),
        Expr::Number(n) => Some(Ok(Value::Num(*n))),
        Expr::Null => Some(Ok(Value::Null)),
        Expr::Bool(bool) => Some(Ok(Value::Bool(*bool))),

        Expr::Var(name) => Some(
            env.get(name)
                .cloned()
                .ok_or_else(|| format!("Undefined variable '{}'", name)),
        ),

        Expr::BinaryOperation { op, lhs, rhs } => Some(
            (async {
                if matches!(op, Op::And | Op::Or) {
                    let l = match recurse(lhs, env.clone()).await? {
                        Value::Bool(b) => b,
                        _ => return Err("&&/||: left operand must be Bool".into()),
                    };
                    return match op {
                        Op::And if !l => Ok(Value::Bool(false)),
                        Op::Or if l => Ok(Value::Bool(true)),
                        _ => match recurse(rhs, env).await? {
                            Value::Bool(b) => Ok(Value::Bool(b)),
                            _ => Err("&&/||: right operand must be Bool".into()),
                        },
                    };
                }
                let l = recurse(lhs, env.clone()).await?;
                let r = recurse(rhs, env).await?;
                match (op, l, r) {
                    (
                        Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Lt | Op::Gt | Op::Le | Op::Ge,
                        l,
                        r,
                    ) => numeric_op(op, l, r),
                    (Op::Eq, a, b) => Ok(Value::Bool(a == b)),
                    (Op::Ne, a, b) => Ok(Value::Bool(a != b)),
                    (op, l, r) => Err(format!("type error: {:?} {:?} {:?}", l, op, r)),
                }
            })
            .await,
        ),

        Expr::Split { target, delimiter } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("split", |s| {
                let parts: Vec<_> = s
                    .split(delimiter.as_str())
                    .map(|p| Value::Str(p.to_owned()))
                    .collect();
                if parts.len() > limits.max_list_size {
                    return Err(format!("limit:max_list_size:{}", limits.max_list_size));
                }
                Ok(Value::List(parts))
            })
        })),

        Expr::At { target, index } => Some(recurse(target, env).await.and_then(|v| match v {
            Value::Null => Ok(Value::Null),
            v => v.into_list("at").map(|items| {
                let i = if *index < 0 {
                    items.len().checked_sub((-*index) as usize)
                } else {
                    Some(*index as usize)
                };
                match i.and_then(|i| items.into_iter().nth(i)) {
                    Some(v) => v,
                    None => Value::Null,
                }
            }),
        })),

        Expr::Replace { target, from, to } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("replace", |s| {
                Ok(Value::Str(s.replace(from.as_str(), to.as_str())))
            })
        })),

        Expr::Trim { target } => Some(
            recurse(target, env)
                .await
                .and_then(|v| v.map_str("trim", |s| Ok(Value::Str(s.trim().to_owned())))),
        ),

        Expr::Prepend { target, prefix } => Some(
            (async {
                let s = recurse(target, env.clone()).await?;
                let p = recurse(prefix, env).await?;
                match (s, p) {
                    (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                    (s, p) => {
                        let result = p.into_str("prepend")? + s.into_str("prepend")?.as_str();
                        if result.len() > limits.max_string_length {
                            return Err(format!(
                                "limit:max_string_length:{}",
                                limits.max_string_length
                            ));
                        }
                        Ok(Value::Str(result))
                    }
                }
            })
            .await,
        ),

        Expr::Append { target, suffix } => Some(
            (async {
                let s = recurse(target, env.clone()).await?;
                let x = recurse(suffix, env).await?;
                match (s, x) {
                    (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                    (s, x) => {
                        let result = s.into_str("append")? + x.into_str("append")?.as_str();
                        if result.len() > limits.max_string_length {
                            return Err(format!(
                                "limit:max_string_length:{}",
                                limits.max_string_length
                            ));
                        }
                        Ok(Value::Str(result))
                    }
                }
            })
            .await,
        ),

        Expr::Lower { target } => Some(
            recurse(target, env)
                .await
                .and_then(|v| v.map_str("lower", |s| Ok(Value::Str(s.to_lowercase())))),
        ),

        Expr::Matches { target, pattern } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("matches", |s| {
                get_or_compile_regex(pattern).map(|re| Value::Bool(re.is_match(&s)))
            })
        })),

        Expr::Capture { target, pattern } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("capture", |s| {
                get_or_compile_regex(pattern).map(|re| match re.captures(&s) {
                    None => Value::List(vec![]),
                    Some(caps) => Value::List(
                        (0..caps.len())
                            .map(|i| {
                                caps.get(i)
                                    .map_or(Value::Null, |m| Value::Str(m.as_str().to_owned()))
                            })
                            .collect(),
                    ),
                })
            })
        })),

        Expr::ParseFloat { target } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("parse_float", |s| {
                Ok(s.parse::<f64>().map(Value::Num).unwrap_or(Value::Null))
            })
        })),

        Expr::ParseInt { target } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("parse_int", |s| {
                Ok(s.parse::<i64>().map(Value::Int).unwrap_or(Value::Null))
            })
        })),

        Expr::DateParse { target, format } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("date_parse", |date| {
                Ok(parse_date_flexible(&date, format)
                    .map(Value::Int)
                    .unwrap_or(Value::Null))
            })
        })),

        Expr::DateParseRfc3339 { target } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("date_parse_rfc3339", |date| {
                time::OffsetDateTime::parse(&date, &time::format_description::well_known::Rfc3339)
                    .map(|dt| Value::Int(dt.unix_timestamp()))
                    .map_err(|e| format!("Invalid RFC3339 date '{}': {}", date, e))
            })
        })),

        Expr::Fallback { target, default } => Some(
            (async {
                match recurse(target, env.clone()).await? {
                    Value::Null => recurse(default, env).await,
                    Value::Str(s) if s.is_empty() => recurse(default, env).await,
                    v => Ok(v),
                }
            })
            .await,
        ),

        Expr::Lookup { target, table } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("lookup", |s| {
                Ok(table
                    .iter()
                    .find(|(k, _)| s == *k)
                    .map(|(_, v)| Value::Str(v.clone()))
                    .unwrap_or(Value::Null))
            })
        })),

        Expr::StartsWith { target, prefix } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("starts_with", |s| {
                Ok(Value::Bool(s.starts_with(prefix.as_str())))
            })
        })),

        Expr::EndsWith { target, suffix } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("ends_with", |s| {
                Ok(Value::Bool(s.ends_with(suffix.as_str())))
            })
        })),

        Expr::Slice { target, start, end } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("slice", |s| {
                let len = s.chars().count() as i32;
                let resolve = |i: i32| {
                    if i < 0 {
                        (len + i).max(0) as usize
                    } else {
                        (i as usize).min(len as usize)
                    }
                };
                let s_idx = resolve(*start);
                let e_idx = end.map_or(len as usize, resolve);
                Ok(Value::Str(if s_idx >= e_idx {
                    String::new()
                } else {
                    s.chars().skip(s_idx).take(e_idx - s_idx).collect()
                }))
            })
        })),

        Expr::ResolveUrl { target, base } => Some(
            (async {
                let path = recurse(target, env.clone())
                    .await
                    .and_then(|v| v.into_str("resolve_url"))?;
                let base_url = recurse(base, env)
                    .await
                    .and_then(|v| v.into_str("resolve_url"))?;
                url::Url::parse(&base_url)
                    .and_then(|b| b.join(&path))
                    .map(|u| Value::Str(u.to_string()))
                    .map_err(|e| format!("resolve_url: {}", e))
            })
            .await,
        ),

        Expr::Concat(exprs) => Some(
            (async {
                let mut s = String::new();
                for e in exprs {
                    s.push_str(
                        &recurse(e, env.clone())
                            .await
                            .and_then(|v| v.into_str("concat"))?,
                    );
                    if s.len() > limits.max_string_length {
                        return Err(format!(
                            "limit:max_string_length:{}",
                            limits.max_string_length
                        ));
                    }
                }
                Ok(Value::Str(s))
            })
            .await,
        ),

        Expr::List(exprs) => Some(
            (async {
                let mut items = Vec::with_capacity(exprs.len());
                for e in exprs {
                    items.push(recurse(e, env.clone()).await?);
                }
                Ok(Value::List(items))
            })
            .await,
        ),

        Expr::Map { target, transform } => Some(
            (async {
                let items = recurse(target, env.clone())
                    .await
                    .and_then(|v| v.into_list("map"))?;
                if items.len() > limits.max_list_size {
                    return Err(format!("limit:max_list_size:{}", limits.max_list_size));
                }
                let mut results = Vec::with_capacity(items.len());
                for (i, item) in items.into_iter().enumerate() {
                    let mut inner = env.clone();
                    inner.set("$item", item);
                    inner.set("$index", Value::Int(i as i64));
                    match recurse(transform, inner).await? {
                        Value::Null => {}
                        v => results.push(v),
                    }
                }
                Ok(Value::List(results))
            })
            .await,
        ),

        Expr::FlatMap { target, transform } => Some(
            (async {
                let items = recurse(target, env.clone())
                    .await
                    .and_then(|v| v.into_list("flat_map"))?;
                if items.len() > limits.max_list_size {
                    return Err(format!("limit:max_list_size:{}", limits.max_list_size));
                }
                let mut results = Vec::new();
                for (i, item) in items.into_iter().enumerate() {
                    let mut inner = env.clone();
                    inner.set("$item", item);
                    inner.set("$index", Value::Int(i as i64));
                    match recurse(transform, inner).await? {
                        Value::List(inner) => results.extend(inner),
                        _ => return Err("flat_map: body must return a List".into()),
                    }
                }
                Ok(Value::List(results))
            })
            .await,
        ),

        Expr::Filter { target, filter } => Some(
            (async {
                let items = recurse(target, env.clone())
                    .await
                    .and_then(|v| v.into_list("filter"))?;
                if items.len() > limits.max_list_size {
                    return Err(format!("limit:max_list_size:{}", limits.max_list_size));
                }
                let mut results = Vec::new();
                for (i, item) in items.into_iter().enumerate() {
                    let mut inner = env.clone();
                    inner.set("$item", item.clone());
                    inner.set("$index", Value::Int(i as i64));
                    match recurse(filter, inner).await? {
                        Value::Bool(true) => results.push(item),
                        Value::Bool(false) | Value::Null => {}
                        _ => return Err("filter: predicate must return Bool".into()),
                    }
                }
                Ok(Value::List(results))
            })
            .await,
        ),

        Expr::Fold {
            target,
            transform,
            base,
        } => Some(
            (async {
                let items = recurse(target, env.clone())
                    .await
                    .and_then(|v| v.into_list("fold"))?;
                if items.len() > limits.max_list_size {
                    return Err(format!("limit:max_list_size:{}", limits.max_list_size));
                }
                let mut acc = recurse(base, env.clone()).await?;
                for (i, item) in items.into_iter().enumerate() {
                    let mut inner = env.clone();
                    inner.set("$acc", acc);
                    inner.set("$item", item);
                    inner.set("$index", Value::Int(i as i64));
                    acc = recurse(transform, inner).await?;
                }
                Ok(acc)
            })
            .await,
        ),

        Expr::Let { name, value, body } => Some(
            (async {
                let v = recurse(value, env.clone()).await?;
                let mut inner = env;
                inner.set(name, v);
                recurse(body, inner).await
            })
            .await,
        ),

        Expr::If {
            condition,
            then,
            else_,
        } => Some(
            (async {
                match recurse(condition, env.clone()).await? {
                    Value::Bool(true) => recurse(then, env).await,
                    Value::Bool(false) | Value::Null => recurse(else_, env).await,
                    _ => Err("if: condition must be Bool".into()),
                }
            })
            .await,
        ),

        Expr::ToString { target } => Some(
            (async {
                match recurse(target, env).await? {
                    Value::Str(s) => Ok(Value::Str(s)),
                    Value::Int(i) => Ok(Value::Str(i.to_string())),
                    Value::Num(f) => Ok(Value::Str(if f.fract() == 0.0 && f.abs() < 1e15 {
                        (f as i64).to_string()
                    } else {
                        f.to_string()
                    })),
                    Value::Bool(b) => Ok(Value::Str(if b { "true" } else { "false" }.to_owned())),
                    Value::Null => Ok(Value::Null),
                    other => Err(format!("to_string: cannot convert {:?}", other)),
                }
            })
            .await,
        ),

        Expr::Join { target, delimiter } => Some(
            recurse(target, env)
                .await
                .and_then(|v| v.into_list("join"))
                .and_then(|items| {
                    if items.len() > limits.max_list_size {
                        return Err(format!("limit:max_list_size:{}", limits.max_list_size));
                    }
                    items
                        .into_iter()
                        .filter(|v| !matches!(v, Value::Null))
                        .map(|v| v.into_str("join"))
                        .collect::<Result<Vec<_>, _>>()
                        .and_then(|parts| {
                            let result = parts.join(delimiter);
                            if result.len() > limits.max_string_length {
                                Err(format!(
                                    "limit:max_string_length:{}",
                                    limits.max_string_length
                                ))
                            } else {
                                Ok(Value::Str(result))
                            }
                        })
                }),
        ),

        Expr::Merge(lists) => Some(
            (async {
                let mut merged = Vec::new();
                for list_expr in lists {
                    let items = recurse(list_expr, env.clone())
                        .await
                        .and_then(|v| v.into_list("merge"))?;
                    merged.extend(items);
                }
                Ok(Value::List(merged))
            })
            .await,
        ),

        Expr::Pref(key) => Some(Ok(env
            .get(&format!("$pref:{}", key))
            .cloned()
            .unwrap_or(Value::Null))),

        Expr::Format { template, args } => Some(
            (async {
                let mut resolved = Vec::with_capacity(args.len());
                for arg in args {
                    resolved.push(
                        recurse(arg, env.clone())
                            .await
                            .and_then(|v| v.into_str("format"))?,
                    );
                }
                let mut result = template.clone();
                for val in resolved {
                    if let Some(pos) = result.find("{}") {
                        result = format!("{}{}{}", &result[..pos], val, &result[pos + 2..]);
                    }
                }
                Ok(Value::Str(result))
            })
            .await,
        ),

        Expr::Not { target } => Some(recurse(target, env).await.and_then(|v| match v {
            Value::Bool(b) => Ok(Value::Bool(!b)),
            Value::Null => Ok(Value::Bool(true)),
            other => Err(format!("not: expected Bool, got {:?}", other)),
        })),

        Expr::StringLen { target } => {
            Some(recurse(target, env).await.and_then(|v| {
                v.map_str("string_len", |s| Ok(Value::Int(s.chars().count() as i64)))
            }))
        }

        Expr::SplitN {
            target,
            delimiter,
            n,
        } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("split_n", |s| {
                Ok(Value::List(
                    s.splitn(*n, delimiter.as_str())
                        .map(|p| Value::Str(p.to_owned()))
                        .collect(),
                ))
            })
        })),

        Expr::Take { target, n } => Some(recurse(target, env).await.and_then(|v| {
            v.into_list("take")
                .map(|items| Value::List(items.into_iter().take(*n).collect()))
        })),

        Expr::Skip { target, n } => Some(recurse(target, env).await.and_then(|v| {
            v.into_list("skip")
                .map(|items| Value::List(items.into_iter().skip(*n).collect()))
        })),

        Expr::Reverse { target } => Some(recurse(target, env).await.and_then(|v| {
            v.into_list("reverse").map(|mut items| {
                items.reverse();
                Value::List(items)
            })
        })),

        Expr::SortBy { target, key } => Some(
            (async {
                let items = recurse(target, env.clone())
                    .await
                    .and_then(|v| v.into_list("sort_by"))?;
                let mut pairs: Vec<(Value, Value)> = Vec::with_capacity(items.len());
                for item in items {
                    let mut inner = env.clone();
                    inner.set("$item", item.clone());
                    let k = recurse(key, inner).await?;
                    pairs.push((k, item));
                }
                pairs.sort_by(|(a, _), (b, _)| compare_sort_key(a, b));
                Ok(Value::List(pairs.into_iter().map(|(_, v)| v).collect()))
            })
            .await,
        ),

        Expr::Unique { target } => Some(recurse(target, env).await.and_then(|v| {
            v.into_list("unique").map(|items| {
                let mut seen: Vec<Value> = Vec::new();
                let mut result = Vec::new();
                for item in items {
                    if !seen.iter().any(|s| s == &item) {
                        seen.push(item.clone());
                        result.push(item);
                    }
                }
                Value::List(result)
            })
        })),

        Expr::UrlEncode { target } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("url_encode", |s| {
                Ok(Value::Str(urlencoding::encode(&s).into_owned()))
            })
        })),

        Expr::UrlDecode { target } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("url_decode", |s| {
                Ok(Value::Str(
                    urlencoding::decode(&s).map(|c| c.into_owned()).unwrap_or(s),
                ))
            })
        })),

        Expr::FormatPadded {
            target,
            width,
            fill,
            align,
        } => Some(recurse(target, env).await.and_then(|v| {
            v.map_str("format_padded", |s| {
                let char_count = s.chars().count();
                if char_count >= *width {
                    return Ok(Value::Str(s));
                }
                let pad = width - char_count;
                let fill_str: String = std::iter::repeat_n(*fill, pad).collect();
                let result = match align {
                    PadAlign::Left => format!("{s}{fill_str}"),
                    PadAlign::Right => format!("{fill_str}{s}"),
                    PadAlign::Center => {
                        let left = pad / 2;
                        let right = pad - left;
                        let lpad: String = std::iter::repeat_n(*fill, left).collect();
                        let rpad: String = std::iter::repeat_n(*fill, right).collect();
                        format!("{lpad}{s}{rpad}")
                    }
                };
                Ok(Value::Str(result))
            })
        })),

        Expr::ScalarOverride { name } => Some(Ok(env
            .get(&format!("$scalar:{}", name))
            .cloned()
            .unwrap_or(Value::Null))),

        Expr::EncodedField {
            subfields,
            delimiter,
            encoding,
        } => Some(
            (async {
                let mut parts: Vec<String> = Vec::with_capacity(subfields.len());
                for (_, expr) in subfields {
                    let val = recurse(expr, env.clone()).await?;
                    let s = match val {
                        Value::Str(s) => s,
                        Value::Null => String::new(),
                        other => other.into_str("encoded_field")?,
                    };
                    parts.push(s);
                }
                let part_refs: Vec<&str> = parts.iter().map(String::as_str).collect();
                crate::evaluator::id_encoding::encode_composite(&part_refs, delimiter, encoding)
                    .map(Value::Str)
                    .map_err(|e| format!("encoded_field: {e}"))
            })
            .await,
        ),

        Expr::UserFn { name, args } => Some(
            (async {
                let Some(reg) = registry else {
                    return Err(format!(
                        "UserFn '{}': no script registry available for this source",
                        name
                    ));
                };
                let mut evaluated = Vec::with_capacity(args.len());
                for arg in args {
                    evaluated.push(recurse(arg, env.clone()).await?);
                }
                reg.call(name, &evaluated)
            })
            .await,
        ),

        _ => None,
    }
}

fn compare_sort_key(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Num(x), Value::Num(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Int(x), Value::Num(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Num(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Null, Value::Null) => Ordering::Equal,
        (
            Value::Null | Value::HtmlElement { .. } | Value::Json(_) | Value::List(_),
            Value::Int(_) | Value::Num(_) | Value::Str(_) | Value::Bool(_),
        ) => Ordering::Greater,
        (
            Value::Int(_) | Value::Num(_) | Value::Str(_) | Value::Bool(_),
            Value::Null | Value::HtmlElement { .. } | Value::Json(_) | Value::List(_),
        ) => Ordering::Less,
        _ => Ordering::Equal,
    }
}

/// Fetch a URL and return the response body as a String.
pub async fn fetch_body(
    state: &mut crate::wasm::HostState,
    req: &kani_shared::ast::RequestDef,
) -> Result<String, String> {
    use crate::scripting::{HookActionKind, ScriptableCtx, ScriptableRequest, ScriptableResponse};

    let hook_registry = state.hook_registry.clone();

    let mut working = ScriptableRequest {
        method: req.method.clone(),
        url: req.url.clone(),
        headers: req.headers.clone(),
        queries: req.queries.clone(),
        body: None,
        endpoint_id: req.endpoint_id.clone(),
    };

    let max_hook_retries = state.max_hook_requests;
    let mut hook_retries = 0u32;

    loop {
        if let Some(ref registry) = hook_registry {
            let ctx = ScriptableCtx {
                cache_backend: Arc::clone(&state.ext_cache),
                cache_namespace: state.ext_cache_namespace.clone(),
                prefs: state.preferences.clone(),
                v8_process: Some(state.v8_process.clone()),
                http: Some(state.http_client.clone()),
                browser_scripts: state.browser_scripts.clone(),
                browser_profile_key: Some(state.browser_profile_key.clone()),
                allowed_host: state.allowed_host.clone(),
            };
            let action = registry
                .run_pre_request(&mut working, ctx)
                .map_err(|e| format!("pre_request hook: {e}"))?;
            if let HookActionKind::Fail { kind, reason } = action.kind {
                return Err(format!("hook rejected request: {kind}: {reason}"));
            }
        }

        let mut url = url::Url::parse(&working.url).map_err(|e| format!("Invalid URL: {}", e))?;
        if !working.queries.is_empty() {
            url.query_pairs_mut().extend_pairs(working.queries.iter());
        }

        state.check_allowed_host(url.host_str().unwrap_or(""))?;
        state.charge_io()?;

        let method = match working.method.to_uppercase().as_str() {
            "GET" => rquest::Method::GET,
            "POST" => rquest::Method::POST,
            "PUT" => rquest::Method::PUT,
            "DELETE" => rquest::Method::DELETE,
            m => return Err(format!("Unsupported HTTP method: {}", m)),
        };

        let mut builder = state.http_client.inner().request(method, url.to_string());
        for (k, v) in &working.headers {
            builder = builder.header(k, v);
        }
        let request = builder.build().map_err(|e| e.to_string())?;

        let ttfb_start = std::time::Instant::now();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(90),
            state.http_client.send_request(request),
        )
        .await
        .map_err(|_| "HTTP request timed out after 90 seconds".to_string())?
        .map_err(|e| e.to_string())?;
        let ttfb = ttfb_start.elapsed();
        state.last_io_at = Some(std::time::Instant::now());

        let status = response.status();
        if crate::HTTP_LOGGING_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!(
                ttfb_ms = ttfb.as_millis(),
                status_code = status.as_u16(),
                url = url.to_string(),
                "fetch_body: connection established"
            );
        }

        // `Retry-After`, read before the response is consumed, so a 429 can
        // carry the server's own wait time to the retry policy.
        let retry_after = response
            .headers()
            .get(rquest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u32>().ok());

        // Whether an on_status hook explicitly accepted this status. A source
        // with hooks has opted into its own status handling, so the typed-error
        // default below defers to it.
        let mut proceeded = false;

        const MAX_BYTES: usize = 15 * 1024 * 1024;
        let resp_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let raw_body = response
            .bytes_limited(MAX_BYTES)
            .await
            .map_err(|e| e.to_string())?
            .to_vec();

        // Only a source with hooks sees a lossily-decoded body; without them the
        // strict conversion below still rejects a non-UTF-8 payload.
        let mut hook_body: Option<String> = None;

        if let Some(ref registry) = hook_registry {
            let mut scriptable_resp = ScriptableResponse {
                status: status.as_u16() as i64,
                headers: resp_headers,
                body: String::from_utf8_lossy(&raw_body).into_owned(),
            };
            let ctx = ScriptableCtx {
                cache_backend: Arc::clone(&state.ext_cache),
                cache_namespace: state.ext_cache_namespace.clone(),
                prefs: state.preferences.clone(),
                v8_process: Some(state.v8_process.clone()),
                http: Some(state.http_client.clone()),
                browser_scripts: state.browser_scripts.clone(),
                browser_profile_key: Some(state.browser_profile_key.clone()),
                allowed_host: state.allowed_host.clone(),
            };
            let action = registry
                .run_on_status(&working, &mut scriptable_resp, ctx)
                .map_err(|e| format!("on_status hook: {e}"))?;
            hook_body = Some(scriptable_resp.body);

            match action.kind {
                HookActionKind::Proceed => {
                    proceeded = true;
                }
                HookActionKind::Retry if hook_retries < max_hook_retries => {
                    hook_retries += 1;
                    continue;
                }
                HookActionKind::RetryAfter { seconds } if hook_retries < max_hook_retries => {
                    hook_retries += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(seconds.max(0) as u64)).await;
                    continue;
                }
                HookActionKind::Fail { kind, reason } => {
                    return Err(format!("hook: {kind}: {reason}"));
                }
                HookActionKind::RefreshAuth { endpoint_id } => {
                    return Err(format!("__refresh_auth__:{endpoint_id}"));
                }
                _ => {}
            }
        }

        let code = status.as_u16();
        if !proceeded && (code == 429 || code == 401 || code == 403 || (500..600).contains(&code)) {
            let ra = retry_after.map(|s| s.to_string()).unwrap_or_default();
            return Err(format!("{HTTP_STATUS_ERR_PREFIX}{code}:{ra}"));
        }

        return match hook_body {
            Some(body) => Ok(body),
            None => String::from_utf8(raw_body)
                .map_err(|_| "Invalid UTF-8 in response body".to_string()),
        };
    }
}

fn expr_has_fetch(expr: &kani_shared::ast::Expr) -> bool {
    use kani_shared::ast::Expr;
    match expr {
        Expr::Fetch { .. } => true,
        Expr::Attr { target, .. }
        | Expr::Text { target }
        | Expr::InnerHtml { target }
        | Expr::First { target, .. }
        | Expr::Select { target, .. }
        | Expr::HasClass { target, .. }
        | Expr::Children { target }
        | Expr::Split { target, .. }
        | Expr::At { target, .. }
        | Expr::Replace { target, .. }
        | Expr::Trim { target }
        | Expr::Prepend { target, .. }
        | Expr::Append { target, .. }
        | Expr::Lower { target }
        | Expr::Matches { target, .. }
        | Expr::Capture { target, .. }
        | Expr::ParseFloat { target }
        | Expr::Not { target }
        | Expr::StringLen { target }
        | Expr::JsonPtr { target, .. }
        | Expr::JsonStr { target }
        | Expr::JsonInt { target }
        | Expr::JsonFloat { target }
        | Expr::JsonBool { target }
        | Expr::ArrayLen { target }
        | Expr::JsonKeys { target }
        | Expr::JsonFold { target }
        | Expr::Join { target, .. } => expr_has_fetch(target),
        Expr::Map { target, transform } | Expr::FlatMap { target, transform } => {
            expr_has_fetch(target) || expr_has_fetch(transform)
        }
        Expr::Filter { target, filter } => expr_has_fetch(target) || expr_has_fetch(filter),
        Expr::Fold {
            target,
            base,
            transform,
        } => expr_has_fetch(target) || expr_has_fetch(base) || expr_has_fetch(transform),
        Expr::ResolveUrl { target, base } => expr_has_fetch(target) || expr_has_fetch(base),
        Expr::BinaryOperation { lhs, rhs, .. } => expr_has_fetch(lhs) || expr_has_fetch(rhs),
        Expr::Let { value, body, .. } => expr_has_fetch(value) || expr_has_fetch(body),
        Expr::If {
            condition,
            then,
            else_,
        } => expr_has_fetch(condition) || expr_has_fetch(then) || expr_has_fetch(else_),
        Expr::JsonGet { target, key } => expr_has_fetch(target) || expr_has_fetch(key),
        Expr::JsonFind { target, key, value } => {
            expr_has_fetch(target) || expr_has_fetch(key) || expr_has_fetch(value)
        }
        Expr::Format { args, .. } => args.iter().any(expr_has_fetch),
        Expr::Concat(parts) | Expr::List(parts) | Expr::JsonArray(parts) | Expr::Merge(parts) => {
            parts.iter().any(expr_has_fetch)
        }
        _ => false,
    }
}

pub(super) fn blueprint_has_fetch(bp: &kani_shared::ast::Blueprint) -> bool {
    bp.fields.iter().any(|f| expr_has_fetch(&f.expr))
        || bp.scalars.iter().any(|f| expr_has_fetch(&f.expr))
        || bp.bindings.iter().any(|b| expr_has_fetch(&b.expr))
}

/// Charges the io/host-allowlist budget and builds a `RequestDef` without sending it. Only
/// valid when `state.hook_registry` is `None`, since hooks can retry/rewrite a request after
/// seeing the response — see [`send_prepared_request`].
pub(super) fn charge_fetch_request(
    state: &mut crate::wasm::HostState,
    url: &str,
    method: &kani_shared::ast::HttpMethod,
    headers: Vec<(String, String)>,
    endpoint_id: Option<String>,
) -> Result<kani_shared::ast::RequestDef, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;
    state.check_allowed_host(parsed.host_str().unwrap_or(""))?;
    state.charge_io()?;

    let method_str = match method {
        kani_shared::ast::HttpMethod::Get => "GET",
        kani_shared::ast::HttpMethod::Post => "POST",
        kani_shared::ast::HttpMethod::Put => "PUT",
        kani_shared::ast::HttpMethod::Delete => "DELETE",
    };
    Ok(kani_shared::ast::RequestDef {
        url: url.to_string(),
        method: method_str.to_string(),
        headers,
        queries: vec![],
        endpoint_id,
    })
}

/// Sends a request prepared by [`charge_fetch_request`] and reads its body. Takes an owned,
/// `Clone`-cheap `SmartClient` instead of `&mut HostState` so callers can run several of these
/// concurrently; the per-domain rate limiter/semaphore in `SmartClient::send_request` still
/// applies. Mirrors `fetch_body`'s no-hooks path exactly.
pub(super) async fn send_prepared_request(
    client: crate::http::SmartClient,
    req: kani_shared::ast::RequestDef,
) -> Result<String, String> {
    let method = match req.method.to_uppercase().as_str() {
        "GET" => rquest::Method::GET,
        "POST" => rquest::Method::POST,
        "PUT" => rquest::Method::PUT,
        "DELETE" => rquest::Method::DELETE,
        m => return Err(format!("Unsupported HTTP method: {}", m)),
    };

    let mut url = url::Url::parse(&req.url).map_err(|e| format!("Invalid URL: {}", e))?;
    if !req.queries.is_empty() {
        url.query_pairs_mut().extend_pairs(req.queries.iter());
    }
    let mut builder = client.inner().request(method, url.to_string());
    for (k, v) in &req.headers {
        builder = builder.header(k, v);
    }
    let request = builder.build().map_err(|e| e.to_string())?;

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        client.send_request(request),
    )
    .await
    .map_err(|_| "HTTP request timed out after 90 seconds".to_string())?
    .map_err(|e| e.to_string())?;

    const MAX_BYTES: usize = 15 * 1024 * 1024;
    let body = response
        .bytes_limited(MAX_BYTES)
        .await
        .map_err(|e| e.to_string())?
        .to_vec();
    String::from_utf8(body).map_err(|_| "Invalid UTF-8 in response body".to_string())
}

pub(super) async fn eval_fetch_field(
    state: &mut crate::wasm::HostState,
    url: &str,
    method: &kani_shared::ast::HttpMethod,
    headers: Vec<(String, String)>,
    sub_blueprint: &kani_shared::ast::Blueprint,
    kind: &kani_shared::ast::SubBlueprintKind,
    endpoint_id: Option<String>,
) -> Result<Value, String> {
    if blueprint_has_fetch(sub_blueprint) {
        return Err("Nested Expr::Fetch inside a sub-blueprint is not allowed".into());
    }

    let method_str = match method {
        kani_shared::ast::HttpMethod::Get => "GET",
        kani_shared::ast::HttpMethod::Post => "POST",
        kani_shared::ast::HttpMethod::Put => "PUT",
        kani_shared::ast::HttpMethod::Delete => "DELETE",
    };

    let req = kani_shared::ast::RequestDef {
        url: url.to_string(),
        method: method_str.to_string(),
        headers,
        queries: vec![],
        endpoint_id,
    };

    let body = fetch_body(state, &req).await?;

    let result = match kind {
        kani_shared::ast::SubBlueprintKind::Html => {
            Box::pin(crate::evaluator::html_eval::extract_html_str(
                state,
                &body,
                sub_blueprint,
            ))
            .await?
        }
        kani_shared::ast::SubBlueprintKind::Json => {
            Box::pin(crate::evaluator::json_eval::extract_json_str(
                state,
                &body,
                sub_blueprint,
            ))
            .await?
        }
    };
    let first = result["rows"].as_array().and_then(|a| a.first()).cloned();
    Ok(first.map(Value::Json).unwrap_or(Value::Null))
}

fn get_or_compile_regex(pattern: &str) -> Result<regex::Regex, String> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, regex::Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().map_err(|_| "regex cache lock poisoned")?;
    if let Some(re) = guard.get(pattern) {
        return Ok(re.clone());
    }
    let re =
        regex::Regex::new(pattern).map_err(|e| format!("Invalid regex '{}': {}", pattern, e))?;
    guard.insert(pattern.to_owned(), re.clone());
    Ok(re)
}

fn to_numeric(v: Value) -> Result<(f64, bool), String> {
    match v {
        Value::Int(i) => Ok((i as f64, true)),
        Value::Num(f) => Ok((f, false)),
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

fn as_numeric(l: Value, r: Value) -> Result<(f64, f64, bool), String> {
    let (a, ai) = to_numeric(l)?;
    let (b, bi) = to_numeric(r)?;
    Ok((a, b, ai && bi))
}

fn numeric_op(op: &Op, l: Value, r: Value) -> Result<Value, String> {
    let (a, b, int) = as_numeric(l, r)?;
    match op {
        Op::Add => Ok(if int {
            Value::Int(a as i64 + b as i64)
        } else {
            Value::Num(a + b)
        }),
        Op::Sub => Ok(if int {
            Value::Int(a as i64 - b as i64)
        } else {
            Value::Num(a - b)
        }),
        Op::Mul => Ok(if int {
            Value::Int(a as i64 * b as i64)
        } else {
            Value::Num(a * b)
        }),
        Op::Div => {
            if b == 0.0 {
                return Err("division by zero".into());
            }
            Ok(Value::Num(a / b))
        }
        Op::Lt => Ok(Value::Bool(a < b)),
        Op::Gt => Ok(Value::Bool(a > b)),
        Op::Le => Ok(Value::Bool(a <= b)),
        Op::Ge => Ok(Value::Bool(a >= b)),
        _ => Err(format!("{:?}: not a numeric operator", op)),
    }
}

/// Inserts a field's evaluated value into a row, enforcing the string limit and
/// the field's own optionality.
///
/// A `None` value means the expression produced nothing: optional fields record
/// that as JSON null, required fields are an error naming the field. Shared by the
/// HTML and JSON evaluators so both enforce the same rule.
pub(super) fn insert_field_value(
    row: &mut serde_json::Map<String, serde_json::Value>,
    field_name: &str,
    optional: bool,
    val: Value,
    max_string_length: usize,
) -> Result<(), String> {
    match val.to_json() {
        Some(v) => {
            if let serde_json::Value::String(s) = &v
                && s.len() > max_string_length
            {
                return Err(format!("limit:max_string_length:{max_string_length}"));
            }
            row.insert(field_name.to_string(), v);
            Ok(())
        }
        None if optional => {
            row.insert(field_name.to_string(), serde_json::Value::Null);
            Ok(())
        }
        None => Err(format!("Required field '{field_name}' produced null")),
    }
}

/// Coerces an evaluated header key/value pair to strings.
///
/// A header whose key or value evaluated to a non-string is a blueprint error
/// rather than something to stringify, because the coercion would be silent and
/// the resulting request would not be the one the extension described.
pub(super) fn header_pair(key: Value, value: Value) -> Result<(String, String), String> {
    match (key, value) {
        (Value::Str(k), Value::Str(v)) => Ok((k, v)),
        _ => Err("Fetch: header keys and values must be strings".into()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::fetch_body;
    use crate::scripting::{HookRegistry, HookScripts};
    use crate::wasm::{AllowedHost, HostState};
    use kani_shared::ast::RequestDef;
    use std::sync::Arc;

    fn simple_request(url: &str) -> RequestDef {
        RequestDef {
            url: url.to_string(),
            method: "GET".to_string(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        }
    }

    #[tokio::test]
    async fn pre_request_hook_disallowed_host_after_mutation_errors() {
        let mut state = HostState {
            allowed_host: AllowedHost::Restricted("example.com".to_string()),
            ..Default::default()
        };

        let scripts = HookScripts {
            pre_request: Some(r#"req.url = "https://evil.com/api"; proceed()"#.to_string()),
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        state.hook_registry = Some(Arc::new(registry));

        let req = simple_request("https://example.com/api");
        let result = fetch_body(&mut state, &req).await;
        assert!(
            result.is_err(),
            "should fail after hook mutates URL to disallowed host"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("blocked") || err.contains("not permitted") || err.contains("evil.com"),
            "error should reflect host policy: {err}"
        );
    }

    #[tokio::test]
    async fn on_status_retry_bounded_by_max_hook_requests() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let mut state = HostState {
            allowed_host: AllowedHost::Unrestricted,
            max_hook_requests: 2,
            ..Default::default()
        };

        let mut on_status = std::collections::BTreeMap::new();
        on_status.insert("401".to_string(), "retry()".to_string());
        let scripts = HookScripts {
            on_status,
            ..Default::default()
        };
        let registry = HookRegistry::compile(&scripts).unwrap();
        state.hook_registry = Some(Arc::new(registry));

        let url = format!("{}/test", server.uri());
        let req = simple_request(&url);
        let _result = fetch_body(&mut state, &req).await;

        let received = server.received_requests().await.unwrap();
        assert_eq!(
            received.len(),
            3,
            "should make initial + 2 retries = 3 total requests"
        );
    }

    #[test]
    fn eval_budget_iteration_cap() {
        use super::{EvalBudget, MAX_EVAL_ITERATIONS};
        use std::sync::Arc;
        let budget = Arc::new(EvalBudget::new());
        for _ in 0..MAX_EVAL_ITERATIONS {
            budget.charge_step().unwrap();
        }
        let err = budget.charge_step().unwrap_err();
        assert!(
            err.starts_with("limit:max_iterations:"),
            "expected limit sentinel, got: {err}"
        );
    }

    #[test]
    fn eval_budget_depth_cap() {
        use super::{EvalBudget, MAX_EVAL_DEPTH};
        use std::sync::Arc;
        let budget = Arc::new(EvalBudget::new());
        let mut guards = Vec::new();
        for _ in 0..MAX_EVAL_DEPTH {
            guards.push(budget.enter_depth().unwrap());
        }
        let err = budget.enter_depth().unwrap_err();
        assert!(
            err.starts_with("limit:max_depth:"),
            "expected depth sentinel, got: {err}"
        );
        drop(guards);
        budget.enter_depth().unwrap();
    }

    #[test]
    fn eval_budget_reset_restores_budget() {
        use super::{EvalBudget, MAX_EVAL_ITERATIONS};
        use std::sync::Arc;
        let budget = Arc::new(EvalBudget::new());
        for _ in 0..MAX_EVAL_ITERATIONS {
            budget.charge_step().unwrap();
        }
        budget.reset();
        budget.charge_step().unwrap();
    }

    #[test]
    fn flat_arena_evaluates_ten_thousand_nodes_without_depth_budget() {
        use super::{Env, EvalBudget, Value, eval_flat_arena};
        use kani_shared::ast::{ExprArena, ExprId, ExprLeaf, ExprNode, Op};
        use std::sync::Arc;

        let mut nodes = vec![ExprNode::Leaf(ExprLeaf::Number(1.0))];
        let mut root = ExprId(0);
        for _ in 0..4_999 {
            let rhs = ExprId(nodes.len() as u32);
            nodes.push(ExprNode::Leaf(ExprLeaf::Number(1.0)));
            let next = ExprId(nodes.len() as u32);
            nodes.push(ExprNode::BinaryOperation {
                op: Op::Add,
                lhs: root,
                rhs,
            });
            root = next;
        }
        let arena = ExprArena { nodes };
        let budget = Arc::new(EvalBudget::new());
        let value = eval_flat_arena(&arena, root, &Env::new(), &budget)
            .expect("supported flat expression")
            .expect("evaluation succeeds");
        assert_eq!(value, Value::Num(5_000.0));
    }

    #[tokio::test]
    async fn json_eval_depth_limit() {
        use super::MAX_EVAL_DEPTH;
        use crate::evaluator::json_eval;
        use crate::wasm::HostState;
        use kani_shared::ast::{BlueprintBuilder, Expr};

        let mut e = Expr::Json("/x".to_string());
        for _ in 0..=(MAX_EVAL_DEPTH as usize + 10) {
            e = Expr::JsonStr {
                target: Box::new(e),
            };
        }
        let bp = BlueprintBuilder::new("/items").field("val", e).build();
        let doc = serde_json::json!({ "items": [{"x": "hi"}] });
        let mut state = HostState::default();

        let result = json_eval::extract_json_str(&mut state, &doc.to_string(), &bp).await;
        assert!(result.is_err(), "deeply nested expr should hit depth limit");
        let err = result.unwrap_err();
        assert!(
            err.contains("limit:max_depth"),
            "expected depth-limit sentinel, got: {err}"
        );
    }

    #[tokio::test]
    async fn arena_structure_does_not_consume_legacy_depth_budget() {
        use crate::evaluator::json_eval;
        use crate::wasm::HostState;
        use kani_shared::ast::{
            BlueprintBuilder, Expr, ExprArena, ExprId, ExprLeaf, ExprNode, UnaryExprOp,
        };
        use std::sync::Arc;

        let mut nodes = vec![ExprNode::Leaf(ExprLeaf::Literal(" value ".into()))];
        let mut root = ExprId(0);
        for _ in 0..100 {
            let next = ExprId(nodes.len() as u32);
            nodes.push(ExprNode::Unary {
                op: UnaryExprOp::Trim,
                target: root,
            });
            root = next;
        }
        let expression = Expr::Arena {
            arena: Arc::new(ExprArena { nodes }),
            root,
        };
        let blueprint = BlueprintBuilder::new("/items")
            .field("value", expression)
            .build();
        let mut state = HostState::default();
        let result = json_eval::extract_json_str(
            &mut state,
            &serde_json::json!({ "items": [{}] }).to_string(),
            &blueprint,
        )
        .await
        .expect("arena evaluation");
        assert_eq!(result["rows"][0]["value"], "value");
    }

    #[tokio::test]
    async fn a_multi_binding_arena_preserves_control_flow_and_collections() {
        use crate::evaluator::json_eval;
        use crate::wasm::HostState;
        use kani_shared::ast::{BlueprintBuilder, Expr};

        let source = r#"let $synopsis = self.ptr("/synopsis").str().fallback("");
let $alts = if pref("alt_titles_in_description") == "true"
  then self.ptr("/altTitles").map($item.str()).join("\n").fallback("")
  else "";
let $facts = if pref("extra_info_in_description") == "true"
  then merge([
    [format("Year: {}", self.ptr("/year").int().to_string())],
    [format("Rating: {} from {} ratings",
            self.ptr("/ratedAvg").float().to_string(),
            self.ptr("/ratedCount").int().to_string())],
    [format("Followed by: {}", self.ptr("/followsTotal").int().to_string())]
  ]).join("\n")
  else "";
merge([
  [$synopsis],
  [if $alts == "" then "" else format("Alternative names:\n{}", $alts)],
  [$facts]
]).filter($item != "").join("\n\n")"#;
        let parsed = kani_yaml::dsl::parse(source).expect("parse the expression");
        let expression = Expr::try_from(parsed).expect("lower the expression");
        assert!(matches!(expression, Expr::Arena { .. }));
        let blueprint = BlueprintBuilder::new("/items")
            .field("description", expression)
            .build();
        let document = serde_json::json!({
            "items": [{
                "synopsis": "Summary",
                "altTitles": ["Alternative"],
                "year": 2024,
                "ratedAvg": 4.5,
                "ratedCount": 12,
                "followsTotal": 99
            }]
        });
        let mut state = HostState::default();
        state
            .preferences
            .insert("alt_titles_in_description".into(), "true".into());
        state
            .preferences
            .insert("extra_info_in_description".into(), "true".into());
        let result = json_eval::extract_json_str(&mut state, &document.to_string(), &blueprint)
            .await
            .expect("evaluate the arena");
        let description = result["rows"][0]["description"].as_str().unwrap();
        assert!(description.contains("Summary"));
        assert!(description.contains("Alternative names:\nAlternative"));
        assert!(description.contains("Year: 2024"));
        assert!(description.contains("Rating: 4.5 from 12 ratings"));
    }
}

#[cfg(test)]
mod field_insertion_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn row() -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::new()
    }

    #[test]
    fn a_string_at_the_limit_is_accepted_and_one_over_is_not() {
        let mut r = row();
        insert_field_value(&mut r, "t", false, Value::Str("abcd".into()), 4).unwrap();
        assert_eq!(r["t"], "abcd", "a string of exactly the limit must pass");

        insert_field_value(&mut r, "t", false, Value::Str("abc".into()), 4).unwrap();
        assert_eq!(r["t"], "abc");

        let error =
            insert_field_value(&mut r, "t", false, Value::Str("abcde".into()), 4).unwrap_err();
        assert_eq!(
            error, "limit:max_string_length:4",
            "one character over the limit must be refused, and the error carries the limit"
        );
        assert_eq!(r["t"], "abc", "a refused value must not land in the row");
    }

    #[test]
    fn the_limit_applies_to_strings_alone() {
        let mut r = row();
        // A long non-string carries no length the limit could describe.
        insert_field_value(&mut r, "n", false, Value::Int(1_234_567_890), 2).unwrap();
        assert_eq!(r["n"], 1_234_567_890_i64);
    }

    #[test]
    fn an_absent_optional_field_becomes_null_and_an_absent_required_one_is_an_error() {
        let mut r = row();
        insert_field_value(&mut r, "maybe", true, Value::Null, 100).unwrap();
        assert!(
            r.contains_key("maybe") && r["maybe"].is_null(),
            "an optional field must be present as null, not missing: {r:?}"
        );

        let error = insert_field_value(&mut r, "must", false, Value::Null, 100).unwrap_err();
        assert!(
            error.contains("must"),
            "the error must name the field that produced nothing, got: {error}"
        );
        assert!(
            !r.contains_key("must"),
            "a required field that produced nothing must not be inserted"
        );
    }

    #[test]
    fn header_pairs_must_be_strings_on_both_sides() {
        assert_eq!(
            header_pair(Value::Str("Referer".into()), Value::Str("https://x".into())).unwrap(),
            ("Referer".to_string(), "https://x".to_string())
        );

        for (key, value) in [
            (Value::Int(1), Value::Str("v".into())),
            (Value::Str("k".into()), Value::Int(1)),
            (Value::Null, Value::Null),
        ] {
            let error = header_pair(key, value).unwrap_err();
            assert!(
                error.contains("must be strings"),
                "a non-string header part must be refused rather than coerced, got: {error}"
            );
        }
    }
}
