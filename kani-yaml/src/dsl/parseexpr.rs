use crate::error::YamlError;
use chumsky::prelude::SimpleSpan;
use kani_shared::ast::{
    BinaryExprOp, Expr, ExprArena, ExprId, ExprLeaf, ExprNode, ManyExprOp, Op, PadAlign,
    TernaryExprOp, UnaryExprOp,
};
use std::sync::Arc;

/// A `ParseExpr` paired with its source span, produced at the parse boundary.
///
/// Using a newtype (rather than embedding spans in every `ParseExpr` variant)
/// keeps internal types clean while giving the conversion to `Expr` access to
/// source positions for accurate error reporting.
#[derive(Debug, Clone)]
pub struct SpannedParseExpr(pub(super) ParseExpr, pub(super) SimpleSpan);

#[derive(Debug, Clone)]
pub enum ParseExpr {
    SelfRef,
    Dom(String),
    Json(String),
    Var(String),
    Literal(String),
    Number(f64),
    Bool(bool),
    Null,
    BinaryOperation {
        op: Op,
        lhs: Box<ParseExpr>,
        rhs: Box<ParseExpr>,
    },
    Let {
        name: String,
        value: Box<ParseExpr>,
        body: Box<ParseExpr>,
    },
    List(Vec<ParseExpr>),
    Concat(Vec<ParseExpr>),
    Merge(Vec<ParseExpr>),
    Pref(String),
    Scalar(String),
    Format {
        template: String,
        args: Vec<ParseExpr>,
    },
    JsonArray(Vec<ParseExpr>),
    If {
        condition: Box<ParseExpr>,
        then: Box<ParseExpr>,
        else_: Box<ParseExpr>,
    },
    MethodCall {
        target: Box<ParseExpr>,
        name: String,
        args: Vec<ParseExpr>,
        span: SimpleSpan,
    },
    MapLiteral(Vec<(String, String)>),
    Index,
}

fn collect_results(items: Vec<ParseExpr>) -> Result<Vec<Expr>, Vec<YamlError>> {
    let mut exprs = Vec::with_capacity(items.len());
    let mut errors = Vec::new();
    for item in items {
        match Expr::try_from(item) {
            Ok(e) => exprs.push(e),
            Err(mut es) => errors.append(&mut es),
        }
    }
    if errors.is_empty() {
        Ok(exprs)
    } else {
        Err(errors)
    }
}

impl TryFrom<ParseExpr> for Expr {
    type Error = Vec<YamlError>;

