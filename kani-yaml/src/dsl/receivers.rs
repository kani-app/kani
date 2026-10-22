use kani_shared::ast::{
    BinaryExprOp, Expr, ExprArena, ExprLeaf, ExprNode, ManyExprOp, UnaryExprOp,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Unknown,
    List,
    Str,
    Num,
    Bool,
    Json,
}

impl Kind {
    fn described(self) -> Option<&'static str> {
        match self {
            Kind::Unknown => None,
            Kind::List => Some("a list"),
            Kind::Str => Some("a string"),
            Kind::Num => Some("a number"),
            Kind::Bool => Some("a boolean"),
            Kind::Json => Some("a JSON value"),
        }
    }
}

fn needs_element(op: &UnaryExprOp) -> bool {
    matches!(
        op,
        UnaryExprOp::Attr(_)
            | UnaryExprOp::Text
            | UnaryExprOp::InnerHtml
            | UnaryExprOp::Select(_)
            | UnaryExprOp::First(_)
            | UnaryExprOp::HasClass(_)
            | UnaryExprOp::Children
    )
}

fn unary_output(op: &UnaryExprOp) -> Kind {
    match op {
        UnaryExprOp::Select(_) | UnaryExprOp::Children | UnaryExprOp::Split(_) => Kind::List,
        UnaryExprOp::SplitN(..) => Kind::List,
        UnaryExprOp::Attr(_)
        | UnaryExprOp::Text
        | UnaryExprOp::InnerHtml
        | UnaryExprOp::Replace(..)
        | UnaryExprOp::Trim
        | UnaryExprOp::Lower
        | UnaryExprOp::JsonStr
        | UnaryExprOp::ToString
        | UnaryExprOp::Join(_)
        | UnaryExprOp::UrlEncode
        | UnaryExprOp::UrlDecode
        | UnaryExprOp::FormatPadded { .. } => Kind::Str,
        UnaryExprOp::ParseFloat
        | UnaryExprOp::ParseInt
        | UnaryExprOp::JsonInt
        | UnaryExprOp::JsonFloat
        | UnaryExprOp::ArrayLen
        | UnaryExprOp::StringLen => Kind::Num,
        UnaryExprOp::Matches(_)
        | UnaryExprOp::StartsWith(_)
        | UnaryExprOp::EndsWith(_)
        | UnaryExprOp::HasClass(_)
        | UnaryExprOp::JsonBool
        | UnaryExprOp::Not => Kind::Bool,
        UnaryExprOp::JsonPtr(_) => Kind::Json,
        _ => Kind::Unknown,
    }
}

fn check_arena(arena: &ExprArena) -> Result<(), String> {
    let mut kinds = Vec::with_capacity(arena.nodes.len());
    for node in &arena.nodes {
        let kind_of = |id: kani_shared::ast::ExprId| {
            kinds.get(id.0 as usize).copied().unwrap_or(Kind::Unknown)
        };
        let kind = match node {
            ExprNode::Leaf(ExprLeaf::Literal(_)) => Kind::Str,
            ExprNode::Leaf(ExprLeaf::Number(_)) => Kind::Num,
            ExprNode::Leaf(ExprLeaf::Bool(_)) => Kind::Bool,
            ExprNode::Leaf(ExprLeaf::Json(_)) => Kind::Json,
            ExprNode::Unary { op, target } => {
                if needs_element(op)
                    && let Some(receiver) = kind_of(*target).described()
                {
                    return Err(format!(
                        "this method needs an HTML element, but its receiver is {receiver}"
                    ));
                }
                unary_output(op)
            }
            ExprNode::Binary {
                op: BinaryExprOp::Prepend | BinaryExprOp::Append,
                ..
            } => Kind::Str,
            ExprNode::Many {
                op: ManyExprOp::List,
                ..
            } => Kind::List,
            _ => Kind::Unknown,
        };
        kinds.push(kind);
    }
    Ok(())
}

/// Refuses an element-only method (`attr`, `text`, `select`, …) whose receiver is statically
/// known to be a list, string, number, boolean or JSON value. Receivers whose kind depends on
/// data pass, so the check never rejects an expression that could evaluate.
pub fn check_receivers(expr: &Expr) -> Result<(), String> {
    match expr {
        Expr::Arena { arena, .. } => check_arena(arena),
        _ => Ok(()),
    }
}
