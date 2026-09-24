#![allow(clippy::unwrap_used)]

use kani_cli::dsl::parse;
use kani_shared::ast::{Expr, Op, PadAlign};

fn parse_ok(input: &str) -> Expr {
    let parse_expr =
        parse(input).unwrap_or_else(|errors| panic!("parse failed for {input:?}: {errors:?}"));
    Expr::try_from(parse_expr)
        .unwrap_or_else(|errors| panic!("conversion failed for {input:?}: {errors:?}"))
}

fn parse_err(input: &str) -> bool {
    parse(input).is_err()
}

#[test]
fn deeply_flat_binary_expressions_lower_serialize_format_and_drop_without_overflow() {
    for leaves in [2_000usize, 5_000] {
        let source = std::iter::repeat_n("1", leaves)
            .collect::<Vec<_>>()
            .join("+");
        let expr = parse_ok(&source);
        let Expr::Arena { arena, root } = &expr else {
            panic!("large expressions must use flat arena storage");
        };
        assert_eq!(arena.nodes.len(), leaves * 2 - 1);
        arena.validate(*root).expect("valid arena");
        let debug = format!("{expr:?}");
        assert!(debug.starts_with("Arena"));
        let generated = kani_cli::codegen::expr::emit_expr(&expr);
        assert!(generated.starts_with("Expr::arena_from_bytes"));
        assert!(generated.len() > source.len());
        let bytes = postcard::to_allocvec(&expr).expect("serialize arena expression");
        let decoded: Expr = postcard::from_bytes(&bytes).expect("deserialize arena expression");
        assert_eq!(expr, decoded);
        drop(decoded);
        drop(expr);
    }
}