    fn try_from(value: ParseExpr) -> Result<Self, Self::Error> {
        macro_rules! accumulate {
            ($res1:expr, $res2:expr => $map_fn:expr) => {
                match ($res1, $res2) {
                    (Ok(v1), Ok(v2)) => Ok($map_fn(Box::new(v1), Box::new(v2))),
                    (r1, r2) => {
                        let mut errs = Vec::new();
                        if let Err(mut e) = r1 {
                            errs.append(&mut e);
                        }
                        if let Err(mut e) = r2 {
                            errs.append(&mut e);
                        }
                        Err(errs)
                    }
                }
            };
            ($res1:expr, $res2:expr, $res3:expr => $map_fn:expr) => {
                match ($res1, $res2, $res3) {
                    (Ok(v1), Ok(v2), Ok(v3)) => {
                        Ok($map_fn(Box::new(v1), Box::new(v2), Box::new(v3)))
                    }
                    (r1, r2, r3) => {
                        let mut errs = Vec::new();
                        if let Err(mut e) = r1 {
                            errs.append(&mut e);
                        }
                        if let Err(mut e) = r2 {
                            errs.append(&mut e);
                        }
                        if let Err(mut e) = r3 {
                            errs.append(&mut e);
                        }
                        Err(errs)
                    }
                }
            };
        }

        macro_rules! wrap_target {
            ($target_res:expr, $mapper:expr) => {
                $target_res.map(|t| $mapper(Box::new(t)))
            };
        }

        match value {
            ParseExpr::SelfRef => Ok(Expr::SelfRef),
            ParseExpr::Dom(s) => Ok(Expr::Dom(s)),
            ParseExpr::Json(s) => Ok(Expr::Json(s)),
            ParseExpr::Var(s) => Ok(Expr::Var(s)),
            ParseExpr::Literal(s) => Ok(Expr::Literal(s)),
            ParseExpr::Number(n) => Ok(Expr::Number(n)),
            ParseExpr::Bool(b) => Ok(Expr::Bool(b)),
            ParseExpr::Null => Ok(Expr::Null),
            ParseExpr::Pref(s) => Ok(Expr::Pref(s)),
            ParseExpr::Scalar(name) => Ok(Expr::ScalarOverride { name }),
            ParseExpr::Index => Ok(Expr::Index),

            ParseExpr::BinaryOperation { op, lhs, rhs } => {
                accumulate!(Expr::try_from(*lhs), Expr::try_from(*rhs) => |l, r| Expr::BinaryOperation { op, lhs: l, rhs: r })
            }

            ParseExpr::Let { name, value, body } => {
                accumulate!(Expr::try_from(*value), Expr::try_from(*body) => |v, b| Expr::Let { name, value: v, body: b })
            }

            ParseExpr::If {
                condition,
                then,
                else_,
            } => {
                accumulate!(Expr::try_from(*condition), Expr::try_from(*then), Expr::try_from(*else_) => |c, t, e| Expr::If { condition: c, then: t, else_: e })
            }

            ParseExpr::List(l) => collect_results(l).map(Expr::List),
            ParseExpr::Concat(l) => collect_results(l).map(Expr::Concat),
            ParseExpr::Merge(l) => collect_results(l).map(Expr::Merge),
            ParseExpr::JsonArray(l) => collect_results(l).map(Expr::JsonArray),
            ParseExpr::Format { template, args } => {
                collect_results(args).map(|a| Expr::Format { template, args: a })
            }

            ParseExpr::MapLiteral(_) => Err(vec![YamlError::DslConversion {
                message: "Map literals '{...}' can only be used inside .lookup()".to_string(),
                span: 0..0,
            }]),

            ParseExpr::MethodCall {
                target,
                name,
                args,
                span,
            } => {
                let target_res = Expr::try_from(*target);

                if let Some(fn_name) = name.strip_prefix("__user::") {
                    let args_res = collect_results(args);
                    return match (target_res, args_res) {
                        (Ok(receiver), Ok(mut explicit_args)) => {
                            explicit_args.insert(0, receiver);
                            Ok(Expr::UserFn {
                                name: fn_name.to_string(),
                                args: explicit_args,
                            })
                        }
                        (r_res, a_res) => {
                            let mut errs = r_res.err().unwrap_or_default();
                            if let Err(mut e) = a_res {
                                errs.append(&mut e);
                            }
                            Err(errs)
                        }
                    };
                }

                match (name.as_str(), args.as_slice()) {
                    ("attr", [ParseExpr::Literal(n)]) => wrap_target!(target_res, |t| Expr::Attr {
                        target: t,
                        name: n.clone()
                    }),
                    ("text", []) => wrap_target!(target_res, |t| Expr::Text { target: t }),
                    ("inner_html", []) => {
                        wrap_target!(target_res, |t| Expr::InnerHtml { target: t })
                    }
                    ("select", [ParseExpr::Literal(s)]) => {
                        wrap_target!(target_res, |t| Expr::Select {
                            target: t,
                            selector: s.clone()
                        })
                    }
                    ("first", [ParseExpr::Literal(s)]) => {
                        wrap_target!(target_res, |t| Expr::First {
                            target: t,
                            selector: s.clone()
                        })
                    }
                    ("split", [ParseExpr::Literal(d)]) => {
                        wrap_target!(target_res, |t| Expr::Split {
                            target: t,
                            delimiter: d.clone()
                        })
                    }
                    ("at", [ParseExpr::Number(i)]) => wrap_target!(target_res, |t| Expr::At {
                        target: t,
                        index: *i as i32
                    }),
                    ("trim", []) => wrap_target!(target_res, |t| Expr::Trim { target: t }),
                    ("lower", []) => wrap_target!(target_res, |t| Expr::Lower { target: t }),
                    ("parse_float", []) => {
                        wrap_target!(target_res, |t| Expr::ParseFloat { target: t })
                    }
                    ("parse_int", []) => wrap_target!(target_res, |t| Expr::ParseInt { target: t }),
                    ("to_string", []) => wrap_target!(target_res, |t| Expr::ToString { target: t }),
                    ("str", []) => wrap_target!(target_res, |t| Expr::JsonStr { target: t }),
                    ("int", []) => wrap_target!(target_res, |t| Expr::JsonInt { target: t }),
                    ("float", []) => wrap_target!(target_res, |t| Expr::JsonFloat { target: t }),
                    ("bool", []) => wrap_target!(target_res, |t| Expr::JsonBool { target: t }),
                    ("array_len", []) => wrap_target!(target_res, |t| Expr::ArrayLen { target: t }),
                    ("keys", []) => wrap_target!(target_res, |t| Expr::JsonKeys { target: t }),
                    ("json_fold", []) => wrap_target!(target_res, |t| Expr::JsonFold { target: t }),
                    ("children", []) => wrap_target!(target_res, |t| Expr::Children { target: t }),
                    ("date_parse_rfc3339", []) => {
                        wrap_target!(target_res, |t| Expr::DateParseRfc3339 { target: t })
                    }
                    ("not", []) => wrap_target!(target_res, |t| Expr::Not { target: t }),
                    ("string_len", []) => {
                        wrap_target!(target_res, |t| Expr::StringLen { target: t })
                    }

                    ("slice", [ParseExpr::Number(s)]) => {
                        wrap_target!(target_res, |t| Expr::Slice {
                            target: t,
                            start: *s as i32,
                            end: None
                        })
                    }
                    ("slice", [ParseExpr::Number(s), ParseExpr::Number(e)]) => {
                        wrap_target!(target_res, |t| Expr::Slice {
                            target: t,
                            start: *s as i32,
                            end: Some(*e as i32)
                        })
                    }
                    ("replace", [ParseExpr::Literal(f), ParseExpr::Literal(to)]) => {
                        wrap_target!(target_res, |t| Expr::Replace {
                            target: t,
                            from: f.clone(),
                            to: to.clone()
                        })
                    }
                    ("lookup", [ParseExpr::MapLiteral(table)]) => {
                        wrap_target!(target_res, |t| Expr::Lookup {
                            target: t,
                            table: table.clone()
                        })
                    }
                    ("matches", [ParseExpr::Literal(p)]) => {
                        wrap_target!(target_res, |t| Expr::Matches {
                            target: t,
                            pattern: p.clone()
                        })
                    }
                    ("capture", [ParseExpr::Literal(p)]) => {
                        wrap_target!(target_res, |t| Expr::Capture {
                            target: t,
                            pattern: p.clone()
                        })
                    }
                    ("ptr", [ParseExpr::Literal(p)]) => {
                        wrap_target!(target_res, |t| Expr::JsonPtr {
                            target: t,
                            pointer: p.clone()
                        })
                    }
                    ("has_class", [ParseExpr::Literal(c)]) => {
                        wrap_target!(target_res, |t| Expr::HasClass {
                            target: t,
                            class: c.clone()
                        })
                    }
                    ("starts_with", [ParseExpr::Literal(p)]) => {
                        wrap_target!(target_res, |t| Expr::StartsWith {
                            target: t,
                            prefix: p.clone()
                        })
                    }
                    ("ends_with", [ParseExpr::Literal(s)]) => {
                        wrap_target!(target_res, |t| Expr::EndsWith {
                            target: t,
                            suffix: s.clone()
                        })
                    }
                    ("date_parse", [ParseExpr::Literal(f)]) => {
                        wrap_target!(target_res, |t| Expr::DateParse {
                            target: t,
                            format: f.clone()
                        })
                    }
                    ("join", [ParseExpr::Literal(d)]) => wrap_target!(target_res, |t| Expr::Join {
                        target: t,
                        delimiter: d.clone()
                    }),

                    ("map", [tr]) => {
                        accumulate!(target_res, Expr::try_from(tr.clone()) => |t, tr| Expr::Map { target: t, transform: tr })
                    }
                    ("flat_map", [tr]) => {
                        accumulate!(target_res, Expr::try_from(tr.clone()) => |t, tr| Expr::FlatMap { target: t, transform: tr })
                    }
                    ("filter", [f]) => {
                        accumulate!(target_res, Expr::try_from(f.clone()) => |t, f| Expr::Filter { target: t, filter: f })
                    }
                    ("prepend", [p]) => {
                        accumulate!(target_res, Expr::try_from(p.clone()) => |t, p| Expr::Prepend { target: t, prefix: p })
                    }
                    ("append", [s]) => {
                        accumulate!(target_res, Expr::try_from(s.clone()) => |t, s| Expr::Append { target: t, suffix: s })
                    }
                    ("fallback", [d]) => {
                        accumulate!(target_res, Expr::try_from(d.clone()) => |t, d| Expr::Fallback { target: t, default: d })
                    }
                    ("resolve_url", [b]) => {
                        accumulate!(target_res, Expr::try_from(b.clone()) => |t, b| Expr::ResolveUrl { target: t, base: b })
                    }
                    ("get", [k]) => {
                        accumulate!(target_res, Expr::try_from(k.clone()) => |t, k| Expr::JsonGet { target: t, key: k })
                    }

                    ("fold", [b, tr]) => {
                        accumulate!(target_res, Expr::try_from(b.clone()), Expr::try_from(tr.clone()) => |t, b, tr| Expr::Fold { target: t, transform: tr, base: b })
                    }
                    ("find", [k, v]) => {
                        accumulate!(target_res, Expr::try_from(k.clone()), Expr::try_from(v.clone()) => |t, k, v| Expr::JsonFind { target: t, key: k, value: v })
                    }

                    ("split_n", [ParseExpr::Literal(d), ParseExpr::Number(n)]) => {
                        wrap_target!(target_res, |t| Expr::SplitN {
                            target: t,
                            delimiter: d.clone(),
                            n: *n as usize,
                        })
                    }
                    ("take", [ParseExpr::Number(n)]) => {
                        wrap_target!(target_res, |t| Expr::Take {
                            target: t,
                            n: *n as usize
                        })
                    }
                    ("skip", [ParseExpr::Number(n)]) => {
                        wrap_target!(target_res, |t| Expr::Skip {
                            target: t,
                            n: *n as usize
                        })
                    }
                    ("reverse", []) => wrap_target!(target_res, |t| Expr::Reverse { target: t }),
                    ("sort_by", [k]) => {
                        accumulate!(target_res, Expr::try_from(k.clone()) => |t, k| Expr::SortBy { target: t, key: k })
                    }
                    ("unique", []) => wrap_target!(target_res, |t| Expr::Unique { target: t }),
                    ("url_encode" | "urlencode", []) => {
                        wrap_target!(target_res, |t| Expr::UrlEncode { target: t })
                    }
                    ("url_decode" | "urldecode", []) => {
                        wrap_target!(target_res, |t| Expr::UrlDecode { target: t })
                    }
                    (
                        "format_padded",
                        [
                            ParseExpr::Number(w),
                            ParseExpr::Literal(f),
                            ParseExpr::Literal(a),
                        ],
                    ) => {
                        let align_res: Result<PadAlign, YamlError> = match a.as_str() {
                            "left" => Ok(PadAlign::Left),
                            "right" => Ok(PadAlign::Right),
                            "center" => Ok(PadAlign::Center),
                            _ => Err(YamlError::DslConversion {
                                message: format!(
                                    "format_padded align must be \"left\", \"right\", or \"center\", got {:?}",
                                    a
                                ),
                                span: span.into_range(),
                            }),
                        };
                        let mut fc = f.chars();
                        let fill_res: Result<char, YamlError> = match (fc.next(), fc.next()) {
                            (Some(c), None) => Ok(c),
                            _ => Err(YamlError::DslConversion {
                                message: "format_padded fill must be exactly one character"
                                    .to_string(),
                                span: span.into_range(),
                            }),
                        };
                        match (target_res, align_res, fill_res) {
                            (Ok(t), Ok(align), Ok(fill)) => Ok(Expr::FormatPadded {
                                target: Box::new(t),
                                width: *w as usize,
                                fill,
                                align,
                            }),
                            (t_res, a_res, f_res) => {
                                let mut errs = Vec::new();
                                if let Err(mut e) = t_res {
                                    errs.append(&mut e);
                                }
                                if let Err(e) = a_res {
                                    errs.push(e);
                                }
                                if let Err(e) = f_res {
                                    errs.push(e);
                                }
                                Err(errs)
                            }
                        }
                    }

                    _ => {
                        let mut errs = target_res.err().unwrap_or_default();
                        errs.push(YamlError::DslConversion {
                            message: format!("Unknown method '{}' or invalid arguments", name),
                            span: span.into_range(),
                        });
                        Err(errs)
                    }
                }
            }
        }
    }
}

