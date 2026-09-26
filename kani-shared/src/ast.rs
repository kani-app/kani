//! Declarative extraction blueprint AST and its guest-safe builder API.

use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// Expression evaluated by the host against an HTML or JSON extraction context.
///
/// Expressions are serialized into a [`Blueprint`]; variants describe data access and pure
/// transformations except [`Expr::Fetch`], which performs a bounded sub-request.
pub enum Expr {
    /// The current iteration element (inside a container loop)
    SelfRef,

    /// Select from the document root: dom("selector")
    Dom(String),

    /// Navigate to a JSON value using a JSON Pointer
    Json(String),

    /// Reference a bound variable: $name
    Var(String),

    /// A string literal
    Literal(String),

    /// A numeric literal
    Number(f64),

    /// Null value
    Null,

    Bool(bool),

    BinaryOperation {
        op: Op,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },

    /// .attr("name") - get attribute value
    Attr {
        target: Box<Expr>,
        name: String,
    },

    /// .text() - get text content
    Text {
        target: Box<Expr>,
    },

    /// .inner_html() - get inner HTML
    InnerHtml {
        target: Box<Expr>,
    },

    /// .select("selector") - sub-select within an element
    Select {
        target: Box<Expr>,
        selector: String,
    },

    /// .first("selector") - first matching child
    First {
        target: Box<Expr>,
        selector: String,
    },

    /// `.split("delim")` splits a string, returning a `List<String>`.
    Split {
        target: Box<Expr>,
        delimiter: String,
    },

    /// .at(n) - get element at index from a List; negative indices count from the end
    At {
        target: Box<Expr>,
        index: i32,
    },

    /// .replace("from", "to")
    Replace {
        target: Box<Expr>,
        from: String,
        to: String,
    },

    /// .trim()
    Trim {
        target: Box<Expr>,
    },

    /// .prepend(expr) - prepend another expression's result
    Prepend {
        target: Box<Expr>,
        prefix: Box<Expr>,
    },

    /// .append(expr) - append another expression's result
    Append {
        target: Box<Expr>,
        suffix: Box<Expr>,
    },

    /// .to_lowercase()
    Lower {
        target: Box<Expr>,
    },

    /// .matches("regex") - test if value matches pattern
    Matches {
        target: Box<Expr>,
        pattern: String,
    },

    /// .capture("regex") - returns all capture groups from the first match as List<Str|Null>,
    /// where index 0 is the whole match and index n is capture group n.
    /// Returns an empty list if there is no match.
    Capture {
        target: Box<Expr>,
        pattern: String,
    },

    /// .parse_float() - parse string to f64
    ParseFloat {
        target: Box<Expr>,
    },

    /// .parse_int() - parse string to i64
    ParseInt {
        target: Box<Expr>,
    },

    /// .ptr("/path/to/value") - JSON Pointer navigation
    JsonPtr {
        target: Box<Expr>,
        pointer: String,
    },

    /// .str() - Extract as a string.
    JsonStr {
        target: Box<Expr>,
    },

    /// .int() - Extract as an integer.
    JsonInt {
        target: Box<Expr>,
    },

    /// .float() - Extract as a float.
    JsonFloat {
        target: Box<Expr>,
    },

    /// .bool() - Extract as a boolean.
    JsonBool {
        target: Box<Expr>,
    },

    /// .array_len() - length of JSON array at pointer
    ArrayLen {
        target: Box<Expr>,
    },

    /// .keys() - get the keys of a JSON object as a List of strings
    JsonKeys {
        target: Box<Expr>,
    },

    /// .has_class("name") - test if element has a CSS class
    HasClass {
        target: Box<Expr>,
        class: String,
    },

    /// `.children()` returns direct child elements as a `List<Element>`.
    Children {
        target: Box<Expr>,
    },

    /// .starts_with("prefix") - test if string starts with prefix
    StartsWith {
        target: Box<Expr>,
        prefix: String,
    },

    /// .ends_with("suffix") - test if string ends with suffix
    EndsWith {
        target: Box<Expr>,
        suffix: String,
    },

    /// .slice(start, end) - substring by character index (0-based, exclusive end; negatives count from end)
    Slice {
        target: Box<Expr>,
        start: i32,
        end: Option<i32>,
    },

    /// let $name = expr; continuation
    Let {
        name: String,
        value: Box<Expr>,
        body: Box<Expr>,
    },

    /// expr.fallback(default) - use default if expr is null/empty/failed
    Fallback {
        target: Box<Expr>,
        default: Box<Expr>,
    },

    /// expr.map_status({ "publishing": "ongoing", ... }) - lookup table
    Lookup {
        target: Box<Expr>,
        table: Vec<(String, String)>,
    },

    /// Iterate over a give list applying the transform to each element, producing a List
    Map {
        target: Box<Expr>,
        transform: Box<Expr>,
    },

    /// Same as map, but results are flattened into a single List result
    FlatMap {
        target: Box<Expr>,
        transform: Box<Expr>,
    },

    /// Concatenate multiple strings
    Concat(Vec<Expr>),

    /// Perform a left-fold applying transform on a list, starting with base as the base case
    Fold {
        target: Box<Expr>,
        transform: Box<Expr>,
        base: Box<Expr>,
    },

    /// Filter elements from a list based on a filter predicate
    Filter {
        target: Box<Expr>,
        filter: Box<Expr>,
    },

    /// Evaluate multiple expressions and return them as a list/array
    List(Vec<Expr>),

    /// Parse a date string with a format pattern, returning unix timestamp
    DateParse {
        target: Box<Expr>,
        format: String,
    },

    /// Parse an RFC3339 date string, returning unix timestamp
    DateParseRfc3339 {
        target: Box<Expr>,
    },

    /// Resolve a relative URL against a base
    ResolveUrl {
        target: Box<Expr>,
        base: Box<Expr>,
    },

    /// The iteration index (0-based) inside a container loop
    Index,

    /// if condition then value_if_true else value_if_false
    If {
        condition: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },

    /// .to_string() — convert Int, Float, Bool to their string representation; Null stays Null
    ToString {
        target: Box<Expr>,
    },

    /// `.join("delim")` joins a `List<String>` with a delimiter, skipping null items.
    Join {
        target: Box<Expr>,
        delimiter: String,
    },

    /// .get(key_expr) — dynamic JSON object field access by an expression-evaluated key
    JsonGet {
        target: Box<Expr>,
        key: Box<Expr>,
    },

    /// `.find(key, value)` finds the first JSON array element whose object key equals the value.
    JsonFind {
        target: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
    },

    /// json_array([a, b, ...]) — construct a JSON array from N evaluated expressions
    JsonArray(Vec<Expr>),

    /// `.json_fold()` reduces a JSON array through merge.
    ///
    /// Objects merge by key and nested arrays flatten by one level.
    JsonFold {
        target: Box<Expr>,
    },

    /// merge([list1, list2, ...]) — concatenate multiple lists into one
    Merge(Vec<Expr>),

    /// pref("key") — read an extension preference value; returns String or Null if unset
    Pref(String),

    /// format("template {}", arg1, arg2, ...) — interpolate `{}` placeholders with evaluated args
    Format {
        template: String,
        args: Vec<Expr>,
    },

    /// .not() — boolean negation; Null is treated as false (not(null) = true)
    Not {
        target: Box<Expr>,
    },

    /// .string_len() — number of Unicode characters in the string; returns Int
    StringLen {
        target: Box<Expr>,
    },

    SplitN {
        target: Box<Expr>,
        delimiter: String,
        n: usize,
    },

    Take {
        target: Box<Expr>,
        n: usize,
    },

    Skip {
        target: Box<Expr>,
        n: usize,
    },

    Reverse {
        target: Box<Expr>,
    },