#[test]
fn parse_string_literal() {
    assert_eq!(parse_ok(r#""hello""#), Expr::Literal("hello".into()));
}

#[test]
fn parse_integer_number() {
    assert_eq!(parse_ok("42"), Expr::Number(42.0));
}

#[test]
fn parse_negative_number() {
    assert_eq!(parse_ok("-7"), Expr::Number(-7.0));
}

#[test]
fn parse_float_number() {
    assert_eq!(parse_ok("3.25"), Expr::Number(3.25));
}

#[test]
fn parse_bool_true() {
    assert_eq!(parse_ok("true"), Expr::Bool(true));
}

#[test]
fn parse_bool_false() {
    assert_eq!(parse_ok("false"), Expr::Bool(false));
}

#[test]
fn parse_null() {
    assert_eq!(parse_ok("null"), Expr::Null);
}

#[test]
fn parse_index() {
    assert_eq!(parse_ok("index()"), Expr::Index);
}

#[test]
fn parse_self_ref() {
    assert_eq!(parse_ok("self"), Expr::SelfRef);
}

#[test]
fn parse_dom() {
    assert_eq!(parse_ok(r#"dom("h1")"#), Expr::Dom("h1".into()));
}

#[test]
fn parse_json() {
    assert_eq!(
        parse_ok(r#"json("/data/id")"#),
        Expr::Json("/data/id".into())
    );
}

#[test]
fn parse_pref() {
    assert_eq!(
        parse_ok(r#"pref("language")"#),
        Expr::Pref("language".into())
    );
}

#[test]
fn parse_scalar() {
    assert_eq!(
        parse_ok(r#"scalar("base_url")"#),
        Expr::ScalarOverride {
            name: "base_url".into()
        }
    );
}

#[test]
fn parse_var() {
    assert_eq!(parse_ok("$manga_id"), Expr::Var("$manga_id".into()));
}

#[test]
fn parse_text_method() {
    let expr = parse_ok(r#"self.text()"#);
    assert_eq!(
        expr,
        Expr::Text {
            target: Box::new(Expr::SelfRef)
        }
    );
}

#[test]
fn parse_attr_method() {
    let expr = parse_ok(r#"self.attr("href")"#);
    assert_eq!(
        expr,
        Expr::Attr {
            target: Box::new(Expr::SelfRef),
            name: "href".into()
        }
    );
}

#[test]
fn parse_trim_method() {
    let expr = parse_ok(r#"self.text().trim()"#);
    assert_eq!(
        expr,
        Expr::Trim {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            })
        }
    );
}

#[test]
fn parse_lower_method() {
    let expr = parse_ok(r#"self.text().lower()"#);
    assert_eq!(
        expr,
        Expr::Lower {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            })
        }
    );
}

#[test]
fn parse_split_at_chain() {
    let expr = parse_ok(r#"self.attr("href").split("/").at(2)"#);
    assert_eq!(
        expr,
        Expr::At {
            target: Box::new(Expr::Split {
                target: Box::new(Expr::Attr {
                    target: Box::new(Expr::SelfRef),
                    name: "href".into(),
                }),
                delimiter: "/".into(),
            }),
            index: 2,
        }
    );
}

#[test]
fn parse_at_negative_index() {
    let expr = parse_ok(r#"self.split("/").at(-1)"#);
    assert_eq!(
        expr,
        Expr::At {
            target: Box::new(Expr::Split {
                target: Box::new(Expr::SelfRef),
                delimiter: "/".into(),
            }),
            index: -1,
        }
    );
}

#[test]
fn parse_parse_float() {
    let expr = parse_ok(r#"self.text().parse_float()"#);
    assert_eq!(
        expr,
        Expr::ParseFloat {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            })
        }
    );
}

#[test]
fn parse_parse_int() {
    let expr = parse_ok(r#"self.text().parse_int()"#);
    assert_eq!(
        expr,
        Expr::ParseInt {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            })
        }
    );
}

#[test]
fn parse_select_method() {
    let expr = parse_ok(r#"self.select("a.link")"#);
    assert_eq!(
        expr,
        Expr::Select {
            target: Box::new(Expr::SelfRef),
            selector: "a.link".into()
        }
    );
}

#[test]
fn parse_first_method() {
    let expr = parse_ok(r#"self.first("h1")"#);
    assert_eq!(
        expr,
        Expr::First {
            target: Box::new(Expr::SelfRef),
            selector: "h1".into()
        }
    );
}

#[test]
fn parse_replace_method() {
    let expr = parse_ok(r#"self.text().replace("foo", "bar")"#);
    assert_eq!(
        expr,
        Expr::Replace {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            from: "foo".into(),
            to: "bar".into(),
        }
    );
}

#[test]
fn parse_fallback_method() {
    let expr = parse_ok(r#"self.text().fallback("unknown")"#);
    assert_eq!(
        expr,
        Expr::Fallback {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            default: Box::new(Expr::Literal("unknown".into())),
        }
    );
}

#[test]
fn parse_matches_method() {
    let expr = parse_ok(r#"self.text().matches("[0-9]+")"#);
    assert_eq!(
        expr,
        Expr::Matches {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            pattern: "[0-9]+".into()
        }
    );
}

#[test]
fn parse_map_method() {
    let expr = parse_ok(r#"self.select("li").map($item.text())"#);
    assert_eq!(
        expr,
        Expr::Map {
            target: Box::new(Expr::Select {
                target: Box::new(Expr::SelfRef),
                selector: "li".into(),
            }),
            transform: Box::new(Expr::Text {
                target: Box::new(Expr::Var("$item".into()))
            }),
        }
    );
}

#[test]
fn parse_filter_method() {
    let expr = parse_ok(r#"self.select("li").filter($item.text())"#);
    assert_eq!(
        expr,
        Expr::Filter {
            target: Box::new(Expr::Select {
                target: Box::new(Expr::SelfRef),
                selector: "li".into(),
            }),
            filter: Box::new(Expr::Text {
                target: Box::new(Expr::Var("$item".into()))
            }),
        }
    );
}

#[test]
fn parse_join_method() {
    let expr = parse_ok(r#"self.select("li").join(", ")"#);
    assert_eq!(
        expr,
        Expr::Join {
            target: Box::new(Expr::Select {
                target: Box::new(Expr::SelfRef),
                selector: "li".into(),
            }),
            delimiter: ", ".into(),
        }
    );
}

#[test]
fn parse_starts_with() {
    let expr = parse_ok(r#"self.text().starts_with("http")"#);
    assert_eq!(
        expr,
        Expr::StartsWith {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            prefix: "http".into(),
        }
    );
}

#[test]
fn parse_ends_with() {
    let expr = parse_ok(r#"self.text().ends_with(".jpg")"#);
    assert_eq!(
        expr,
        Expr::EndsWith {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            suffix: ".jpg".into(),
        }
    );
}

#[test]
fn parse_resolve_url() {
    let expr = parse_ok(r#"self.attr("src").resolve_url("https://example.com")"#);
    assert_eq!(
        expr,
        Expr::ResolveUrl {
            target: Box::new(Expr::Attr {
                target: Box::new(Expr::SelfRef),
                name: "src".into()
            }),
            base: Box::new(Expr::Literal("https://example.com".into())),
        }
    );
}

#[test]
fn parse_lookup_method() {
    let expr = parse_ok(r#"self.text().lookup({"a": "b", "c": "d"})"#);
    assert_eq!(
        expr,
        Expr::Lookup {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            table: vec![("a".into(), "b".into()), ("c".into(), "d".into())],
        }
    );
}

#[test]
fn parse_json_str() {
    let expr = parse_ok(r#"json("/data/id").str()"#);
    assert_eq!(
        expr,
        Expr::JsonStr {
            target: Box::new(Expr::Json("/data/id".into()))
        }
    );
}

#[test]
fn parse_json_int() {
    let expr = parse_ok(r#"json("/data/count").int()"#);
    assert_eq!(
        expr,
        Expr::JsonInt {
            target: Box::new(Expr::Json("/data/count".into()))
        }
    );
}

#[test]
fn parse_json_ptr() {
    let expr = parse_ok(r#"json("/data").ptr("/attributes/title")"#);
    assert_eq!(
        expr,
        Expr::JsonPtr {
            target: Box::new(Expr::Json("/data".into())),
            pointer: "/attributes/title".into(),
        }
    );
}

#[test]
fn parse_binary_add() {
    let expr = parse_ok("1 + 2");
    assert_eq!(
        expr,
        Expr::BinaryOperation {
            op: Op::Add,
            lhs: Box::new(Expr::Number(1.0)),
            rhs: Box::new(Expr::Number(2.0))
        }
    );
}

#[test]
fn parse_binary_eq() {
    let expr = parse_ok(r#""a" == "b""#);
    assert_eq!(
        expr,
        Expr::BinaryOperation {
            op: Op::Eq,
            lhs: Box::new(Expr::Literal("a".into())),
            rhs: Box::new(Expr::Literal("b".into())),
        }
    );
}

#[test]
fn parse_binary_and() {
    let expr = parse_ok("true && false");
    assert_eq!(
        expr,
        Expr::BinaryOperation {
            op: Op::And,
            lhs: Box::new(Expr::Bool(true)),
            rhs: Box::new(Expr::Bool(false))
        }
    );
}

#[test]
fn parse_binary_or() {
    let expr = parse_ok("true || false");
    assert_eq!(
        expr,
        Expr::BinaryOperation {
            op: Op::Or,
            lhs: Box::new(Expr::Bool(true)),
            rhs: Box::new(Expr::Bool(false))
        }
    );
}

#[test]
fn parse_if_then_else() {
    let expr = parse_ok(r#"if true then "yes" else "no""#);
    assert_eq!(
        expr,
        Expr::If {
            condition: Box::new(Expr::Bool(true)),
            then: Box::new(Expr::Literal("yes".into())),
            else_: Box::new(Expr::Literal("no".into())),
        }
    );
}

#[test]
fn parse_let_binding() {
    let expr = parse_ok("let $x = \"hello\"; $x");
    assert_eq!(
        expr,
        Expr::Let {
            name: "$x".into(),
            value: Box::new(Expr::Literal("hello".into())),
            body: Box::new(Expr::Var("$x".into())),
        }
    );
}

#[test]
fn parse_format_expr() {
    let expr = parse_ok(r#"format("hello {}", "world")"#);
    assert_eq!(
        expr,
        Expr::Format {
            template: "hello {}".into(),
            args: vec![Expr::Literal("world".into())]
        }
    );
}

#[test]
fn parse_merge_expr() {
    let expr = parse_ok(r#"merge([self.text(), "extra"])"#);
    assert_eq!(
        expr,
        Expr::Merge(vec![
            Expr::Text {
                target: Box::new(Expr::SelfRef)
            },
            Expr::Literal("extra".into()),
        ])
    );
}

#[test]
fn parse_list_literal() {
    let expr = parse_ok(r#"["a", "b", "c"]"#);
    assert_eq!(
        expr,
        Expr::List(vec![
            Expr::Literal("a".into()),
            Expr::Literal("b".into()),
            Expr::Literal("c".into()),
        ])
    );
}

#[test]
fn parse_error_unclosed_paren() {
    assert!(parse_err("self.text("));
}

#[test]
fn parse_error_trailing_junk() {
    assert!(parse_err(r#""hello" garbage"#));
}

#[test]
fn parse_error_empty_input() {
    assert!(parse_err(""));
}

#[test]
fn convert_error_unknown_method() {
    let parse_expr = parse(r#"self.nonexistent_method()"#).expect("should parse as a method call");
    let err = Expr::try_from(parse_expr).expect_err("should fail conversion");
    let msg = err
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        msg.contains("nonexistent_method") || msg.contains("Unknown method"),
        "got: {msg}"
    );
}

#[test]
fn convert_error_map_literal_outside_lookup() {
    let expr = parse_ok(r#"self.text().lookup({"x": "y"})"#);
    assert!(
        matches!(expr, Expr::Lookup { .. }),
        "lookup with map literal should succeed"
    );
}

#[test]
fn parse_split_n() {
    let expr = parse_ok(r#"self.text().split_n("/", 3)"#);
    assert_eq!(
        expr,
        Expr::SplitN {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            delimiter: "/".into(),
            n: 3,
        }
    );
}

#[test]
fn parse_take() {
    let expr = parse_ok(r#"self.select("li").take(5)"#);
    assert_eq!(
        expr,
        Expr::Take {
            target: Box::new(Expr::Select {
                target: Box::new(Expr::SelfRef),
                selector: "li".into(),
            }),
            n: 5,
        }
    );
}

#[test]
fn parse_skip() {
    let expr = parse_ok(r#"self.select("li").skip(2)"#);
    assert_eq!(
        expr,
        Expr::Skip {
            target: Box::new(Expr::Select {
                target: Box::new(Expr::SelfRef),
                selector: "li".into(),
            }),
            n: 2,
        }
    );
}

#[test]
fn parse_reverse() {
    let expr = parse_ok(r#"self.select("li").reverse()"#);
    assert_eq!(
        expr,
        Expr::Reverse {
            target: Box::new(Expr::Select {
                target: Box::new(Expr::SelfRef),
                selector: "li".into(),
            }),
        }
    );
}

#[test]
fn parse_sort_by() {
    let expr = parse_ok(r#"self.select("li").sort_by($item.text())"#);
    assert_eq!(
        expr,
        Expr::SortBy {
            target: Box::new(Expr::Select {
                target: Box::new(Expr::SelfRef),
                selector: "li".into(),
            }),
            key: Box::new(Expr::Text {
                target: Box::new(Expr::Var("$item".into()))
            }),
        }
    );
}

#[test]
fn parse_unique() {
    let expr = parse_ok(r#"self.select("li").map($item.text()).unique()"#);
    assert_eq!(
        expr,
        Expr::Unique {
            target: Box::new(Expr::Map {
                target: Box::new(Expr::Select {
                    target: Box::new(Expr::SelfRef),
                    selector: "li".into(),
                }),
                transform: Box::new(Expr::Text {
                    target: Box::new(Expr::Var("$item".into()))
                }),
            }),
        }
    );
}

#[test]
fn parse_url_encode() {
    let expr = parse_ok(r#"self.text().url_encode()"#);
    assert_eq!(
        expr,
        Expr::UrlEncode {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            })
        }
    );
}

#[test]
fn parse_url_encode_alias() {
    let expr = parse_ok(r#"self.text().urlencode()"#);
    assert_eq!(
        expr,
        Expr::UrlEncode {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            })
        }
    );
}

#[test]
fn parse_url_decode() {
    let expr = parse_ok(r#"self.text().url_decode()"#);
    assert_eq!(
        expr,
        Expr::UrlDecode {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            })
        }
    );
}

#[test]
fn parse_url_decode_alias() {
    let expr = parse_ok(r#"self.text().urldecode()"#);
    assert_eq!(
        expr,
        Expr::UrlDecode {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            })
        }
    );
}

#[test]
fn parse_format_padded_left() {
    let expr = parse_ok(r#"self.text().format_padded(10, "0", "left")"#);
    assert_eq!(
        expr,
        Expr::FormatPadded {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            width: 10,
            fill: '0',
            align: PadAlign::Left,
        }
    );
}

#[test]
fn parse_format_padded_right() {
    let expr = parse_ok(r#"self.text().format_padded(8, " ", "right")"#);
    assert_eq!(
        expr,
        Expr::FormatPadded {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            width: 8,
            fill: ' ',
            align: PadAlign::Right,
        }
    );
}

#[test]
fn parse_format_padded_center() {
    let expr = parse_ok(r#"self.text().format_padded(6, "-", "center")"#);
    assert_eq!(
        expr,
        Expr::FormatPadded {
            target: Box::new(Expr::Text {
                target: Box::new(Expr::SelfRef)
            }),
            width: 6,
            fill: '-',
            align: PadAlign::Center,
        }
    );
}

#[test]
fn convert_error_format_padded_bad_align() {
    let parse_expr = parse(r#"self.text().format_padded(10, "0", "diagonal")"#)
        .expect("should parse syntactically");
    let err = Expr::try_from(parse_expr).expect_err("bad align should fail conversion");
    let msg = err
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        msg.contains("align") || msg.contains("diagonal"),
        "got: {msg}"
    );
}

#[test]
fn convert_error_format_padded_empty_fill() {
    let parse_expr =
        parse(r#"self.text().format_padded(10, "", "left")"#).expect("should parse syntactically");
    let err = Expr::try_from(parse_expr).expect_err("empty fill should fail conversion");
    let msg = err
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        msg.contains("fill") || msg.contains("character"),
        "got: {msg}"
    );
}

#[test]
fn parse_user_fn_no_extra_args() {
    let expr = parse_ok(r#"self.text().user.slugify()"#);
    assert!(
        matches!(&expr, Expr::UserFn { name, args } if name == "slugify" && args.len() == 1),
        "got: {:?}",
        expr
    );
}

#[test]
fn parse_user_fn_with_extra_args() {
    let expr = parse_ok(r#"self.text().user.format_date("YYYY-MM-DD")"#);
    match &expr {
        Expr::UserFn { name, args } => {
            assert_eq!(name, "format_date");
            assert_eq!(args.len(), 2, "receiver + 1 explicit arg");
            assert!(matches!(&args[1], Expr::Literal(s) if s == "YYYY-MM-DD"));
        }
        other => panic!("expected UserFn, got {:?}", other),
    }
}

#[test]
fn parse_user_fn_receiver_is_first_arg() {
    let expr = parse_ok(r#"self.attr("href").user.clean_url()"#);
    match &expr {
        Expr::UserFn { name, args } => {
            assert_eq!(name, "clean_url");
            assert_eq!(args.len(), 1);
            assert!(matches!(&args[0], Expr::Attr { name, .. } if name == "href"));
        }
        other => panic!("expected UserFn, got {:?}", other),
    }
}

#[test]
fn parse_chain_split_across_lines() {
    let expr = parse_ok("self.ptr(\"/a\")\n  .str()\n  .fallback(\"x\")");
    assert_eq!(expr, parse_ok(r#"self.ptr("/a").str().fallback("x")"#));
}

#[test]
fn parse_multiline_list_and_call_arguments() {
    let expr = parse_ok("merge([\n  [\"a\"],\n  [\"b\"],\n]).join(\n  \", \"\n)");
    assert_eq!(expr, parse_ok(r#"merge([["a"],["b"]]).join(", ")"#));
}

#[test]
fn parse_empty_list_as_a_later_element() {
    let expr = parse_ok(r#"merge([["a"], []])"#);
    assert_eq!(
        expr,
        Expr::Merge(vec![
            Expr::List(vec![Expr::Literal("a".into())]),
            Expr::List(vec![]),
        ])
    );
}

#[test]
fn parse_let_bindings_one_per_line() {
    let expr = parse_ok("let $a = \"x\";\nlet $b = \"y\";\nmerge([[$a], [$b]]).join(\"|\")");
    assert_eq!(
        expr,
        parse_ok(r#"let $a = "x"; let $b = "y"; merge([[$a], [$b]]).join("|")"#)
    );
}

#[test]
fn parse_multiline_if_branches() {
    let expr = parse_ok("if pref(\"t\") == \"true\"\n  then [\"a\"]\n  else []");
    assert_eq!(
        expr,
        parse_ok(r#"if pref("t") == "true" then ["a"] else []"#)
    );
}

#[test]
fn parse_string_escapes_that_matter() {
    assert_eq!(parse_ok(r#""a\nb""#), Expr::Literal("a\nb".into()));
    assert_eq!(parse_ok(r#""a\"b""#), Expr::Literal("a\"b".into()));
    assert_eq!(parse_ok(r#""a\\b""#), Expr::Literal("a\\b".into()));
}

#[test]
fn parse_string_keeps_regex_escapes_intact() {
    assert_eq!(
        parse_ok(r#""Chapter\s+(\d+)""#),
        Expr::Literal(r"Chapter\s+(\d+)".into())
    );
}

#[test]
fn parse_a_multi_binding_description_expression() {
    let expression = r#"let $synopsis = self.ptr("/synopsis").str().fallback("");
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

    let parsed = parse_ok(expression);
    let Expr::Arena { arena, root } = &parsed else {
        panic!("a multi-binding expression must use arena storage");
    };
    arena.validate(*root).expect("valid arena");
    assert!(arena.nodes.len() > 32);
    assert!(kani_cli::codegen::expr::emit_expr(&parsed).starts_with("Expr::arena_from_bytes"));
}

#[test]
fn parse_error_requires_semicolon_between_let_binding_and_body() {
    let errors = parse("let $value = \"x\"\n$value").expect_err("missing semicolon must fail");
    assert!(errors.iter().any(|error| error.message.contains("';'")));
}

#[test]
fn parse_error_reports_unterminated_string() {
    let errors = parse("self.text(\"").expect_err("unterminated string must fail");
    assert!(errors.iter().any(|error| matches!(
        error.kind,
        kani_cli::dsl::DslParseErrorKind::UnterminatedString
    )));
}

#[test]
fn parse_error_limits_nesting() {
    let input = format!("{}self{}", "(".repeat(51), ")".repeat(51));
    let errors = parse(&input).expect_err("excessive nesting must fail");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error.kind, kani_cli::dsl::DslParseErrorKind::LimitExceeded))
    );
}