/// Conversion from `SpannedParseExpr` — the type produced at the parse boundary.
///
/// This is the entry point for callers of `dsl::parse()`. The outer span is
/// used for accurate source positions in top-level errors (e.g. a bare map
/// literal outside `.lookup()`). All other variants delegate to the inner
/// `TryFrom<ParseExpr>` implementation which uses embedded spans where available.
impl TryFrom<SpannedParseExpr> for Expr {
    type Error = Vec<YamlError>;

    fn try_from(SpannedParseExpr(value, span): SpannedParseExpr) -> Result<Self, Self::Error> {
        if parse_node_count(&value) <= 32 {
            return match value {
                ParseExpr::MapLiteral(_) => Err(vec![YamlError::DslConversion {
                    message: "Map literals '{...}' can only be used inside .lookup()".to_string(),
                    span: span.into_range(),
                }]),
                other => Expr::try_from(other),
            };
        }
        lower_arena(value, span)
    }
}

fn parse_node_count(root: &ParseExpr) -> usize {
    let mut count = 0usize;
    let mut stack = vec![root];
    while let Some(expr) = stack.pop() {
        count += 1;
        match expr {
            ParseExpr::BinaryOperation { lhs, rhs, .. }
            | ParseExpr::Let {
                value: lhs,
                body: rhs,
                ..
            } => {
                stack.push(rhs);
                stack.push(lhs);
            }
            ParseExpr::If {
                condition,
                then,
                else_,
            } => {
                stack.push(else_);
                stack.push(then);
                stack.push(condition);
            }
            ParseExpr::List(items)
            | ParseExpr::Concat(items)
            | ParseExpr::Merge(items)
            | ParseExpr::JsonArray(items)
            | ParseExpr::Format { args: items, .. } => stack.extend(items.iter()),
            ParseExpr::MethodCall { target, args, .. } => {
                stack.extend(args.iter());
                stack.push(target);
            }
            ParseExpr::SelfRef
            | ParseExpr::Dom(_)
            | ParseExpr::Json(_)
            | ParseExpr::Var(_)
            | ParseExpr::Literal(_)
            | ParseExpr::Number(_)
            | ParseExpr::Bool(_)
            | ParseExpr::Null
            | ParseExpr::Pref(_)
            | ParseExpr::Scalar(_)
            | ParseExpr::MapLiteral(_)
            | ParseExpr::Index => {}
        }
    }
    count
}