    SortBy {
        target: Box<Expr>,
        key: Box<Expr>,
    },

    Unique {
        target: Box<Expr>,
    },

    UrlEncode {
        target: Box<Expr>,
    },

    UrlDecode {
        target: Box<Expr>,
    },

    FormatPadded {
        target: Box<Expr>,
        width: usize,
        fill: char,
        align: PadAlign,
    },

    ScalarOverride {
        name: String,
    },

    Fetch {
        url_expr: Box<Expr>,
        blueprint: Box<Blueprint>,
        method: HttpMethod,
        headers: Vec<(Expr, Expr)>,
        kind: SubBlueprintKind,
        on_failure: OnFailurePolicy,
        endpoint_id: Option<String>,
    },

    EncodedField {
        subfields: Vec<(String, Box<Expr>)>,
        delimiter: String,
        encoding: IdEncoding,
    },

    UserFn {
        name: String,
        args: Vec<Expr>,
    },

    /// Flat expression storage emitted by the YAML Pratt parser.
    Arena {
        arena: Arc<ExprArena>,
        root: ExprId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct ExprArena {
    pub nodes: Vec<ExprNode>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum ExprNode {
    Leaf(ExprLeaf),
    Unary {
        op: UnaryExprOp,
        target: ExprId,
    },
    BinaryOperation {
        op: Op,
        lhs: ExprId,
        rhs: ExprId,
    },
    Binary {
        op: BinaryExprOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Ternary {
        op: TernaryExprOp,
        first: ExprId,
        second: ExprId,
        third: ExprId,
    },
    Let {
        name: String,
        value: ExprId,
        body: ExprId,
    },
    Lookup {
        target: ExprId,
        table: Vec<(String, String)>,
    },
    Many {
        op: ManyExprOp,
        items: Vec<ExprId>,
    },
    Format {
        template: String,
        args: Vec<ExprId>,
    },
    UserFn {
        name: String,
        args: Vec<ExprId>,
    },
    MapLiteral(Vec<(String, String)>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum ExprLeaf {
    SelfRef,
    Dom(String),
    Json(String),
    Var(String),
    Literal(String),
    Number(f64),
    Null,
    Bool(bool),
    Index,
    Pref(String),
    ScalarOverride(String),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum UnaryExprOp {
    Attr(String),
    Text,
    InnerHtml,
    Select(String),
    First(String),
    Split(String),
    At(i32),
    Replace(String, String),
    Trim,
    Lower,
    Matches(String),
    Capture(String),
    ParseFloat,
    ParseInt,
    JsonPtr(String),
    JsonStr,
    JsonInt,
    JsonFloat,
    JsonBool,
    ArrayLen,
    JsonKeys,
    HasClass(String),
    Children,
    StartsWith(String),
    EndsWith(String),
    Slice(i32, Option<i32>),
    DateParse(String),
    DateParseRfc3339,
    ToString,
    Join(String),
    JsonFold,
    Not,
    StringLen,
    SplitN(String, usize),
    Take(usize),
    Skip(usize),
    Reverse,
    Unique,
    UrlEncode,
    UrlDecode,
    FormatPadded {
        width: usize,
        fill: char,
        align: PadAlign,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum BinaryExprOp {
    Prepend,
    Append,
    Fallback,
    Map,
    FlatMap,
    Filter,
    ResolveUrl,
    JsonGet,
    SortBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum TernaryExprOp {
    Fold,
    JsonFind,
    If,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum ManyExprOp {
    Concat,
    List,
    JsonArray,
    Merge,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// Binary operator used by [`Expr::BinaryOperation`].
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

/// HTTP method for sub-blueprint fetches.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Delete,
}

/// Whether a sub-fetch result should be parsed as HTML or JSON.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum SubBlueprintKind {
    Html,
    Json,
}

/// What to do when an `Expr::Fetch` sub-request fails.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum OnFailurePolicy {
    /// Propagate the error to the caller (default).
    #[default]
    Fail,
    /// Suppress the error and produce `Value::Null`.
    Skip,
    /// Suppress the error and evaluate a fallback expression instead.
    Use(Box<Expr>),
}

/// How the offset/page query parameter is calculated for each chunk fetch.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum OffsetType {
    /// Param value = absolute item count: 0, 32, 64, …
    ItemOffset,
    /// Param value = page number starting at `start` (typically 0 or 1)
    PageNumber { start: u32 },
    /// Cursor-based pagination: each chunk response carries the next cursor in
    /// the scalar named `next_cursor_field`. Injected as `offset_param` on the
    /// next request; stops when the scalar is absent or null.
    CursorToken { next_cursor_field: String },
}

/// Alignment direction for `Expr::FormatPadded`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum PadAlign {
    Left,
    Right,
    Center,
}

/// Encoding scheme for composite ID fields produced by `Expr::EncodedField`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum IdEncoding {
    Base64Url,
    Base64,
    Passthrough,
    Hex,
}

pub const MAX_EXPR_NODES: usize = 10_000;

impl ExprArena {
    pub fn validate(&self, root: ExprId) -> Result<(), String> {
        if self.nodes.is_empty() {
            return Err("expression arena is empty".to_string());
        }
        if self.nodes.len() > MAX_EXPR_NODES {
            return Err(format!(
                "expression arena has {} nodes; limit is {MAX_EXPR_NODES}",
                self.nodes.len()
            ));
        }
        if root.0 as usize >= self.nodes.len() {
            return Err(format!("expression root id {} is out of bounds", root.0));
        }
        for (index, node) in self.nodes.iter().enumerate() {
            for child in node.children() {
                let child = child.0 as usize;
                if child >= self.nodes.len() {
                    return Err(format!(
                        "expression node {index} references out-of-bounds child {child}"
                    ));
                }
                if child >= index {
                    return Err(format!(
                        "expression node {index} references non-topological child {child}"
                    ));
                }
            }
        }
        if matches!(self.nodes[root.0 as usize], ExprNode::MapLiteral(_)) {
            return Err("map literals can only be used by lookup expressions".to_string());
        }
        Ok(())
    }

    pub fn node_expr(self: &Arc<Self>, id: ExprId) -> Result<Expr, String> {
        let node = self
            .nodes
            .get(id.0 as usize)
            .ok_or_else(|| format!("expression node id {} is out of bounds", id.0))?;
        let child = |root| Expr::Arena {
            arena: Arc::clone(self),
            root,
        };
        Ok(match node {
            ExprNode::Leaf(leaf) => match leaf {
                ExprLeaf::SelfRef => Expr::SelfRef,
                ExprLeaf::Dom(value) => Expr::Dom(value.clone()),
                ExprLeaf::Json(value) => Expr::Json(value.clone()),
                ExprLeaf::Var(value) => Expr::Var(value.clone()),
                ExprLeaf::Literal(value) => Expr::Literal(value.clone()),
                ExprLeaf::Number(value) => Expr::Number(*value),
                ExprLeaf::Null => Expr::Null,
                ExprLeaf::Bool(value) => Expr::Bool(*value),
                ExprLeaf::Index => Expr::Index,
                ExprLeaf::Pref(value) => Expr::Pref(value.clone()),
                ExprLeaf::ScalarOverride(value) => Expr::ScalarOverride {
                    name: value.clone(),
                },
            },
            ExprNode::Unary { op, target } => {
                let target = Box::new(child(*target));
                match op {
                    UnaryExprOp::Attr(name) => Expr::Attr {
                        target,
                        name: name.clone(),
                    },
                    UnaryExprOp::Text => Expr::Text { target },
                    UnaryExprOp::InnerHtml => Expr::InnerHtml { target },
                    UnaryExprOp::Select(selector) => Expr::Select {
                        target,
                        selector: selector.clone(),
                    },
                    UnaryExprOp::First(selector) => Expr::First {
                        target,
                        selector: selector.clone(),
                    },
                    UnaryExprOp::Split(delimiter) => Expr::Split {
                        target,
                        delimiter: delimiter.clone(),
                    },
                    UnaryExprOp::At(index) => Expr::At {
                        target,
                        index: *index,
                    },
                    UnaryExprOp::Replace(from, to) => Expr::Replace {
                        target,
                        from: from.clone(),
                        to: to.clone(),
                    },
                    UnaryExprOp::Trim => Expr::Trim { target },
                    UnaryExprOp::Lower => Expr::Lower { target },
                    UnaryExprOp::Matches(pattern) => Expr::Matches {
                        target,
                        pattern: pattern.clone(),
                    },
                    UnaryExprOp::Capture(pattern) => Expr::Capture {
                        target,
                        pattern: pattern.clone(),
                    },
                    UnaryExprOp::ParseFloat => Expr::ParseFloat { target },
                    UnaryExprOp::ParseInt => Expr::ParseInt { target },
                    UnaryExprOp::JsonPtr(pointer) => Expr::JsonPtr {
                        target,
                        pointer: pointer.clone(),
                    },
                    UnaryExprOp::JsonStr => Expr::JsonStr { target },
                    UnaryExprOp::JsonInt => Expr::JsonInt { target },
                    UnaryExprOp::JsonFloat => Expr::JsonFloat { target },
                    UnaryExprOp::JsonBool => Expr::JsonBool { target },
                    UnaryExprOp::ArrayLen => Expr::ArrayLen { target },
                    UnaryExprOp::JsonKeys => Expr::JsonKeys { target },
                    UnaryExprOp::HasClass(class) => Expr::HasClass {
                        target,
                        class: class.clone(),
                    },
                    UnaryExprOp::Children => Expr::Children { target },
                    UnaryExprOp::StartsWith(prefix) => Expr::StartsWith {
                        target,
                        prefix: prefix.clone(),
                    },
                    UnaryExprOp::EndsWith(suffix) => Expr::EndsWith {
                        target,
                        suffix: suffix.clone(),
                    },
                    UnaryExprOp::Slice(start, end) => Expr::Slice {
                        target,
                        start: *start,
                        end: *end,
                    },
                    UnaryExprOp::DateParse(format) => Expr::DateParse {
                        target,
                        format: format.clone(),
                    },
                    UnaryExprOp::DateParseRfc3339 => Expr::DateParseRfc3339 { target },
                    UnaryExprOp::ToString => Expr::ToString { target },
                    UnaryExprOp::Join(delimiter) => Expr::Join {
                        target,
                        delimiter: delimiter.clone(),
                    },
                    UnaryExprOp::JsonFold => Expr::JsonFold { target },
                    UnaryExprOp::Not => Expr::Not { target },
                    UnaryExprOp::StringLen => Expr::StringLen { target },
                    UnaryExprOp::SplitN(delimiter, n) => Expr::SplitN {
                        target,
                        delimiter: delimiter.clone(),
                        n: *n,
                    },
                    UnaryExprOp::Take(n) => Expr::Take { target, n: *n },
                    UnaryExprOp::Skip(n) => Expr::Skip { target, n: *n },
                    UnaryExprOp::Reverse => Expr::Reverse { target },
                    UnaryExprOp::Unique => Expr::Unique { target },
                    UnaryExprOp::UrlEncode => Expr::UrlEncode { target },
                    UnaryExprOp::UrlDecode => Expr::UrlDecode { target },
                    UnaryExprOp::FormatPadded { width, fill, align } => Expr::FormatPadded {
                        target,
                        width: *width,
                        fill: *fill,
                        align: align.clone(),
                    },
                }
            }
            ExprNode::BinaryOperation { op, lhs, rhs } => Expr::BinaryOperation {
                op: op.clone(),
                lhs: Box::new(child(*lhs)),
                rhs: Box::new(child(*rhs)),
            },
            ExprNode::Binary { op, lhs, rhs } => {
                let lhs = Box::new(child(*lhs));
                let rhs = Box::new(child(*rhs));
                match op {
                    BinaryExprOp::Prepend => Expr::Prepend {
                        target: lhs,
                        prefix: rhs,
                    },
                    BinaryExprOp::Append => Expr::Append {
                        target: lhs,
                        suffix: rhs,
                    },
                    BinaryExprOp::Fallback => Expr::Fallback {
                        target: lhs,
                        default: rhs,
                    },
                    BinaryExprOp::Map => Expr::Map {
                        target: lhs,
                        transform: rhs,
                    },
                    BinaryExprOp::FlatMap => Expr::FlatMap {
                        target: lhs,
                        transform: rhs,
                    },
                    BinaryExprOp::Filter => Expr::Filter {
                        target: lhs,
                        filter: rhs,
                    },
                    BinaryExprOp::ResolveUrl => Expr::ResolveUrl {
                        target: lhs,
                        base: rhs,
                    },
                    BinaryExprOp::JsonGet => Expr::JsonGet {
                        target: lhs,
                        key: rhs,
                    },
                    BinaryExprOp::SortBy => Expr::SortBy {
                        target: lhs,
                        key: rhs,
                    },
                }
            }
            ExprNode::Ternary {
                op,
                first,
                second,
                third,
            } => {
                let first = Box::new(child(*first));
                let second = Box::new(child(*second));
                let third = Box::new(child(*third));
                match op {
                    TernaryExprOp::Fold => Expr::Fold {
                        target: first,
                        base: second,
                        transform: third,
                    },
                    TernaryExprOp::JsonFind => Expr::JsonFind {
                        target: first,
                        key: second,
                        value: third,
                    },
                    TernaryExprOp::If => Expr::If {
                        condition: first,
                        then: second,
                        else_: third,
                    },
                }
            }
            ExprNode::Let { name, value, body } => Expr::Let {
                name: name.clone(),
                value: Box::new(child(*value)),
                body: Box::new(child(*body)),
            },
            ExprNode::Lookup { target, table } => Expr::Lookup {
                target: Box::new(child(*target)),
                table: table.clone(),
            },
            ExprNode::Many { op, items } => {
                let values = items.iter().copied().map(child).collect();
                match op {
                    ManyExprOp::Concat => Expr::Concat(values),
                    ManyExprOp::List => Expr::List(values),
                    ManyExprOp::JsonArray => Expr::JsonArray(values),
                    ManyExprOp::Merge => Expr::Merge(values),
                }
            }
            ExprNode::Format { template, args } => Expr::Format {
                template: template.clone(),
                args: args.iter().copied().map(child).collect(),
            },
            ExprNode::UserFn { name, args } => Expr::UserFn {
                name: name.clone(),
                args: args.iter().copied().map(child).collect(),
            },
            ExprNode::MapLiteral(_) => {
                return Err("map literal cannot be evaluated as an expression".to_string());
            }
        })
    }
}

impl ExprNode {
    pub fn children(&self) -> Vec<ExprId> {
        match self {
            Self::Leaf(_) | Self::MapLiteral(_) => Vec::new(),
            Self::Unary { target, .. } | Self::Lookup { target, .. } => vec![*target],
            Self::BinaryOperation { lhs, rhs, .. } | Self::Binary { lhs, rhs, .. } => {
                vec![*lhs, *rhs]
            }
            Self::Ternary {
                first,
                second,
                third,
                ..
            } => vec![*first, *second, *third],
            Self::Let { value, body, .. } => vec![*value, *body],
            Self::Many { items, .. }
            | Self::Format { args: items, .. }
            | Self::UserFn { args: items, .. } => items.clone(),
        }
    }
}

/// Current serialized blueprint schema understood by codegen and the host evaluator.
pub const DSL_SCHEMA_VERSION: u32 = 6;

/// Oldest blueprint schema the host still reads. Within 1.x a version bump only appends enum
/// variants, so every version from this one up to [`DSL_SCHEMA_VERSION`] stays readable.
pub const MIN_READABLE_DSL_SCHEMA_VERSION: u32 = 5;

pub fn is_readable_dsl_schema_version(version: u32) -> bool {
    (MIN_READABLE_DSL_SCHEMA_VERSION..=DSL_SCHEMA_VERSION).contains(&version)
}

/// Declares that a source paginates in fixed-size chunks, so the framework can
/// handle the offset algebra instead of each extension doing it manually.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct PaginationConfig {
    /// How many items the source actually returns per chunk (its real page size)
    pub native_page_size: usize,
    /// Query parameter name the source uses for the offset/page (e.g. "offset", "page")
    pub offset_param: String,
    pub offset_type: OffsetType,
}

/// A complete extraction blueprint sent across the FFI boundary.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
#[must_use = "a Blueprint is built to be evaluated or serialized; discarding it does nothing"]
pub struct Blueprint {
    pub request: Option<RequestDef>,

    /// CSS selector (HTML) or JSON Pointer (JSON) for the repeating container
    pub container: String,

    pub fields: Vec<FieldDef>,

    /// Variables bound before iteration (e.g., from document-level selectors)
    pub bindings: Vec<Binding>,

    /// Document-level fields evaluated once (not per-element); returned in output alongside rows
    pub scalars: Vec<FieldDef>,

    /// When set, `paginated-extract-html` handles multi-chunk fetching automatically
    pub pagination: Option<PaginationConfig>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// Named expression emitted as one field in each extraction row or scalar result.
pub struct FieldDef {
    pub name: String,

    pub expr: Expr,

    /// Whether this field is optional (null allowed in output)
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// Named expression evaluated before row iteration and made available to later expressions.
pub struct Binding {
    pub name: String,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// Upstream request optionally carried inside a serialized [`Blueprint`].
pub struct RequestDef {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub queries: Vec<(String, String)>,
    #[cfg_attr(any(feature = "host", feature = "builder"), serde(default))]
    pub endpoint_id: Option<String>,
}

#[cfg(feature = "builder")]
#[allow(clippy::should_implement_trait)]
impl Expr {
    #[inline]
    pub fn arena_from_bytes(bytes: &[u8]) -> Self {
        let (arena, root): (ExprArena, ExprId) =
            postcard::from_bytes(bytes).expect("generated expression arena is invalid");
        arena
            .validate(root)
            .expect("generated expression arena failed validation");
        Self::Arena {
            arena: Arc::new(arena),
            root,
        }
    }

    #[inline]
    pub fn self_ref() -> Self {
        Expr::SelfRef
    }
    #[inline]
    pub fn index() -> Self {
        Expr::Index
    }
    #[inline]
    pub fn null() -> Self {
        Expr::Null
    }
    #[inline]
    pub fn true_val() -> Self {
        Expr::Bool(true)
    }
    #[inline]
    pub fn false_val() -> Self {
        Expr::Bool(false)
    }
    #[inline]
    pub fn lit(s: impl Into<String>) -> Self {
        Expr::Literal(s.into())
    }
    #[inline]
    pub fn num(n: f64) -> Self {
        Expr::Number(n)
    }
    #[inline]
    pub fn var(name: impl Into<String>) -> Self {
        Expr::Var(name.into())
    }
    #[inline]
    pub fn bool(bool: bool) -> Self {
        Expr::Bool(bool)
    }
    #[inline]
    pub fn dom(selector: impl Into<String>) -> Self {
        Expr::Dom(selector.into())
    }
    #[inline]
    pub fn json_root(pointer: impl Into<String>) -> Self {
        Expr::Json(pointer.into())
    }

    #[inline]
    pub fn attr(self, name: impl Into<String>) -> Self {
        Expr::Attr {
            target: Box::new(self),
            name: name.into(),
        }
    }
    #[inline]
    pub fn text(self) -> Self {
        Expr::Text {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn inner_html(self) -> Self {
        Expr::InnerHtml {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn select(self, selector: impl Into<String>) -> Self {
        Expr::Select {
            target: Box::new(self),
            selector: selector.into(),
        }
    }
    #[inline]
    pub fn first(self, selector: impl Into<String>) -> Self {
        Expr::First {
            target: Box::new(self),
            selector: selector.into(),
        }
    }
    #[inline]
    pub fn has_class(self, class: impl Into<String>) -> Self {
        Expr::HasClass {
            target: Box::new(self),
            class: class.into(),
        }
    }
    #[inline]
    pub fn children(self) -> Self {
        Expr::Children {
            target: Box::new(self),
        }
    }

    #[inline]
    pub fn split(self, delimiter: impl Into<String>) -> Self {
        Expr::Split {
            target: Box::new(self),
            delimiter: delimiter.into(),
        }
    }
    #[inline]
    pub fn at(self, index: i32) -> Self {
        Expr::At {
            target: Box::new(self),
            index,
        }
    }
    #[inline]
    pub fn trim(self) -> Self {
        Expr::Trim {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn lower(self) -> Self {
        Expr::Lower {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn replace(self, from: impl Into<String>, to: impl Into<String>) -> Self {
        Expr::Replace {
            target: Box::new(self),
            from: from.into(),
            to: to.into(),
        }
    }
    #[inline]
    pub fn slice(self, start: i32, end: Option<i32>) -> Self {
        Expr::Slice {
            target: Box::new(self),
            start,
            end,
        }
    }
    #[inline]
    pub fn starts_with(self, prefix: impl Into<String>) -> Self {
        Expr::StartsWith {
            target: Box::new(self),
            prefix: prefix.into(),
        }
    }
    #[inline]
    pub fn ends_with(self, suffix: impl Into<String>) -> Self {
        Expr::EndsWith {
            target: Box::new(self),
            suffix: suffix.into(),
        }
    }
    #[inline]
    pub fn matches(self, pattern: impl Into<String>) -> Self {
        Expr::Matches {
            target: Box::new(self),
            pattern: pattern.into(),
        }
    }
    #[inline]
    pub fn capture(self, pattern: impl Into<String>) -> Self {
        Expr::Capture {
            target: Box::new(self),
            pattern: pattern.into(),
        }
    }
    #[inline]
    pub fn append(self, suffix: Expr) -> Self {
        Expr::Append {
            target: Box::new(self),
            suffix: Box::new(suffix),
        }
    }
    #[inline]
    pub fn append_str(self, s: impl Into<String>) -> Self {
        self.append(Expr::Literal(s.into()))
    }
    #[inline]
    pub fn prepend(self, prefix: Expr) -> Self {
        Expr::Prepend {
            target: Box::new(self),
            prefix: Box::new(prefix),
        }
    }
    #[inline]
    pub fn prepend_str(self, s: impl Into<String>) -> Self {
        self.prepend(Expr::Literal(s.into()))
    }

    #[inline]
    pub fn parse_float(self) -> Self {
        Expr::ParseFloat {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn parse_int(self) -> Self {
        Expr::ParseInt {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn stringify(self) -> Self {
        Expr::ToString {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn date_parse(self, format: impl Into<String>) -> Self {
        Expr::DateParse {
            target: Box::new(self),
            format: format.into(),
        }
    }
    #[inline]
    pub fn date_parse_rfc3339(self) -> Self {
        Expr::DateParseRfc3339 {
            target: Box::new(self),
        }
    }

    #[inline]
    pub fn fallback(self, default: Expr) -> Self {
        Expr::Fallback {
            target: Box::new(self),
            default: Box::new(default),
        }
    }
    #[inline]
    pub fn fallback_str(self, default: impl Into<String>) -> Self {
        self.fallback(Expr::Literal(default.into()))
    }
    #[inline]
    pub fn lookup(self, table: Vec<(&str, &str)>) -> Self {
        Expr::Lookup {
            target: Box::new(self),
            table: table
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        }
    }
    #[inline]
    pub fn if_then_else(condition: Expr, then: Expr, else_: Expr) -> Self {
        Expr::If {
            condition: Box::new(condition),
            then: Box::new(then),
            else_: Box::new(else_),
        }
    }
    #[inline]
    pub fn let_bind(name: impl Into<String>, value: Expr, body: Expr) -> Self {
        Expr::Let {
            name: name.into(),
            value: Box::new(value),
            body: Box::new(body),
        }
    }

    #[inline]
    pub fn map(self, transform: Expr) -> Self {
        Expr::Map {
            target: Box::new(self),
            transform: Box::new(transform),
        }
    }
    #[inline]
    pub fn flat_map(self, transform: Expr) -> Self {
        Expr::FlatMap {
            target: Box::new(self),
            transform: Box::new(transform),
        }
    }
    #[inline]
    pub fn filter(self, predicate: Expr) -> Self {
        Expr::Filter {
            target: Box::new(self),
            filter: Box::new(predicate),
        }
    }
    #[inline]
    pub fn fold(self, base: Expr, transform: Expr) -> Self {
        Expr::Fold {
            target: Box::new(self),
            base: Box::new(base),
            transform: Box::new(transform),
        }
    }
    #[inline]
    pub fn join(self, delimiter: impl Into<String>) -> Self {
        Expr::Join {
            target: Box::new(self),
            delimiter: delimiter.into(),
        }
    }
    #[inline]
    pub fn resolve_url(self, base: Expr) -> Self {
        Expr::ResolveUrl {
            target: Box::new(self),
            base: Box::new(base),
        }
    }
    #[inline]
    pub fn list(items: Vec<Expr>) -> Self {
        Expr::List(items)
    }
    #[inline]
    pub fn concat(parts: Vec<Expr>) -> Self {
        Expr::Concat(parts)
    }

    #[inline]
    pub fn ptr(self, pointer: impl Into<String>) -> Self {
        Expr::JsonPtr {
            target: Box::new(self),
            pointer: pointer.into(),
        }
    }
    #[inline]
    pub fn str_val(self) -> Self {
        Expr::JsonStr {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn int_val(self) -> Self {
        Expr::JsonInt {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn float_val(self) -> Self {
        Expr::JsonFloat {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn bool_val(self) -> Self {
        Expr::JsonBool {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn array_len(self) -> Self {
        Expr::ArrayLen {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn keys(self) -> Self {
        Expr::JsonKeys {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn get(self, key: Expr) -> Self {
        Expr::JsonGet {
            target: Box::new(self),
            key: Box::new(key),
        }
    }
    #[inline]
    pub fn get_key(self, key: impl Into<String>) -> Self {
        self.get(Expr::Literal(key.into()))
    }
    #[inline]
    pub fn find(self, key: Expr, value: Expr) -> Self {
        Expr::JsonFind {
            target: Box::new(self),
            key: Box::new(key),
            value: Box::new(value),
        }
    }
    #[inline]
    pub fn find_kv(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.find(Expr::Literal(key.into()), Expr::Literal(value.into()))
    }
    #[inline]
    pub fn json_array(items: Vec<Expr>) -> Self {
        Expr::JsonArray(items)
    }
    #[inline]
    pub fn json_fold(self) -> Self {
        Expr::JsonFold {
            target: Box::new(self),
        }
    }

    #[inline]
    pub fn coalesce_keys(self, keys: impl IntoIterator<Item = Expr>) -> Self {
        let mut iter = keys.into_iter();
        let Some(first) = iter.next() else {
            return Expr::Null;
        };
        let base = self.clone().get(first).str_val();
        iter.fold(base, |acc, key| {
            acc.fallback(self.clone().get(key).str_val())
        })
    }

    #[inline]
    pub fn merge(lists: Vec<Expr>) -> Self {
        Expr::Merge(lists)
    }

    #[inline]
    pub fn pref(key: impl Into<String>) -> Self {
        Expr::Pref(key.into())
    }

    #[inline]
    pub fn format(template: impl Into<String>, args: Vec<Expr>) -> Self {
        Expr::Format {
            template: template.into(),
            args,
        }
    }

    #[inline]
    pub fn not(self) -> Self {
        Expr::Not {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn is_null(self) -> Self {
        Expr::BinaryOperation {
            op: Op::Eq,
            lhs: Box::new(self),
            rhs: Box::from(Expr::Null),
        }
    }
    #[inline]
    pub fn string_len(self) -> Self {
        Expr::StringLen {
            target: Box::new(self),
        }
    }

    #[inline]
    pub fn split_n(self, delimiter: impl Into<String>, n: usize) -> Self {
        Expr::SplitN {
            target: Box::new(self),
            delimiter: delimiter.into(),
            n,
        }
    }
    #[inline]
    pub fn take(self, n: usize) -> Self {
        Expr::Take {
            target: Box::new(self),
            n,
        }
    }
    #[inline]
    pub fn skip(self, n: usize) -> Self {
        Expr::Skip {
            target: Box::new(self),
            n,
        }
    }
    #[inline]
    pub fn reverse(self) -> Self {
        Expr::Reverse {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn sort_by(self, key: Expr) -> Self {
        Expr::SortBy {
            target: Box::new(self),
            key: Box::new(key),
        }
    }
    #[inline]
    pub fn unique(self) -> Self {
        Expr::Unique {
            target: Box::new(self),
        }
    }

    #[inline]
    pub fn url_encode(self) -> Self {
        Expr::UrlEncode {
            target: Box::new(self),
        }
    }
    #[inline]
    pub fn url_decode(self) -> Self {
        Expr::UrlDecode {
            target: Box::new(self),
        }
    }

    #[inline]
    pub fn format_padded(self, width: usize, fill: char, align: PadAlign) -> Self {
        Expr::FormatPadded {
            target: Box::new(self),
            width,
            fill,
            align,
        }
    }

    #[inline]
    pub fn scalar(name: impl Into<String>) -> Self {
        Expr::ScalarOverride { name: name.into() }
    }

    #[inline]
    pub fn fetch_html(url_expr: Expr, blueprint: Blueprint) -> Self {
        Expr::Fetch {
            url_expr: Box::new(url_expr),
            blueprint: Box::new(blueprint),
            method: HttpMethod::Get,
            headers: vec![],
            kind: SubBlueprintKind::Html,
            on_failure: OnFailurePolicy::Fail,
            endpoint_id: None,
        }
    }
    #[inline]
    pub fn fetch_json(url_expr: Expr, blueprint: Blueprint) -> Self {
        Expr::Fetch {
            url_expr: Box::new(url_expr),
            blueprint: Box::new(blueprint),
            method: HttpMethod::Get,
            headers: vec![],
            kind: SubBlueprintKind::Json,
            on_failure: OnFailurePolicy::Fail,
            endpoint_id: None,
        }
    }
    #[inline]
    pub fn with_endpoint_id(self, id: impl Into<String>) -> Self {
        match self {
            Expr::Fetch {
                url_expr,
                blueprint,
                method,
                headers,
                kind,
                on_failure,
                ..
            } => Expr::Fetch {
                url_expr,
                blueprint,
                method,
                headers,
                kind,
                on_failure,
                endpoint_id: Some(id.into()),
            },
            other => other,
        }
    }
    #[inline]
    pub fn with_method(self, method: HttpMethod) -> Self {
        match self {
            Expr::Fetch {
                url_expr,
                blueprint,
                headers,
                kind,
                on_failure,
                endpoint_id,
                ..
            } => Expr::Fetch {
                url_expr,
                blueprint,
                method,
                headers,
                kind,
                on_failure,
                endpoint_id,
            },
            other => other,
        }
    }
    #[inline]
    pub fn with_header(self, key: Expr, value: Expr) -> Self {
        match self {
            Expr::Fetch {
                url_expr,
                blueprint,
                method,
                mut headers,
                kind,
                on_failure,
                endpoint_id,
            } => {
                headers.push((key, value));
                Expr::Fetch {
                    url_expr,
                    blueprint,
                    method,
                    headers,
                    kind,
                    on_failure,
                    endpoint_id,
                }
            }
            other => other,
        }
    }
    #[inline]
    pub fn with_on_failure(self, policy: OnFailurePolicy) -> Self {
        match self {
            Expr::Fetch {
                url_expr,
                blueprint,
                method,
                headers,
                kind,
                endpoint_id,
                ..
            } => Expr::Fetch {
                url_expr,
                blueprint,
                method,
                headers,
                kind,
                endpoint_id,
                on_failure: policy,
            },
            other => other,
        }
    }
    #[inline]
    pub fn eq(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Eq,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn ne(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Ne,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn lt(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Lt,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn gt(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Gt,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn le(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Le,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn ge(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Ge,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn and(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::And,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn or(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Or,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn add(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Add,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn sub(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Sub,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn mul(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Mul,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }
    #[inline]
    pub fn div(self, rhs: Expr) -> Self {
        Expr::BinaryOperation {
            op: Op::Div,
            lhs: Box::new(self),
            rhs: Box::new(rhs),
        }
    }

    #[inline]
    pub fn encoded_field(
        subfields: Vec<(String, Expr)>,
        delimiter: impl Into<String>,
        encoding: crate::ast::IdEncoding,
    ) -> Self {
        Expr::EncodedField {
            subfields: subfields
                .into_iter()
                .map(|(k, v)| (k, Box::new(v)))
                .collect(),
            delimiter: delimiter.into(),
            encoding,
        }
    }

    pub fn user_fn(name: impl Into<String>, args: Vec<Expr>) -> Self {
        Expr::UserFn {
            name: name.into(),
            args,
        }
    }
}

#[cfg(feature = "builder")]
#[must_use = "BlueprintBuilder is a fluent builder; its methods return a new builder that must be used (chain further or call .build())"]
pub struct BlueprintBuilder {
    request: Option<RequestDef>,
    container: String,
    fields: Vec<FieldDef>,
    bindings: Vec<Binding>,
    scalars: Vec<FieldDef>,
    pagination: Option<PaginationConfig>,
}

#[cfg(feature = "builder")]
impl BlueprintBuilder {
    pub fn new(container: impl Into<String>) -> Self {
        Self {
            request: None,
            container: container.into(),
            fields: vec![],
            bindings: vec![],
            scalars: vec![],
            pagination: None,
        }
    }
    pub fn field(mut self, name: &str, expr: Expr) -> Self {
        self.fields.push(FieldDef {
            name: name.into(),
            expr,
            optional: false,
        });
        self
    }
    pub fn field_opt(mut self, name: &str, expr: Expr) -> Self {
        self.fields.push(FieldDef {
            name: name.into(),
            expr,
            optional: true,
        });
        self
    }
    pub fn bind(mut self, name: &str, expr: Expr) -> Self {
        self.bindings.push(Binding {
            name: name.into(),
            expr,
        });
        self
    }
    pub fn scalar(mut self, name: &str, expr: Expr) -> Self {
        self.scalars.push(FieldDef {
            name: name.into(),
            expr,
            optional: false,
        });
        self
    }
    pub fn scalar_opt(mut self, name: &str, expr: Expr) -> Self {
        self.scalars.push(FieldDef {
            name: name.into(),
            expr,
            optional: true,
        });
        self
    }
    pub fn with_request(mut self, req: RequestDef) -> Self {
        self.request = Some(req);
        self
    }
    #[inline]
    pub fn paginated(
        mut self,
        native_page_size: usize,
        offset_param: impl Into<String>,
        offset_type: OffsetType,
    ) -> Self {
        self.pagination = Some(PaginationConfig {
            native_page_size,
            offset_param: offset_param.into(),
            offset_type,
        });
        self
    }
    pub fn build(self) -> Blueprint {
        Blueprint {
            request: self.request,
            container: self.container,
            fields: self.fields,
            bindings: self.bindings,
            scalars: self.scalars,
            pagination: self.pagination,
        }
    }
}

#[cfg(feature = "builder")]
impl Blueprint {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes =
            postcard::to_allocvec(&DSL_SCHEMA_VERSION).expect("version serialization failed");
        bytes.extend(postcard::to_allocvec(self).expect("Blueprint serialization failed"));
        bytes
    }

    pub fn with_request_def(&self, req: RequestDef) -> Blueprint {
        let mut bp = self.clone();
        bp.request = Some(req);
        bp
    }
}

#[cfg(all(test, feature = "builder"))]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::approx_constant)]
    use super::*;

    fn postcard_rt(expr: &Expr) {
        let bytes = postcard::to_allocvec(expr).unwrap();
        let back: Expr = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(*expr, back);
    }

    #[test]
    fn expr_postcard_round_trip_leaf_variants() {
        for e in &[
            Expr::SelfRef,
            Expr::Index,
            Expr::Null,
            Expr::Bool(true),
            Expr::Bool(false),
            Expr::Number(3.14),
            Expr::Number(-0.0),
            Expr::Literal("hello world".into()),
            Expr::Literal(String::new()),
            Expr::Var("base_url".into()),
            Expr::Dom("div.manga-title".into()),
            Expr::Json("/data/items".into()),
            Expr::Pref("lang".into()),
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn expr_postcard_round_trip_binary_operations_all_ops() {
        for op in &[
            Op::Add,
            Op::Sub,
            Op::Mul,
            Op::Div,
            Op::Eq,
            Op::Ne,
            Op::Lt,
            Op::Gt,
            Op::Le,
            Op::Ge,
            Op::And,
            Op::Or,
        ] {
            let e = Expr::BinaryOperation {
                op: op.clone(),
                lhs: Box::new(Expr::Number(1.0)),
                rhs: Box::new(Expr::Number(2.0)),
            };
            postcard_rt(&e);
        }
    }

    #[test]
    fn expr_postcard_round_trip_html_methods() {
        for e in &[
            Expr::Attr {
                target: Box::new(Expr::SelfRef),
                name: "href".into(),
            },
            Expr::Text {
                target: Box::new(Expr::SelfRef),
            },
            Expr::InnerHtml {
                target: Box::new(Expr::SelfRef),
            },
            Expr::Select {
                target: Box::new(Expr::SelfRef),
                selector: "a.link".into(),
            },
            Expr::First {
                target: Box::new(Expr::SelfRef),
                selector: "span".into(),
            },
            Expr::HasClass {
                target: Box::new(Expr::SelfRef),
                class: "active".into(),
            },
            Expr::Children {
                target: Box::new(Expr::SelfRef),
            },
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn expr_postcard_round_trip_string_methods() {
        for e in &[
            Expr::Split {
                target: Box::new(Expr::Literal("a,b,c".into())),
                delimiter: ",".into(),
            },
            Expr::At {
                target: Box::new(Expr::SelfRef),
                index: 0,
            },
            Expr::At {
                target: Box::new(Expr::SelfRef),
                index: -1,
            },
            Expr::Replace {
                target: Box::new(Expr::Literal("hello".into())),
                from: "l".into(),
                to: "r".into(),
            },
            Expr::Trim {
                target: Box::new(Expr::SelfRef),
            },
            Expr::Lower {
                target: Box::new(Expr::SelfRef),
            },
            Expr::Matches {
                target: Box::new(Expr::SelfRef),
                pattern: r"^\d+$".into(),
            },
            Expr::Capture {
                target: Box::new(Expr::SelfRef),
                pattern: r"(\d+)".into(),
            },
            Expr::Prepend {
                target: Box::new(Expr::Literal("world".into())),
                prefix: Box::new(Expr::Literal("hello ".into())),
            },
            Expr::Append {
                target: Box::new(Expr::Literal("hello".into())),
                suffix: Box::new(Expr::Literal(" world".into())),
            },
            Expr::StartsWith {
                target: Box::new(Expr::Literal("hello".into())),
                prefix: "he".into(),
            },
            Expr::EndsWith {
                target: Box::new(Expr::Literal("hello".into())),
                suffix: "lo".into(),
            },
            Expr::Slice {
                target: Box::new(Expr::SelfRef),
                start: 0,
                end: Some(5),
            },
            Expr::Slice {
                target: Box::new(Expr::SelfRef),
                start: -3,
                end: None,
            },
            Expr::Join {
                target: Box::new(Expr::SelfRef),
                delimiter: ", ".into(),
            },
            Expr::StringLen {
                target: Box::new(Expr::Literal("hello".into())),
            },
            Expr::ToString {
                target: Box::new(Expr::Number(42.0)),
            },
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn expr_postcard_round_trip_parse_and_date() {
        for e in &[
            Expr::ParseFloat {
                target: Box::new(Expr::Literal("3.14".into())),
            },
            Expr::ParseInt {
                target: Box::new(Expr::Literal("42".into())),
            },
            Expr::DateParse {
                target: Box::new(Expr::SelfRef),
                format: "%Y-%m-%d".into(),
            },
            Expr::DateParseRfc3339 {
                target: Box::new(Expr::SelfRef),
            },
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn expr_postcard_round_trip_json_methods() {
        for e in &[
            Expr::JsonPtr {
                target: Box::new(Expr::Json("/".into())),
                pointer: "/key".into(),
            },
            Expr::JsonStr {
                target: Box::new(Expr::SelfRef),
            },
            Expr::JsonInt {
                target: Box::new(Expr::SelfRef),
            },
            Expr::JsonFloat {
                target: Box::new(Expr::SelfRef),
            },
            Expr::JsonBool {
                target: Box::new(Expr::SelfRef),
            },
            Expr::ArrayLen {
                target: Box::new(Expr::SelfRef),
            },
            Expr::JsonKeys {
                target: Box::new(Expr::SelfRef),
            },
            Expr::JsonGet {
                target: Box::new(Expr::SelfRef),
                key: Box::new(Expr::Literal("k".into())),
            },
            Expr::JsonFind {
                target: Box::new(Expr::SelfRef),
                key: Box::new(Expr::Literal("id".into())),
                value: Box::new(Expr::Literal("1".into())),
            },
            Expr::JsonFold {
                target: Box::new(Expr::SelfRef),
            },
            Expr::JsonArray(vec![Expr::Number(1.0), Expr::Null, Expr::Bool(false)]),
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn expr_postcard_round_trip_control_flow_and_collections() {
        for e in &[
            Expr::Let {
                name: "x".into(),
                value: Box::new(Expr::Number(1.0)),
                body: Box::new(Expr::Var("x".into())),
            },
            Expr::Fallback {
                target: Box::new(Expr::Null),
                default: Box::new(Expr::Literal("default".into())),
            },
            Expr::Lookup {
                target: Box::new(Expr::SelfRef),
                table: vec![("a".into(), "A".into()), ("b".into(), "B".into())],
            },
            Expr::If {
                condition: Box::new(Expr::Bool(true)),
                then: Box::new(Expr::Number(1.0)),
                else_: Box::new(Expr::Number(0.0)),
            },
            Expr::Not {
                target: Box::new(Expr::Bool(false)),
            },
            Expr::Map {
                target: Box::new(Expr::SelfRef),
                transform: Box::new(Expr::Text {
                    target: Box::new(Expr::SelfRef),
                }),
            },
            Expr::FlatMap {
                target: Box::new(Expr::SelfRef),
                transform: Box::new(Expr::SelfRef),
            },
            Expr::Filter {
                target: Box::new(Expr::SelfRef),
                filter: Box::new(Expr::Bool(true)),
            },
            Expr::Fold {
                target: Box::new(Expr::SelfRef),
                transform: Box::new(Expr::SelfRef),
                base: Box::new(Expr::Literal(String::new())),
            },
            Expr::List(vec![Expr::Number(1.0), Expr::Number(2.0)]),
            Expr::List(vec![]),
            Expr::Concat(vec![Expr::Literal("a".into()), Expr::Literal("b".into())]),
            Expr::Merge(vec![Expr::SelfRef, Expr::SelfRef]),
            Expr::ResolveUrl {
                target: Box::new(Expr::Literal("/page".into())),
                base: Box::new(Expr::Literal("https://example.com".into())),
            },
            Expr::Format {
                template: "Hello {}!".into(),
                args: vec![Expr::Literal("world".into())],
            },
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn expr_postcard_round_trip_v2_list_ops() {
        for e in &[
            Expr::SplitN {
                target: Box::new(Expr::Literal("a,b,c,d".into())),
                delimiter: ",".into(),
                n: 2,
            },
            Expr::SplitN {
                target: Box::new(Expr::SelfRef),
                delimiter: "/".into(),
                n: 1,
            },
            Expr::Take {
                target: Box::new(Expr::SelfRef),
                n: 5,
            },
            Expr::Take {
                target: Box::new(Expr::SelfRef),
                n: 0,
            },
            Expr::Skip {
                target: Box::new(Expr::SelfRef),
                n: 3,
            },
            Expr::Reverse {
                target: Box::new(Expr::SelfRef),
            },
            Expr::SortBy {
                target: Box::new(Expr::SelfRef),
                key: Box::new(Expr::SelfRef),
            },
            Expr::Unique {
                target: Box::new(Expr::SelfRef),
            },
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn expr_postcard_round_trip_v2_url_and_format() {
        for e in &[
            Expr::UrlEncode {
                target: Box::new(Expr::Literal("hello world".into())),
            },
            Expr::UrlDecode {
                target: Box::new(Expr::Literal("hello%20world".into())),
            },
            Expr::FormatPadded {
                target: Box::new(Expr::SelfRef),
                width: 10,
                fill: ' ',
                align: PadAlign::Left,
            },
            Expr::FormatPadded {
                target: Box::new(Expr::SelfRef),
                width: 8,
                fill: '0',
                align: PadAlign::Right,
            },
            Expr::FormatPadded {
                target: Box::new(Expr::SelfRef),
                width: 20,
                fill: '-',
                align: PadAlign::Center,
            },
            Expr::ScalarOverride {
                name: "total_pages".into(),
            },
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn blueprint_to_bytes_has_version_prefix() {
        let bp = BlueprintBuilder::new(".item").build();
        let bytes = bp.to_bytes();
        let (version, _): (u32, &[u8]) = postcard::take_from_bytes(&bytes).unwrap();
        assert_eq!(version, DSL_SCHEMA_VERSION);
    }

    #[test]
    fn blueprint_postcard_round_trip_all_fields_populated() {
        let bp = BlueprintBuilder::new(".manga-list .item")
            .bind("base", Expr::dom("meta[property='og:url']").attr("content"))
            .field("id", Expr::self_ref().attr("data-id"))
            .field_opt("cover", Expr::self_ref().first("img").attr("src"))
            .scalar("total", Expr::dom(".pagination").text().parse_int())
            .scalar_opt("has_next", Expr::Null)
            .with_request(RequestDef {
                url: "https://example.com/popular".into(),
                method: "GET".into(),
                headers: vec![("Accept".into(), "text/html".into())],
                queries: vec![("page".into(), "1".into())],
                endpoint_id: None,
            })
            .paginated(32, "offset", OffsetType::ItemOffset)
            .build();

        let bytes = postcard::to_allocvec(&bp).unwrap();
        let back: Blueprint = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(bp.container, back.container);
        assert_eq!(bp.fields.len(), back.fields.len());
        assert_eq!(bp.bindings.len(), back.bindings.len());
        assert_eq!(bp.scalars.len(), back.scalars.len());

        let req = back.request.as_ref().unwrap();
        assert_eq!(req.url, "https://example.com/popular");
        assert_eq!(req.method, "GET");
        assert_eq!(req.headers, vec![("Accept".into(), "text/html".into())]);
        assert_eq!(req.queries, vec![("page".into(), "1".into())]);

        let pg = back.pagination.as_ref().unwrap();
        assert_eq!(pg.native_page_size, 32);
        assert_eq!(pg.offset_param, "offset");
        assert_eq!(pg.offset_type, OffsetType::ItemOffset);
    }

    #[test]
    fn blueprint_postcard_round_trip_minimal() {
        let bp = BlueprintBuilder::new("").build();
        let bytes = postcard::to_allocvec(&bp).unwrap();
        let back: Blueprint = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(bp.container, back.container);
        assert!(back.request.is_none());
        assert!(back.pagination.is_none());
        assert!(back.fields.is_empty());
    }

    #[test]
    fn blueprint_builder_produces_correct_structure() {
        let bp = BlueprintBuilder::new(".card")
            .field("id", Expr::self_ref().attr("data-id"))
            .field_opt("subtitle", Expr::Null)
            .bind("host", Expr::lit("https://example.com"))
            .scalar("count", Expr::num(42.0))
            .scalar_opt("page", Expr::Null)
            .build();

        assert_eq!(bp.container, ".card");
        assert_eq!(bp.fields.len(), 2);
        assert_eq!(bp.fields[0].name, "id");
        assert!(!bp.fields[0].optional);
        assert_eq!(bp.fields[1].name, "subtitle");
        assert!(bp.fields[1].optional);
        assert_eq!(bp.bindings.len(), 1);
        assert_eq!(bp.bindings[0].name, "host");
        assert_eq!(bp.scalars.len(), 2);
        assert_eq!(bp.scalars[0].name, "count");
        assert!(!bp.scalars[0].optional);
        assert!(bp.scalars[1].optional);
        assert!(bp.request.is_none());
        assert!(bp.pagination.is_none());
    }

    #[test]
    fn blueprint_to_bytes_is_deterministic() {
        let bp = BlueprintBuilder::new(".item")
            .field("title", Expr::self_ref().text().trim())
            .field("url", Expr::self_ref().first("a").attr("href"))
            .build();
        assert_eq!(bp.to_bytes(), bp.to_bytes());
    }

    #[test]
    fn with_request_def_only_changes_request_field() {
        let bp = BlueprintBuilder::new(".item")
            .field("title", Expr::self_ref().text())
            .bind("x", Expr::Null)
            .scalar("n", Expr::num(1.0))
            .paginated(10, "page", OffsetType::PageNumber { start: 1 })
            .build();

        let req = RequestDef {
            url: "https://test.com/list".into(),
            method: "POST".into(),
            headers: vec![],
            queries: vec![],
            endpoint_id: None,
        };
        let bp2 = bp.with_request_def(req);

        let r = bp2.request.as_ref().unwrap();
        assert_eq!(r.url, "https://test.com/list");
        assert_eq!(r.method, "POST");
        assert_eq!(bp2.container, ".item");
        assert_eq!(bp2.fields.len(), 1);
        assert_eq!(bp2.bindings.len(), 1);
        assert_eq!(bp2.scalars.len(), 1);
        let pg = bp2.pagination.as_ref().unwrap();
        assert_eq!(pg.offset_type, OffsetType::PageNumber { start: 1 });
    }

    #[test]
    fn pagination_config_postcard_round_trip_both_offset_types() {
        for cfg in &[
            PaginationConfig {
                native_page_size: 32,
                offset_param: "offset".into(),
                offset_type: OffsetType::ItemOffset,
            },
            PaginationConfig {
                native_page_size: 20,
                offset_param: "page".into(),
                offset_type: OffsetType::PageNumber { start: 1 },
            },
            PaginationConfig {
                native_page_size: 50,
                offset_param: "p".into(),
                offset_type: OffsetType::PageNumber { start: 0 },
            },
        ] {
            let bytes = postcard::to_allocvec(cfg).unwrap();
            let back: PaginationConfig = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(*cfg, back);
        }
    }

    #[test]
    fn encoded_field_postcard_round_trip() {
        for encoding in &[
            IdEncoding::Base64Url,
            IdEncoding::Base64,
            IdEncoding::Passthrough,
            IdEncoding::Hex,
        ] {
            let expr = Expr::EncodedField {
                subfields: vec![
                    ("manga_id".into(), Box::new(Expr::Var("id".into()))),
                    ("ch_id".into(), Box::new(Expr::Literal("ch1".into()))),
                ],
                delimiter: "|".into(),
                encoding: encoding.clone(),
            };
            postcard_rt(&expr);
        }
    }

    #[test]
    fn fetch_on_failure_policy_postcard_round_trip() {
        let sub_bp = BlueprintBuilder::new("")
            .field("x", Expr::Literal("v".into()))
            .build();
        for policy in [
            OnFailurePolicy::Fail,
            OnFailurePolicy::Skip,
            OnFailurePolicy::Use(Box::new(Expr::Literal("fallback".into()))),
        ] {
            let expr =
                Expr::fetch_json(Expr::Literal("https://example.com".into()), sub_bp.clone())
                    .with_on_failure(policy);
            postcard_rt(&expr);
        }
    }

    #[test]
    fn expr_postcard_round_trip_user_fn() {
        for e in &[
            Expr::user_fn("slugify", vec![Expr::SelfRef]),
            Expr::user_fn(
                "format_date",
                vec![Expr::SelfRef, Expr::Literal("YYYY-MM-DD".into())],
            ),
            Expr::user_fn("noop", vec![]),
        ] {
            postcard_rt(e);
        }
    }

    #[test]
    fn schema_version_is_six() {
        assert_eq!(DSL_SCHEMA_VERSION, 6);
    }

    #[test]
    fn arena_validation_rejects_invalid_ids_and_cycles() {
        let invalid = ExprArena {
            nodes: vec![ExprNode::Unary {
                op: UnaryExprOp::Trim,
                target: ExprId(9),
            }],
        };
        assert!(
            invalid
                .validate(ExprId(0))
                .unwrap_err()
                .contains("out-of-bounds")
        );

        let cyclic = ExprArena {
            nodes: vec![ExprNode::Unary {
                op: UnaryExprOp::Trim,
                target: ExprId(0),
            }],
        };
        assert!(
            cyclic
                .validate(ExprId(0))
                .unwrap_err()
                .contains("non-topological")
        );
    }

    #[test]
    fn version_six_arena_blueprint_bytes_are_deterministic() {
        let arena = Arc::new(ExprArena {
            nodes: vec![ExprNode::Leaf(ExprLeaf::Literal("title".into()))],
        });
        let blueprint = BlueprintBuilder::new("")
            .field(
                "title",
                Expr::Arena {
                    arena,
                    root: ExprId(0),
                },
            )
            .build();
        let first = blueprint.to_bytes();
        assert_eq!(first, blueprint.to_bytes());
        let (version, _): (u32, &[u8]) = postcard::take_from_bytes(&first).unwrap();
        assert_eq!(version, 6);
    }
}