enum LowerWork {
    Visit(ParseExpr),
    Binary(Op),
    Let(String),
    If,
    Many(ManyExprOp, usize),
    Format(String, usize),
    Method(String, usize, SimpleSpan),
}

fn lower_arena(value: ParseExpr, outer_span: SimpleSpan) -> Result<Expr, Vec<YamlError>> {
    let mut work = vec![LowerWork::Visit(value)];
    let mut results = Vec::<ExprId>::new();
    let mut nodes = Vec::<ExprNode>::new();

    while let Some(item) = work.pop() {
        match item {
            LowerWork::Visit(expr) => match expr {
                ParseExpr::SelfRef => push_node(&mut nodes, &mut results, ExprLeaf::SelfRef),
                ParseExpr::Dom(value) => {
                    push_node(&mut nodes, &mut results, ExprLeaf::Dom(value));
                }
                ParseExpr::Json(value) => {
                    push_node(&mut nodes, &mut results, ExprLeaf::Json(value));
                }
                ParseExpr::Var(value) => {
                    push_node(&mut nodes, &mut results, ExprLeaf::Var(value));
                }
                ParseExpr::Literal(value) => {
                    push_node(&mut nodes, &mut results, ExprLeaf::Literal(value));
                }
                ParseExpr::Number(value) => {
                    push_node(&mut nodes, &mut results, ExprLeaf::Number(value));
                }
                ParseExpr::Bool(value) => {
                    push_node(&mut nodes, &mut results, ExprLeaf::Bool(value));
                }
                ParseExpr::Null => push_node(&mut nodes, &mut results, ExprLeaf::Null),
                ParseExpr::Pref(value) => {
                    push_node(&mut nodes, &mut results, ExprLeaf::Pref(value));
                }
                ParseExpr::Scalar(value) => {
                    push_node(&mut nodes, &mut results, ExprLeaf::ScalarOverride(value));
                }
                ParseExpr::Index => push_node(&mut nodes, &mut results, ExprLeaf::Index),
                ParseExpr::MapLiteral(table) => {
                    let id = ExprId(nodes.len() as u32);
                    nodes.push(ExprNode::MapLiteral(table));
                    results.push(id);
                }
                ParseExpr::BinaryOperation { op, lhs, rhs } => {
                    work.push(LowerWork::Binary(op));
                    work.push(LowerWork::Visit(*rhs));
                    work.push(LowerWork::Visit(*lhs));
                }
                ParseExpr::Let { name, value, body } => {
                    work.push(LowerWork::Let(name));
                    work.push(LowerWork::Visit(*body));
                    work.push(LowerWork::Visit(*value));
                }
                ParseExpr::If {
                    condition,
                    then,
                    else_,
                } => {
                    work.push(LowerWork::If);
                    work.push(LowerWork::Visit(*else_));
                    work.push(LowerWork::Visit(*then));
                    work.push(LowerWork::Visit(*condition));
                }
                ParseExpr::List(items) => {
                    push_many_work(&mut work, ManyExprOp::List, items);
                }
                ParseExpr::Concat(items) => {
                    push_many_work(&mut work, ManyExprOp::Concat, items);
                }
                ParseExpr::Merge(items) => {
                    push_many_work(&mut work, ManyExprOp::Merge, items);
                }
                ParseExpr::JsonArray(items) => {
                    push_many_work(&mut work, ManyExprOp::JsonArray, items);
                }
                ParseExpr::Format { template, args } => {
                    let len = args.len();
                    work.push(LowerWork::Format(template, len));
                    for arg in args.into_iter().rev() {
                        work.push(LowerWork::Visit(arg));
                    }
                }
                ParseExpr::MethodCall {
                    target,
                    name,
                    args,
                    span,
                } => {
                    let len = args.len();
                    work.push(LowerWork::Method(name, len, span));
                    for arg in args.into_iter().rev() {
                        work.push(LowerWork::Visit(arg));
                    }
                    work.push(LowerWork::Visit(*target));
                }
            },
            LowerWork::Binary(op) => {
                let [lhs, rhs] = take_ids::<2>(&mut results);
                let id = ExprId(nodes.len() as u32);
                nodes.push(ExprNode::BinaryOperation { op, lhs, rhs });
                results.push(id);
            }
            LowerWork::Let(name) => {
                let [value, body] = take_ids::<2>(&mut results);
                let id = ExprId(nodes.len() as u32);
                nodes.push(ExprNode::Let { name, value, body });
                results.push(id);
            }
            LowerWork::If => {
                let [first, second, third] = take_ids::<3>(&mut results);
                let id = ExprId(nodes.len() as u32);
                nodes.push(ExprNode::Ternary {
                    op: TernaryExprOp::If,
                    first,
                    second,
                    third,
                });
                results.push(id);
            }
            LowerWork::Many(op, len) => {
                let items = results.split_off(results.len() - len);
                let id = ExprId(nodes.len() as u32);
                nodes.push(ExprNode::Many { op, items });
                results.push(id);
            }
            LowerWork::Format(template, len) => {
                let args = results.split_off(results.len() - len);
                let id = ExprId(nodes.len() as u32);
                nodes.push(ExprNode::Format { template, args });
                results.push(id);
            }
            LowerWork::Method(name, len, span) => {
                let values = results.split_off(results.len() - len - 1);
                let target = values[0];
                let args = &values[1..];
                let node = lower_method(&nodes, target, &name, args, span)?;
                let id = ExprId(nodes.len() as u32);
                nodes.push(node);
                results.push(id);
            }
        }
    }

    let Some(root) = results.pop() else {
        return Err(vec![YamlError::DslConversion {
            message: "expression produced no root node".to_string(),
            span: outer_span.into_range(),
        }]);
    };
    if !results.is_empty() || matches!(nodes[root.0 as usize], ExprNode::MapLiteral(_)) {
        return Err(vec![YamlError::DslConversion {
            message: "Map literals '{...}' can only be used inside .lookup()".to_string(),
            span: outer_span.into_range(),
        }]);
    }
    let arena = ExprArena { nodes };
    arena.validate(root).map_err(|message| {
        vec![YamlError::DslConversion {
            message,
            span: outer_span.into_range(),
        }]
    })?;
    Ok(Expr::Arena {
        arena: Arc::new(arena),
        root,
    })
}

fn push_node(nodes: &mut Vec<ExprNode>, results: &mut Vec<ExprId>, leaf: ExprLeaf) {
    let id = ExprId(nodes.len() as u32);
    nodes.push(ExprNode::Leaf(leaf));
    results.push(id);
}

fn push_many_work(work: &mut Vec<LowerWork>, op: ManyExprOp, items: Vec<ParseExpr>) {
    let len = items.len();
    work.push(LowerWork::Many(op, len));
    for item in items.into_iter().rev() {
        work.push(LowerWork::Visit(item));
    }
}

fn take_ids<const N: usize>(results: &mut Vec<ExprId>) -> [ExprId; N] {
    let values = results.split_off(results.len() - N);
    values
        .try_into()
        .unwrap_or_else(|_| unreachable!("lowering arity is statically known"))
}

fn literal(nodes: &[ExprNode], id: ExprId) -> Option<&str> {
    match nodes.get(id.0 as usize) {
        Some(ExprNode::Leaf(ExprLeaf::Literal(value))) => Some(value),
        _ => None,
    }
}

fn number(nodes: &[ExprNode], id: ExprId) -> Option<f64> {
    match nodes.get(id.0 as usize) {
        Some(ExprNode::Leaf(ExprLeaf::Number(value))) => Some(*value),
        _ => None,
    }
}

fn map_literal(nodes: &[ExprNode], id: ExprId) -> Option<&[(String, String)]> {
    match nodes.get(id.0 as usize) {
        Some(ExprNode::MapLiteral(value)) => Some(value),
        _ => None,
    }
}

fn lower_method(
    nodes: &[ExprNode],
    target: ExprId,
    name: &str,
    args: &[ExprId],
    span: SimpleSpan,
) -> Result<ExprNode, Vec<YamlError>> {
    let unary = |op| Ok(ExprNode::Unary { op, target });
    let binary = |op, rhs| {
        Ok(ExprNode::Binary {
            op,
            lhs: target,
            rhs,
        })
    };
    let invalid = || {
        Err(vec![YamlError::DslConversion {
            message: format!("Unknown method '{name}' or invalid arguments"),
            span: span.into_range(),
        }])
    };

    if let Some(function) = name.strip_prefix("__user::") {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(target);
        all.extend_from_slice(args);
        return Ok(ExprNode::UserFn {
            name: function.to_string(),
            args: all,
        });
    }

    match (name, args) {
        ("attr", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::Attr(value.to_string()))
            .map_or_else(invalid, unary),
        ("text", []) => unary(UnaryExprOp::Text),
        ("inner_html", []) => unary(UnaryExprOp::InnerHtml),
        ("select", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::Select(value.to_string()))
            .map_or_else(invalid, unary),
        ("first", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::First(value.to_string()))
            .map_or_else(invalid, unary),
        ("split", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::Split(value.to_string()))
            .map_or_else(invalid, unary),
        ("at", [arg]) => number(nodes, *arg)
            .map(|value| UnaryExprOp::At(value as i32))
            .map_or_else(invalid, unary),
        ("trim", []) => unary(UnaryExprOp::Trim),
        ("lower", []) => unary(UnaryExprOp::Lower),
        ("parse_float", []) => unary(UnaryExprOp::ParseFloat),
        ("parse_int", []) => unary(UnaryExprOp::ParseInt),
        ("to_string", []) => unary(UnaryExprOp::ToString),
        ("str", []) => unary(UnaryExprOp::JsonStr),
        ("int", []) => unary(UnaryExprOp::JsonInt),
        ("float", []) => unary(UnaryExprOp::JsonFloat),
        ("bool", []) => unary(UnaryExprOp::JsonBool),
        ("array_len", []) => unary(UnaryExprOp::ArrayLen),
        ("keys", []) => unary(UnaryExprOp::JsonKeys),
        ("json_fold", []) => unary(UnaryExprOp::JsonFold),
        ("children", []) => unary(UnaryExprOp::Children),
        ("date_parse_rfc3339", []) => unary(UnaryExprOp::DateParseRfc3339),
        ("not", []) => unary(UnaryExprOp::Not),
        ("string_len", []) => unary(UnaryExprOp::StringLen),
        ("slice", [start]) => number(nodes, *start)
            .map(|value| UnaryExprOp::Slice(value as i32, None))
            .map_or_else(invalid, unary),
        ("slice", [start, end]) => match (number(nodes, *start), number(nodes, *end)) {
            (Some(start), Some(end)) => unary(UnaryExprOp::Slice(start as i32, Some(end as i32))),
            _ => invalid(),
        },
        ("replace", [from, to]) => match (literal(nodes, *from), literal(nodes, *to)) {
            (Some(from), Some(to)) => unary(UnaryExprOp::Replace(from.into(), to.into())),
            _ => invalid(),
        },
        ("lookup", [table]) => map_literal(nodes, *table)
            .map(|table| ExprNode::Lookup {
                target,
                table: table.to_vec(),
            })
            .ok_or_else(|| invalid().expect_err("invalid always returns an error")),
        ("matches", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::Matches(value.into()))
            .map_or_else(invalid, unary),
        ("capture", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::Capture(value.into()))
            .map_or_else(invalid, unary),
        ("ptr", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::JsonPtr(value.into()))
            .map_or_else(invalid, unary),
        ("has_class", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::HasClass(value.into()))
            .map_or_else(invalid, unary),
        ("starts_with", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::StartsWith(value.into()))
            .map_or_else(invalid, unary),
        ("ends_with", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::EndsWith(value.into()))
            .map_or_else(invalid, unary),
        ("date_parse", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::DateParse(value.into()))
            .map_or_else(invalid, unary),
        ("join", [arg]) => literal(nodes, *arg)
            .map(|value| UnaryExprOp::Join(value.into()))
            .map_or_else(invalid, unary),
        ("map", [rhs]) => binary(BinaryExprOp::Map, *rhs),
        ("flat_map", [rhs]) => binary(BinaryExprOp::FlatMap, *rhs),
        ("filter", [rhs]) => binary(BinaryExprOp::Filter, *rhs),
        ("prepend", [rhs]) => binary(BinaryExprOp::Prepend, *rhs),
        ("append", [rhs]) => binary(BinaryExprOp::Append, *rhs),
        ("fallback", [rhs]) => binary(BinaryExprOp::Fallback, *rhs),
        ("resolve_url", [rhs]) => binary(BinaryExprOp::ResolveUrl, *rhs),
        ("get", [rhs]) => binary(BinaryExprOp::JsonGet, *rhs),
        ("sort_by", [rhs]) => binary(BinaryExprOp::SortBy, *rhs),
        ("fold", [base, transform]) => Ok(ExprNode::Ternary {
            op: TernaryExprOp::Fold,
            first: target,
            second: *base,
            third: *transform,
        }),
        ("find", [key, value]) => Ok(ExprNode::Ternary {
            op: TernaryExprOp::JsonFind,
            first: target,
            second: *key,
            third: *value,
        }),
        ("split_n", [delimiter, n]) => match (literal(nodes, *delimiter), number(nodes, *n)) {
            (Some(delimiter), Some(n)) => unary(UnaryExprOp::SplitN(delimiter.into(), n as usize)),
            _ => invalid(),
        },
        ("take", [arg]) => number(nodes, *arg)
            .map(|value| UnaryExprOp::Take(value as usize))
            .map_or_else(invalid, unary),
        ("skip", [arg]) => number(nodes, *arg)
            .map(|value| UnaryExprOp::Skip(value as usize))
            .map_or_else(invalid, unary),
        ("reverse", []) => unary(UnaryExprOp::Reverse),
        ("unique", []) => unary(UnaryExprOp::Unique),
        ("url_encode" | "urlencode", []) => unary(UnaryExprOp::UrlEncode),
        ("url_decode" | "urldecode", []) => unary(UnaryExprOp::UrlDecode),
        ("format_padded", [width, fill, align]) => {
            let Some(width) = number(nodes, *width) else {
                return invalid();
            };
            let Some(fill) = literal(nodes, *fill) else {
                return invalid();
            };
            let Some(align) = literal(nodes, *align) else {
                return invalid();
            };
            let align = match align {
                "left" => PadAlign::Left,
                "right" => PadAlign::Right,
                "center" => PadAlign::Center,
                value => {
                    return Err(vec![YamlError::DslConversion {
                        message: format!(
                            "format_padded align must be \"left\", \"right\", or \"center\", got {value:?}"
                        ),
                        span: span.into_range(),
                    }]);
                }
            };
            let mut chars = fill.chars();
            let (Some(fill), None) = (chars.next(), chars.next()) else {
                return Err(vec![YamlError::DslConversion {
                    message: "format_padded fill must be exactly one character".to_string(),
                    span: span.into_range(),
                }]);
            };
            unary(UnaryExprOp::FormatPadded {
                width: width as usize,
                fill,
                align,
            })
        }
        _ => invalid(),
    }
}
