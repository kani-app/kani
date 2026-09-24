#![allow(clippy::approx_constant)]
#![allow(clippy::unwrap_used)]

#[cfg(test)]
mod helpers {
    use crate::evaluator::html_eval::extract_html;
    use crate::evaluator::json_eval::extract_json;
    use crate::wasm::{HostState, SendHtml, StoredNode};
    use kani_shared::ast::*;

    pub fn lit(s: &str) -> Expr {
        Expr::Literal(s.into())
    }
    pub fn num(n: f64) -> Expr {
        Expr::Number(n)
    }
    pub fn var(name: &str) -> Expr {
        Expr::Var(name.into())
    }

    pub(super) fn binop(op: Op, lhs: Expr, rhs: Expr) -> Expr {
        Expr::BinaryOperation {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    pub fn field(name: &str, expr: Expr) -> FieldDef {
        FieldDef {
            name: name.into(),
            expr,
            optional: false,
        }
    }

    pub(super) fn opt_field(name: &str, expr: Expr) -> FieldDef {
        FieldDef {
            name: name.into(),
            expr,
            optional: true,
        }
    }

    pub fn bind(name: &str, expr: Expr) -> Binding {
        Binding {
            name: name.into(),
            expr,
        }
    }

    pub(super) async fn json_rows(
        json: &str,
        container: &str,
        fields: Vec<FieldDef>,
        bindings: Vec<Binding>,
    ) -> Vec<serde_json::Value> {
        let mut state = HostState::default();
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let handle = state.next_doc_handle;
        state.next_doc_handle += 1;
        state.json_docs.insert(handle, value);
        let bp = Blueprint {
            request: None,
            container: container.into(),
            fields,
            bindings,
            scalars: vec![],
            pagination: None,
        };
        let out = extract_json(&mut state, Some(handle), &bp).await.unwrap();
        out["rows"].as_array().unwrap().clone()
    }

    /// Evaluate a single expression against `{}` as a root container.
    pub async fn json_eval(expr: Expr) -> serde_json::Value {
        json_rows("{}", "", vec![field("v", expr)], vec![]).await[0]["v"].clone()
    }

    pub(super) async fn json_eval_opt(expr: Expr) -> serde_json::Value {
        json_rows("{}", "", vec![opt_field("v", expr)], vec![]).await[0]["v"].clone()
    }

    /// Evaluate and expect an error, returning it.
    pub(super) async fn json_eval_err(expr: Expr) -> String {
        let mut state = HostState::default();
        let doc: serde_json::Value = serde_json::from_str("{}").unwrap();
        state.json_docs.insert(1, doc);
        state.next_doc_handle = 2;
        let bp = Blueprint {
            request: None,
            container: "".into(),
            fields: vec![field("v", expr)],
            bindings: vec![],
            scalars: vec![],
            pagination: None,
        };
        extract_json(&mut state, Some(1), &bp).await.unwrap_err()
    }

    pub(super) async fn html_rows(
        html: &str,
        container: &str,
        fields: Vec<FieldDef>,
        bindings: Vec<Binding>,
    ) -> Vec<serde_json::Value> {
        let mut state = HostState::default();
        let parsed = SendHtml::parse_document(html);
        let root_id = parsed.0.lock().unwrap().0.root_element().id();
        let handle = state.next_doc_handle;
        state.next_doc_handle += 1;
        state.html_docs.insert(
            handle,
            StoredNode {
                doc: parsed.0,
                node_id: root_id,
            },
        );
        let bp = Blueprint {
            request: None,
            container: container.into(),
            fields,
            bindings,
            scalars: vec![],
            pagination: None,
        };
        let out = extract_html(&mut state, Some(handle), &bp).await.unwrap();
        out["rows"].as_array().unwrap().clone()
    }

    pub(super) const MANGA_HTML: &str = r#"
        <html><body>
          <div class="container">
            <article class="manga-card active" data-id="manga-123">
              <img src="/covers/123.jpg" data-src="https://cdn.example.com/123.jpg" alt="Cover A">
              <h2 class="title">Test Manga</h2>
              <p class="desc">A great manga</p>
              <span class="status">ongoing</span>
              <div class="tags">
                <a class="tag" href="/tag/action">Action</a>
                <a class="tag" href="/tag/adventure">Adventure</a>
              </div>
              <span class="chapter">Chapter 42</span>
            </article>
            <article class="manga-card" data-id="manga-456">
              <img src="/covers/456.jpg" alt="Cover B">
              <h2 class="title">Another Title</h2>
              <p class="desc">Another manga</p>
              <span class="status">completed</span>
              <div class="tags">
                <a class="tag" href="/tag/romance">Romance</a>
              </div>
              <span class="chapter">Chapter 1.5</span>
            </article>
          </div>
        </body></html>
    "#;

    pub(super) const MANGA_JSON: &str = r#"
    {
      "data": [
        {
          "id": "manga-1",
          "attributes": {
            "title": {"en": "Test Manga", "ja": "テスト漫画"},
            "status": "ongoing",
            "chapterCount": 42,
            "rating": 8.5,
            "adult": false
          }
        },
        {
          "id": "manga-2",
          "attributes": {
            "title": {"en": "Another Manga"},
            "status": "completed",
            "chapterCount": 10,
            "rating": 7.2,
            "adult": true
          }
        }
      ],
      "relationships": [
        {"type": "cover_art", "id": "cov-1", "attributes": {"fileName": "cover1.jpg"}},
        {"type": "author",   "id": "auth-1", "attributes": {"name": "John Doe"}}
      ],
      "meta": {"total": 2}
    }
    "#;
}

#[cfg(test)]
mod shared_tests {
    use super::helpers::*;
    use kani_shared::ast::*;

    #[tokio::test]
    async fn literal_string() {
        assert_eq!(json_eval(lit("hello")).await, "hello");
    }

    #[tokio::test]
    async fn literal_number() {
        let v = json_eval(num(3.14)).await;
        assert!((v.as_f64().unwrap() - 3.14).abs() < 1e-9);
    }

    #[tokio::test]
    async fn null_optional_field() {
        let rows = json_rows("{}", "", vec![opt_field("v", Expr::Null)], vec![]).await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn let_binds_and_var_reads() {
        let expr = Expr::Let {
            name: "$x".into(),
            value: Box::new(lit("world")),
            body: Box::new(Expr::Prepend {
                target: Box::new(lit(" world")),
                prefix: Box::new(var("$x")),
            }),
        };
        assert_eq!(json_eval(expr).await, "world world");
    }

    #[tokio::test]
    async fn undefined_var_errors() {
        let err = json_eval_err(var("$nope")).await;
        assert!(err.contains("Undefined variable"));
    }

    #[tokio::test]
    async fn add_floats() {
        let v = json_eval(binop(Op::Add, num(10.0), num(5.0))).await;
        assert_eq!(v.as_f64().unwrap(), 15.0);
    }

    #[tokio::test]
    async fn sub() {
        let v = json_eval(binop(Op::Sub, num(10.0), num(3.0))).await;
        assert_eq!(v.as_f64().unwrap(), 7.0);
    }

    #[tokio::test]
    async fn mul() {
        let v = json_eval(binop(Op::Mul, num(4.0), num(3.0))).await;
        assert_eq!(v.as_f64().unwrap(), 12.0);
    }

    #[tokio::test]
    async fn div() {
        let v = json_eval(binop(Op::Div, num(10.0), num(4.0))).await;
        assert_eq!(v.as_f64().unwrap(), 2.5);
    }

    #[tokio::test]
    async fn div_by_zero_errors() {
        let err = json_eval_err(binop(Op::Div, num(1.0), num(0.0))).await;
        assert!(err.contains("division by zero"));
    }

    #[tokio::test]
    async fn eq_equal() {
        assert_eq!(json_eval(binop(Op::Eq, lit("a"), lit("a"))).await, true);
    }

    #[tokio::test]
    async fn eq_unequal() {
        assert_eq!(json_eval(binop(Op::Eq, lit("a"), lit("b"))).await, false);
    }

    #[tokio::test]
    async fn ne() {
        assert_eq!(json_eval(binop(Op::Ne, lit("a"), lit("b"))).await, true);
    }

    #[tokio::test]
    async fn lt_true() {
        assert_eq!(json_eval(binop(Op::Lt, num(1.0), num(2.0))).await, true);
    }

    #[tokio::test]
    async fn gt_true() {
        assert_eq!(json_eval(binop(Op::Gt, num(5.0), num(3.0))).await, true);
    }

    #[tokio::test]
    async fn le_equal() {
        assert_eq!(json_eval(binop(Op::Le, num(3.0), num(3.0))).await, true);
    }

    #[tokio::test]
    async fn ge_greater() {
        assert_eq!(json_eval(binop(Op::Ge, num(4.0), num(3.0))).await, true);
    }

    #[tokio::test]
    async fn and_truth_table() {
        let tt = json_eval(binop(
            Op::And,
            binop(Op::Eq, lit("a"), lit("a")),
            binop(Op::Eq, lit("b"), lit("b")),
        ))
        .await;
        let tf = json_eval(binop(
            Op::And,
            binop(Op::Eq, lit("a"), lit("a")),
            binop(Op::Eq, lit("a"), lit("b")),
        ))
        .await;
        let ff = json_eval(binop(
            Op::And,
            binop(Op::Eq, lit("a"), lit("b")),
            binop(Op::Eq, lit("a"), lit("b")),
        ))
        .await;
        assert_eq!((tt, tf, ff), (true.into(), false.into(), false.into()));
    }

    #[tokio::test]
    async fn or_truth_table() {
        let ft = json_eval(binop(
            Op::Or,
            binop(Op::Eq, lit("a"), lit("b")),
            binop(Op::Eq, lit("a"), lit("a")),
        ))
        .await;
        let ff = json_eval(binop(
            Op::Or,
            binop(Op::Eq, lit("a"), lit("b")),
            binop(Op::Eq, lit("c"), lit("d")),
        ))
        .await;
        assert_eq!((ft, ff), (true.into(), false.into()));
    }

    #[tokio::test]
    async fn split_and_at_positive() {
        let expr = Expr::At {
            target: Box::new(Expr::Split {
                target: Box::new(lit("a/b/c")),
                delimiter: "/".into(),
            }),
            index: 1,
        };
        assert_eq!(json_eval(expr).await, "b");
    }

    #[tokio::test]
    async fn at_negative_index() {
        let expr = Expr::At {
            target: Box::new(Expr::Split {
                target: Box::new(lit("a/b/c")),
                delimiter: "/".into(),
            }),
            index: -1,
        };
        assert_eq!(json_eval(expr).await, "c");
    }

    #[tokio::test]
    async fn at_out_of_bounds_is_null() {
        let rows = json_rows(
            "{}",
            "",
            vec![opt_field(
                "v",
                Expr::At {
                    target: Box::new(Expr::Split {
                        target: Box::new(lit("a/b")),
                        delimiter: "/".into(),
                    }),
                    index: 10,
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn replace_all_occurrences() {
        let expr = Expr::Replace {
            target: Box::new(lit("aXbXc")),
            from: "X".into(),
            to: "-".into(),
        };
        assert_eq!(json_eval(expr).await, "a-b-c");
    }

    #[tokio::test]
    async fn trim() {
        assert_eq!(
            json_eval(Expr::Trim {
                target: Box::new(lit("  hello  "))
            })
            .await,
            "hello"
        );
    }

    #[tokio::test]
    async fn lower() {
        assert_eq!(
            json_eval(Expr::Lower {
                target: Box::new(lit("HELLO World"))
            })
            .await,
            "hello world"
        );
    }

    #[tokio::test]
    async fn prepend() {
        let expr = Expr::Prepend {
            target: Box::new(lit(" world")),
            prefix: Box::new(lit("hello")),
        };
        assert_eq!(json_eval(expr).await, "hello world");
    }

    #[tokio::test]
    async fn append() {
        let expr = Expr::Append {
            target: Box::new(lit("hello")),
            suffix: Box::new(lit(" world")),
        };
        assert_eq!(json_eval(expr).await, "hello world");
    }

    #[tokio::test]
    async fn starts_with_true() {
        let v = json_eval(Expr::StartsWith {
            target: Box::new(lit("Chapter 1")),
            prefix: "Chapter".into(),
        })
        .await;
        assert_eq!(v, true);
    }

    #[tokio::test]
    async fn starts_with_false() {
        let v = json_eval(Expr::StartsWith {
            target: Box::new(lit("Vol. 1")),
            prefix: "Chapter".into(),
        })
        .await;
        assert_eq!(v, false);
    }

    #[tokio::test]
    async fn ends_with_true() {
        let v = json_eval(Expr::EndsWith {
            target: Box::new(lit("cover.jpg")),
            suffix: ".jpg".into(),
        })
        .await;
        assert_eq!(v, true);
    }

    #[tokio::test]
    async fn slice_from_start() {
        let v = json_eval(Expr::Slice {
            target: Box::new(lit("hello world")),
            start: 0,
            end: Some(5),
        })
        .await;
        assert_eq!(v, "hello");
    }

    #[tokio::test]
    async fn slice_to_end_no_end() {
        let v = json_eval(Expr::Slice {
            target: Box::new(lit("hello world")),
            start: 6,
            end: None,
        })
        .await;
        assert_eq!(v, "world");
    }

    #[tokio::test]
    async fn slice_negative_start() {
        let v = json_eval(Expr::Slice {
            target: Box::new(lit("hello")),
            start: -3,
            end: None,
        })
        .await;
        assert_eq!(v, "llo");
    }

    #[tokio::test]
    async fn slice_inverted_range_empty() {
        let v = json_eval(Expr::Slice {
            target: Box::new(lit("hello")),
            start: 3,
            end: Some(1),
        })
        .await;
        assert_eq!(v, "");
    }

    #[tokio::test]
    async fn matches_true() {
        let v = json_eval(Expr::Matches {
            target: Box::new(lit("Chapter 42")),
            pattern: r"Chapter\s+\d+".into(),
        })
        .await;
        assert_eq!(v, true);
    }

    #[tokio::test]
    async fn matches_false() {
        let v = json_eval(Expr::Matches {
            target: Box::new(lit("Vol. 1")),
            pattern: r"Chapter\s+\d+".into(),
        })
        .await;
        assert_eq!(v, false);
    }

    #[tokio::test]
    async fn capture_group_0_full_match() {
        let v = json_eval(Expr::At {
            target: Box::new(Expr::Capture {
                target: Box::new(lit("Chapter 42")),
                pattern: r"Chapter (\d+)".into(),
            }),
            index: 0,
        })
        .await;
        assert_eq!(v, "Chapter 42");
    }

    #[tokio::test]
    async fn capture_group_1() {
        let v = json_eval(Expr::At {
            target: Box::new(Expr::Capture {
                target: Box::new(lit("Chapter 42")),
                pattern: r"Chapter (\d+)".into(),
            }),
            index: 1,
        })
        .await;
        assert_eq!(v, "42");
    }

    #[tokio::test]
    async fn capture_no_match_returns_empty_list() {
        let rows = json_rows(
            "{}",
            "",
            vec![opt_field(
                "v",
                Expr::At {
                    target: Box::new(Expr::Capture {
                        target: Box::new(lit("Vol. 1")),
                        pattern: r"Chapter (\d+)".into(),
                    }),
                    index: 0,
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn capture_optional_group_null() {
        let v = json_eval_opt(Expr::At {
            target: Box::new(Expr::Capture {
                target: Box::new(lit("Chapter 42")),
                pattern: r"Chapter (\d+)(\.\d+)?".into(),
            }),
            index: 2,
        })
        .await;
        assert_eq!(v, serde_json::Value::Null);
    }

    #[tokio::test]
    async fn parse_float() {
        let v = json_eval(Expr::ParseFloat {
            target: Box::new(lit("3.14")),
        })
        .await;
        assert!((v.as_f64().unwrap() - 3.14).abs() < 1e-9);
    }

    #[tokio::test]
    async fn parse_int() {
        let v = json_eval(Expr::ParseInt {
            target: Box::new(lit("42")),
        })
        .await;
        assert_eq!(v.as_i64().unwrap(), 42);
    }

    #[tokio::test]
    async fn parse_int_negative() {
        let v = json_eval(Expr::ParseInt {
            target: Box::new(lit("-7")),
        })
        .await;
        assert_eq!(v.as_i64().unwrap(), -7);
    }

    #[tokio::test]
    async fn parse_float_invalid_is_null() {
        let v = json_eval_opt(Expr::ParseFloat {
            target: Box::new(lit("abc")),
        })
        .await;
        assert_eq!(v, serde_json::Value::Null);
    }

    #[tokio::test]
    async fn date_parse_rfc3339_epoch() {
        let v = json_eval(Expr::DateParseRfc3339 {
            target: Box::new(lit("1970-01-01T00:00:00Z")),
        })
        .await;
        assert_eq!(v.as_i64().unwrap(), 0);
    }

    #[tokio::test]
    async fn date_parse_rfc3339_known_timestamp() {
        let v = json_eval(Expr::DateParseRfc3339 {
            target: Box::new(lit("2024-01-15T12:00:00Z")),
        })
        .await;
        assert_eq!(v.as_i64().unwrap(), 1_705_320_000);
    }

    #[tokio::test]
    async fn date_parse_format() {
        let v = json_eval(Expr::DateParse {
            target: Box::new(lit("1970-01-01")),
            format: "[year]-[month]-[day]".into(),
        })
        .await;
        assert_eq!(v.as_i64().unwrap(), 0);
    }

    #[tokio::test]
    async fn date_parse_rfc3339_invalid_errors() {
        let err = json_eval_err(Expr::DateParseRfc3339 {
            target: Box::new(lit("not-a-date")),
        })
        .await;
        assert!(err.contains("Invalid RFC3339"));
    }

    #[tokio::test]
    async fn fallback_on_null() {
        let expr = Expr::Fallback {
            target: Box::new(Expr::Null),
            default: Box::new(lit("default")),
        };
        assert_eq!(json_eval(expr).await, "default");
    }

    #[tokio::test]
    async fn fallback_on_empty_string() {
        let expr = Expr::Fallback {
            target: Box::new(lit("")),
            default: Box::new(lit("fallback")),
        };
        assert_eq!(json_eval(expr).await, "fallback");
    }

    #[tokio::test]
    async fn fallback_passes_through_value() {
        let expr = Expr::Fallback {
            target: Box::new(lit("real")),
            default: Box::new(lit("default")),
        };
        assert_eq!(json_eval(expr).await, "real");
    }

    #[tokio::test]
    async fn lookup_hit() {
        let expr = Expr::Lookup {
            target: Box::new(lit("publishing")),
            table: vec![
                ("publishing".into(), "ongoing".into()),
                ("finished".into(), "completed".into()),
            ],
        };
        assert_eq!(json_eval(expr).await, "ongoing");
    }

    #[tokio::test]
    async fn lookup_miss_returns_null_not_error() {
        let rows = json_rows(
            "{}",
            "",
            vec![opt_field(
                "v",
                Expr::Lookup {
                    target: Box::new(lit("unknown")),
                    table: vec![("publishing".into(), "ongoing".into())],
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn lookup_on_null_propagates_null() {
        let v = json_eval_opt(Expr::Lookup {
            target: Box::new(Expr::Null),
            table: vec![("publishing".into(), "ongoing".into())],
        })
        .await;
        assert_eq!(v, serde_json::Value::Null);
    }

    #[tokio::test]
    async fn lookup_miss_with_fallback() {
        let expr = Expr::Fallback {
            target: Box::new(Expr::Lookup {
                target: Box::new(lit("unknown")),
                table: vec![("publishing".into(), "ongoing".into())],
            }),
            default: Box::new(lit("other")),
        };
        assert_eq!(json_eval(expr).await, "other");
    }

    #[tokio::test]
    async fn resolve_url_relative_path() {
        let expr = Expr::ResolveUrl {
            target: Box::new(lit("/chapter/1")),
            base: Box::new(lit("https://example.com/manga/abc")),
        };
        assert_eq!(json_eval(expr).await, "https://example.com/chapter/1");
    }

    #[tokio::test]
    async fn resolve_url_relative_to_dir() {
        let expr = Expr::ResolveUrl {
            target: Box::new(lit("image.jpg")),
            base: Box::new(lit("https://example.com/manga/abc/")),
        };
        assert_eq!(
            json_eval(expr).await,
            "https://example.com/manga/abc/image.jpg"
        );
    }

    #[tokio::test]
    async fn concat_three_parts() {
        let expr = Expr::Concat(vec![lit("hello"), lit(", "), lit("world")]);
        assert_eq!(json_eval(expr).await, "hello, world");
    }

    #[tokio::test]
    async fn list_literal_produces_array() {
        let rows = json_rows(
            "{}",
            "",
            vec![field("v", Expr::List(vec![lit("x"), lit("y"), lit("z")]))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::json!(["x", "y", "z"]));
    }

    #[tokio::test]
    async fn map_transforms_each_item() {
        let expr = Expr::Map {
            target: Box::new(Expr::List(vec![lit("a"), lit("b"), lit("c")])),
            transform: Box::new(Expr::Prepend {
                target: Box::new(var("$item")),
                prefix: Box::new(lit("x_")),
            }),
        };
        let rows = json_rows("{}", "", vec![field("v", expr)], vec![]).await;
        assert_eq!(rows[0]["v"], serde_json::json!(["x_a", "x_b", "x_c"]));
    }

    #[tokio::test]
    async fn map_drops_null_results() {
        let expr = Expr::Map {
            target: Box::new(Expr::List(vec![lit("a"), lit("b"), lit("a")])),
            transform: Box::new(Expr::Lookup {
                target: Box::new(var("$item")),
                table: vec![("a".into(), "kept".into())],
            }),
        };
        let rows = json_rows("{}", "", vec![field("v", expr)], vec![]).await;
        assert_eq!(rows[0]["v"], serde_json::json!(["kept", "kept"]));
    }

    #[tokio::test]
    async fn map_exposes_index() {
        let expr = Expr::Map {
            target: Box::new(Expr::List(vec![lit("a"), lit("b")])),
            transform: Box::new(Expr::ToString {
                target: Box::new(var("$index")),
            }),
        };
        let rows = json_rows("{}", "", vec![field("v", expr)], vec![]).await;
        assert_eq!(rows[0]["v"], serde_json::json!(["0", "1"]));
    }

    #[tokio::test]
    async fn flat_map_flattens_sublists() {
        let expr = Expr::FlatMap {
            target: Box::new(Expr::List(vec![lit("a,b"), lit("c,d")])),
            transform: Box::new(Expr::Split {
                target: Box::new(var("$item")),
                delimiter: ",".into(),
            }),
        };
        let rows = json_rows("{}", "", vec![field("v", expr)], vec![]).await;
        assert_eq!(rows[0]["v"], serde_json::json!(["a", "b", "c", "d"]));
    }

    #[tokio::test]
    async fn filter_keeps_matching_items() {
        let expr = Expr::Filter {
            target: Box::new(Expr::List(vec![
                lit("apple"),
                lit("banana"),
                lit("apricot"),
            ])),
            filter: Box::new(Expr::StartsWith {
                target: Box::new(var("$item")),
                prefix: "a".into(),
            }),
        };
        let rows = json_rows("{}", "", vec![field("v", expr)], vec![]).await;
        assert_eq!(rows[0]["v"], serde_json::json!(["apple", "apricot"]));
    }

    #[tokio::test]
    async fn filter_drops_null_predicate() {
        let expr = Expr::Filter {
            target: Box::new(Expr::List(vec![lit("a"), lit("b"), lit("a")])),
            filter: Box::new(Expr::Matches {
                target: Box::new(var("$item")),
                pattern: "^a$".into(),
            }),
        };
        let rows = json_rows("{}", "", vec![field("v", expr)], vec![]).await;
        assert_eq!(rows[0]["v"], serde_json::json!(["a", "a"]));
    }

    #[tokio::test]
    async fn fold_accumulates() {
        let expr = Expr::Fold {
            target: Box::new(Expr::List(vec![lit("a"), lit("b"), lit("c")])),
            base: Box::new(lit("")),
            transform: Box::new(Expr::Append {
                target: Box::new(var("$acc")),
                suffix: Box::new(var("$item")),
            }),
        };
        assert_eq!(json_eval(expr).await, "abc");
    }

    #[tokio::test]
    async fn fold_empty_list_returns_base() {
        let expr = Expr::Fold {
            target: Box::new(Expr::List(vec![])),
            base: Box::new(lit("base")),
            transform: Box::new(var("$acc")),
        };
        assert_eq!(json_eval(expr).await, "base");
    }

    #[tokio::test]
    async fn if_true_takes_then_branch() {
        let expr = Expr::If {
            condition: Box::new(binop(Op::Eq, lit("a"), lit("a"))),
            then: Box::new(lit("yes")),
            else_: Box::new(lit("no")),
        };
        assert_eq!(json_eval(expr).await, "yes");
    }

    #[tokio::test]
    async fn if_false_takes_else_branch() {
        let expr = Expr::If {
            condition: Box::new(binop(Op::Eq, lit("a"), lit("b"))),
            then: Box::new(lit("yes")),
            else_: Box::new(lit("no")),
        };
        assert_eq!(json_eval(expr).await, "no");
    }

    #[tokio::test]
    async fn if_null_condition_takes_else_branch() {
        let expr = Expr::If {
            condition: Box::new(Expr::Null),
            then: Box::new(lit("yes")),
            else_: Box::new(lit("no")),
        };
        assert_eq!(json_eval(expr).await, "no");
    }

    #[tokio::test]
    async fn if_only_evaluates_taken_branch() {
        let expr = Expr::If {
            condition: Box::new(binop(Op::Eq, lit("a"), lit("a"))),
            then: Box::new(lit("ok")),
            else_: Box::new(var("$undefined_var")),
        };
        assert_eq!(json_eval(expr).await, "ok");
    }

    #[tokio::test]
    async fn to_string_int() {
        let expr = Expr::ToString {
            target: Box::new(Expr::ParseInt {
                target: Box::new(lit("42")),
            }),
        };
        assert_eq!(json_eval(expr).await, "42");
    }

    #[tokio::test]
    async fn to_string_float_whole_no_trailing_zero() {
        assert_eq!(
            json_eval(Expr::ToString {
                target: Box::new(num(3.0))
            })
            .await,
            "3"
        );
    }

    #[tokio::test]
    async fn to_string_float_decimal() {
        assert_eq!(
            json_eval(Expr::ToString {
                target: Box::new(num(3.14))
            })
            .await,
            "3.14"
        );
    }

    #[tokio::test]
    async fn to_string_bool_true() {
        let expr = Expr::ToString {
            target: Box::new(binop(Op::Eq, lit("a"), lit("a"))),
        };
        assert_eq!(json_eval(expr).await, "true");
    }

    #[tokio::test]
    async fn to_string_bool_false() {
        let expr = Expr::ToString {
            target: Box::new(binop(Op::Eq, lit("a"), lit("b"))),
        };
        assert_eq!(json_eval(expr).await, "false");
    }

    #[tokio::test]
    async fn to_string_null_propagates() {
        let rows = json_rows(
            "{}",
            "",
            vec![opt_field(
                "v",
                Expr::ToString {
                    target: Box::new(Expr::Null),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn join_comma_separated() {
        let expr = Expr::Join {
            target: Box::new(Expr::List(vec![lit("a"), lit("b"), lit("c")])),
            delimiter: ", ".into(),
        };
        assert_eq!(json_eval(expr).await, "a, b, c");
    }

    #[tokio::test]
    async fn join_skips_nulls() {
        let expr = Expr::Join {
            target: Box::new(Expr::List(vec![lit("a"), Expr::Null, lit("c")])),
            delimiter: "-".into(),
        };
        assert_eq!(json_eval(expr).await, "a-c");
    }

    #[tokio::test]
    async fn join_empty_list() {
        let expr = Expr::Join {
            target: Box::new(Expr::List(vec![])),
            delimiter: ", ".into(),
        };
        assert_eq!(json_eval(expr).await, "");
    }

    #[tokio::test]
    async fn join_with_map() {
        let expr = Expr::Join {
            target: Box::new(Expr::Map {
                target: Box::new(Expr::List(vec![lit("action"), lit("adventure")])),
                transform: Box::new(Expr::Prepend {
                    target: Box::new(var("$item")),
                    prefix: Box::new(lit("#")),
                }),
            }),
            delimiter: " ".into(),
        };
        assert_eq!(json_eval(expr).await, "#action #adventure");
    }
}

#[cfg(test)]
mod html_tests {
    use super::helpers::*;
    use kani_shared::ast::*;

    fn attr(target: Expr, name: &str) -> Expr {
        Expr::Attr {
            target: Box::new(target),
            name: name.into(),
        }
    }
    fn text(target: Expr) -> Expr {
        Expr::Text {
            target: Box::new(target),
        }
    }
    fn select(target: Expr, selector: &str) -> Expr {
        Expr::Select {
            target: Box::new(target),
            selector: selector.into(),
        }
    }
    fn first(target: Expr, selector: &str) -> Expr {
        Expr::First {
            target: Box::new(target),
            selector: selector.into(),
        }
    }

    #[tokio::test]
    async fn container_iterates_matching_elements() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field("id", attr(Expr::SelfRef, "data-id"))],
            vec![],
        )
        .await;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "manga-123");
        assert_eq!(rows[1]["id"], "manga-456");
    }

    #[tokio::test]
    async fn index_in_container() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "idx",
                Expr::ToString {
                    target: Box::new(Expr::Index),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["idx"], "0");
        assert_eq!(rows[1]["idx"], "1");
    }

    #[tokio::test]
    async fn attr_existing() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field("id", attr(Expr::SelfRef, "data-id"))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "manga-123");
    }

    #[tokio::test]
    async fn attr_missing_on_one_element_optional() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![opt_field(
                "src",
                attr(first(Expr::SelfRef, "img"), "data-src"),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["src"], "https://cdn.example.com/123.jpg");
        assert_eq!(rows[1]["src"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn text_content() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field("title", text(first(Expr::SelfRef, "h2.title")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["title"], "Test Manga");
        assert_eq!(rows[1]["title"], "Another Title");
    }

    #[tokio::test]
    async fn inner_html() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "tags_html",
                Expr::InnerHtml {
                    target: Box::new(first(Expr::SelfRef, "div.tags")),
                },
            )],
            vec![],
        )
        .await;
        let s = rows[0]["tags_html"].as_str().unwrap();
        assert!(s.contains("<a"));
        assert!(s.contains("Action"));
    }

    #[tokio::test]
    async fn select_returns_list_of_elements() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "tags",
                Expr::Map {
                    target: Box::new(select(Expr::SelfRef, "a.tag")),
                    transform: Box::new(text(var("$item"))),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["tags"], serde_json::json!(["Action", "Adventure"]));
        assert_eq!(rows[1]["tags"], serde_json::json!(["Romance"]));
    }

    #[tokio::test]
    async fn first_element() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field("first_tag", text(first(Expr::SelfRef, "a.tag")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["first_tag"], "Action");
        assert_eq!(rows[1]["first_tag"], "Romance");
    }

    #[tokio::test]
    async fn first_returns_null_when_no_match() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![opt_field("v", first(Expr::SelfRef, "span.nonexistent"))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn has_class_true() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "active",
                Expr::HasClass {
                    target: Box::new(Expr::SelfRef),
                    class: "active".into(),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["active"], true);
        assert_eq!(rows[1]["active"], false);
    }

    #[tokio::test]
    async fn children_of_element() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "tag_count",
                Expr::ToString {
                    target: Box::new(Expr::Fold {
                        target: Box::new(Expr::Children {
                            target: Box::new(first(Expr::SelfRef, "div.tags")),
                        }),
                        base: Box::new(num(0.0)),
                        transform: Box::new(binop(Op::Add, var("$acc"), num(1.0))),
                    }),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["tag_count"], "2");
        assert_eq!(rows[1]["tag_count"], "1");
    }

    #[tokio::test]
    async fn dom_from_document_root() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field("title", text(first(Expr::SelfRef, "h2.title")))],
            vec![bind("$page_title", text(Expr::Dom("h2.title".into())))],
        )
        .await;
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn binding_available_in_all_rows() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "suffix",
                Expr::Append {
                    target: Box::new(attr(Expr::SelfRef, "data-id")),
                    suffix: Box::new(var("$suffix")),
                },
            )],
            vec![bind("$suffix", lit("_item"))],
        )
        .await;
        assert_eq!(rows[0]["suffix"], "manga-123_item");
        assert_eq!(rows[1]["suffix"], "manga-456_item");
    }

    #[tokio::test]
    async fn capture_chapter_number() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "chapter",
                Expr::ParseFloat {
                    target: Box::new(Expr::At {
                        target: Box::new(Expr::Capture {
                            target: Box::new(text(first(Expr::SelfRef, "span.chapter"))),
                            pattern: r"Chapter (\d+(?:\.\d+)?)".into(),
                        }),
                        index: 1,
                    }),
                },
            )],
            vec![],
        )
        .await;
        assert!((rows[0]["chapter"].as_f64().unwrap() - 42.0).abs() < 1e-9);
        assert!((rows[1]["chapter"].as_f64().unwrap() - 1.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn if_conditional_on_class() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "kind",
                Expr::If {
                    condition: Box::new(Expr::HasClass {
                        target: Box::new(Expr::SelfRef),
                        class: "active".into(),
                    }),
                    then: Box::new(lit("featured")),
                    else_: Box::new(lit("regular")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["kind"], "featured");
        assert_eq!(rows[1]["kind"], "regular");
    }

    #[tokio::test]
    async fn join_tag_texts() {
        let rows = html_rows(
            MANGA_HTML,
            "article.manga-card",
            vec![field(
                "tags",
                Expr::Join {
                    target: Box::new(Expr::Map {
                        target: Box::new(select(Expr::SelfRef, "a.tag")),
                        transform: Box::new(text(var("$item"))),
                    }),
                    delimiter: ", ".into(),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["tags"], "Action, Adventure");
        assert_eq!(rows[1]["tags"], "Romance");
    }

    #[tokio::test]
    async fn no_container_matches_empty_result() {
        let rows = html_rows(
            MANGA_HTML,
            "div.nonexistent",
            vec![field("x", lit("val"))],
            vec![],
        )
        .await;
        assert_eq!(rows.len(), 0);
    }
}

#[cfg(test)]
mod json_tests {
    use super::helpers::*;
    use kani_shared::ast::*;

    fn ptr(target: Expr, pointer: &str) -> Expr {
        Expr::JsonPtr {
            target: Box::new(target),
            pointer: pointer.into(),
        }
    }

    fn json(pointer: &str) -> Expr {
        Expr::Json(pointer.into())
    }

    #[tokio::test]
    async fn array_container_iterates_elements() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "id",
                Expr::JsonStr {
                    target: Box::new(ptr(Expr::SelfRef, "/id")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "manga-1");
        assert_eq!(rows[1]["id"], "manga-2");
    }

    #[tokio::test]
    async fn json_pointer_root_navigation() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![field(
                "total",
                Expr::JsonInt {
                    target: Box::new(json("/meta/total")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["total"].as_i64().unwrap(), 2);
    }

    #[tokio::test]
    async fn json_ptr_chained() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![field(
                "status",
                Expr::JsonStr {
                    target: Box::new(ptr(json("/data"), "/0/attributes/status")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["status"], "ongoing");
    }

    #[tokio::test]
    async fn json_str() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "status",
                Expr::JsonStr {
                    target: Box::new(ptr(Expr::SelfRef, "/attributes/status")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["status"], "ongoing");
        assert_eq!(rows[1]["status"], "completed");
    }

    #[tokio::test]
    async fn json_int() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "count",
                Expr::JsonInt {
                    target: Box::new(ptr(Expr::SelfRef, "/attributes/chapterCount")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["count"].as_i64().unwrap(), 42);
        assert_eq!(rows[1]["count"].as_i64().unwrap(), 10);
    }

    #[tokio::test]
    async fn json_float() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "rating",
                Expr::JsonFloat {
                    target: Box::new(ptr(Expr::SelfRef, "/attributes/rating")),
                },
            )],
            vec![],
        )
        .await;
        assert!((rows[0]["rating"].as_f64().unwrap() - 8.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn json_bool() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "adult",
                Expr::JsonBool {
                    target: Box::new(ptr(Expr::SelfRef, "/attributes/adult")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["adult"], false);
        assert_eq!(rows[1]["adult"], true);
    }

    #[tokio::test]
    async fn json_ptr_missing_returns_null() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![opt_field(
                "v",
                Expr::JsonStr {
                    target: Box::new(ptr(Expr::SelfRef, "/attributes/nonexistent")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn array_len() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![field(
                "count",
                Expr::ToString {
                    target: Box::new(Expr::ArrayLen {
                        target: Box::new(json("/data")),
                    }),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["count"], "2");
    }

    #[tokio::test]
    async fn json_keys() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![field(
                "keys",
                Expr::JsonKeys {
                    target: Box::new(json("/data/0/attributes/title")),
                },
            )],
            vec![],
        )
        .await;
        let keys = rows[0]["keys"].as_array().unwrap();
        assert!(keys.iter().any(|k| k == "en"));
        assert!(keys.iter().any(|k| k == "ja"));
    }

    #[tokio::test]
    async fn json_get_dynamic_key() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![field(
                "title",
                Expr::JsonStr {
                    target: Box::new(Expr::JsonGet {
                        target: Box::new(json("/data/0/attributes/title")),
                        key: Box::new(Expr::Literal("en".into())),
                    }),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["title"], "Test Manga");
    }

    #[tokio::test]
    async fn json_get_with_variable_key() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![field(
                "title",
                Expr::JsonStr {
                    target: Box::new(Expr::JsonGet {
                        target: Box::new(json("/data/0/attributes/title")),
                        key: Box::new(Expr::Var("$lang".into())),
                    }),
                },
            )],
            vec![bind("$lang", Expr::Literal("ja".into()))],
        )
        .await;
        assert_eq!(rows[0]["title"], "テスト漫画");
    }

    #[tokio::test]
    async fn json_get_missing_key_returns_null() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![opt_field(
                "v",
                Expr::JsonStr {
                    target: Box::new(Expr::JsonGet {
                        target: Box::new(json("/data/0/attributes/title")),
                        key: Box::new(Expr::Literal("fr".into())),
                    }),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn json_find_locates_first_match() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![field(
                "filename",
                Expr::JsonStr {
                    target: Box::new(Expr::JsonPtr {
                        target: Box::new(Expr::JsonFind {
                            target: Box::new(json("/relationships")),
                            key: Box::new(Expr::Literal("type".into())),
                            value: Box::new(Expr::Literal("cover_art".into())),
                        }),
                        pointer: "/attributes/fileName".into(),
                    }),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["filename"], "cover1.jpg");
    }

    #[tokio::test]
    async fn json_find_no_match_returns_null() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![opt_field(
                "v",
                Expr::JsonFind {
                    target: Box::new(json("/relationships")),
                    key: Box::new(Expr::Literal("type".into())),
                    value: Box::new(Expr::Literal("publisher".into())),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["v"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn binding_evaluated_before_iteration() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "cover_file",
                Expr::Fallback {
                    target: Box::new(Expr::JsonStr {
                        target: Box::new(Expr::JsonPtr {
                            target: Box::new(Expr::Var("$cover".into())),
                            pointer: "/attributes/fileName".into(),
                        }),
                    }),
                    default: Box::new(Expr::Literal("unknown.jpg".into())),
                },
            )],
            vec![bind(
                "$cover",
                Expr::JsonFind {
                    target: Box::new(Expr::Json("/relationships".into())),
                    key: Box::new(Expr::Literal("type".into())),
                    value: Box::new(Expr::Literal("cover_art".into())),
                },
            )],
        )
        .await;
        assert_eq!(rows[0]["cover_file"], "cover1.jpg");
        assert_eq!(rows[1]["cover_file"], "cover1.jpg");
    }

    #[tokio::test]
    async fn index_in_json_container() {
        let rows = json_rows(MANGA_JSON, "/data", vec![field("pos", Expr::Index)], vec![]).await;
        assert_eq!(rows[0]["pos"].as_i64().unwrap(), 0);
        assert_eq!(rows[1]["pos"].as_i64().unwrap(), 1);
    }

    #[tokio::test]
    async fn self_ref_in_json_container() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "id",
                Expr::JsonStr {
                    target: Box::new(ptr(Expr::SelfRef, "/id")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "manga-1");
    }

    #[tokio::test]
    async fn map_over_json_array() {
        let rows = json_rows(
            MANGA_JSON,
            "",
            vec![field(
                "ids",
                Expr::Map {
                    target: Box::new(json("/data")),
                    transform: Box::new(Expr::JsonStr {
                        target: Box::new(ptr(var("$item"), "/id")),
                    }),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["ids"], serde_json::json!(["manga-1", "manga-2"]));
    }

    #[tokio::test]
    async fn json_if_conditional_on_field() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "label",
                Expr::If {
                    condition: Box::new(Expr::JsonBool {
                        target: Box::new(ptr(Expr::SelfRef, "/attributes/adult")),
                    }),
                    then: Box::new(lit("nsfw")),
                    else_: Box::new(lit("safe")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["label"], "safe");
        assert_eq!(rows[1]["label"], "nsfw");
    }

    #[tokio::test]
    async fn chapter_count_to_string() {
        let rows = json_rows(
            MANGA_JSON,
            "/data",
            vec![field(
                "display",
                Expr::Append {
                    target: Box::new(Expr::ToString {
                        target: Box::new(Expr::JsonInt {
                            target: Box::new(ptr(Expr::SelfRef, "/attributes/chapterCount")),
                        }),
                    }),
                    suffix: Box::new(lit(" chapters")),
                },
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["display"], "42 chapters");
        assert_eq!(rows[1]["display"], "10 chapters");
    }
}

#[cfg(test)]
mod extension_integration_tests {
    use super::helpers::*;
    use kani_shared::ast::*;

    fn attr(target: Expr, name: &str) -> Expr {
        Expr::Attr {
            target: Box::new(target),
            name: name.into(),
        }
    }
    fn text(target: Expr) -> Expr {
        Expr::Text {
            target: Box::new(target),
        }
    }
    fn first(target: Expr, sel: &str) -> Expr {
        Expr::First {
            target: Box::new(target),
            selector: sel.into(),
        }
    }
    fn select(target: Expr, sel: &str) -> Expr {
        Expr::Select {
            target: Box::new(target),
            selector: sel.into(),
        }
    }
    fn dom(sel: &str) -> Expr {
        Expr::Dom(sel.into())
    }
    fn split(target: Expr, d: &str) -> Expr {
        Expr::Split {
            target: Box::new(target),
            delimiter: d.into(),
        }
    }
    fn at(target: Expr, idx: i32) -> Expr {
        Expr::At {
            target: Box::new(target),
            index: idx,
        }
    }
    fn trim(target: Expr) -> Expr {
        Expr::Trim {
            target: Box::new(target),
        }
    }
    fn lower(target: Expr) -> Expr {
        Expr::Lower {
            target: Box::new(target),
        }
    }
    fn fallback(t: Expr, d: Expr) -> Expr {
        Expr::Fallback {
            target: Box::new(t),
            default: Box::new(d),
        }
    }
    fn map(target: Expr, body: Expr) -> Expr {
        Expr::Map {
            target: Box::new(target),
            transform: Box::new(body),
        }
    }
    fn filter(target: Expr, pred: Expr) -> Expr {
        Expr::Filter {
            target: Box::new(target),
            filter: Box::new(pred),
        }
    }
    fn join(target: Expr, d: &str) -> Expr {
        Expr::Join {
            target: Box::new(target),
            delimiter: d.into(),
        }
    }
    fn matches(target: Expr, p: &str) -> Expr {
        Expr::Matches {
            target: Box::new(target),
            pattern: p.into(),
        }
    }
    fn capture(target: Expr, p: &str) -> Expr {
        Expr::Capture {
            target: Box::new(target),
            pattern: p.into(),
        }
    }
    fn parse_float(target: Expr) -> Expr {
        Expr::ParseFloat {
            target: Box::new(target),
        }
    }
    fn to_string_e(target: Expr) -> Expr {
        Expr::ToString {
            target: Box::new(target),
        }
    }
    fn lookup(target: Expr, table: Vec<(&str, &str)>) -> Expr {
        Expr::Lookup {
            target: Box::new(target),
            table: table
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }
    fn if_expr(cond: Expr, then: Expr, else_: Expr) -> Expr {
        Expr::If {
            condition: Box::new(cond),
            then: Box::new(then),
            else_: Box::new(else_),
        }
    }

    fn json(ptr: &str) -> Expr {
        Expr::Json(ptr.into())
    }
    fn ptr_e(target: Expr, p: &str) -> Expr {
        Expr::JsonPtr {
            target: Box::new(target),
            pointer: p.into(),
        }
    }
    fn jstr(target: Expr) -> Expr {
        Expr::JsonStr {
            target: Box::new(target),
        }
    }
    fn jint(target: Expr) -> Expr {
        Expr::JsonInt {
            target: Box::new(target),
        }
    }
    fn get(target: Expr, key: Expr) -> Expr {
        Expr::JsonGet {
            target: Box::new(target),
            key: Box::new(key),
        }
    }
    fn find(target: Expr, key: Expr, value: Expr) -> Expr {
        Expr::JsonFind {
            target: Box::new(target),
            key: Box::new(key),
            value: Box::new(value),
        }
    }
    fn append(t: Expr, s: Expr) -> Expr {
        Expr::Append {
            target: Box::new(t),
            suffix: Box::new(s),
        }
    }
    fn rfc3339(target: Expr) -> Expr {
        Expr::DateParseRfc3339 {
            target: Box::new(target),
        }
    }

    const LAZY_COVER_SEARCH_HTML: &str = r#"<html><body>
      <div class="grid gap-3">
        <div>
          <a href="/manga/1234/action-hero">
            <img src="/placeholder.jpg" data-src="https://cdn.example.com/1234.jpg">
            <div class="line-clamp-2">Action Hero</div>
          </a>
        </div>
        <div>
          <a href="/manga/5678/romance-story">
            <img src="https://cdn.example.com/5678.jpg">
            <div class="line-clamp-2">Romance Story</div>
          </a>
        </div>
      </div>
    </body></html>"#;

    #[tokio::test]
    async fn lazy_cover_search_id_from_href() {
        let rows = html_rows(
            LAZY_COVER_SEARCH_HTML,
            ".grid.gap-3 > div",
            vec![field(
                "id",
                at(split(attr(first(Expr::SelfRef, "a"), "href"), "/"), 2),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "1234");
        assert_eq!(rows[1]["id"], "5678");
    }

    #[tokio::test]
    async fn lazy_cover_search_cover_fallback_to_src() {
        let rows = html_rows(
            LAZY_COVER_SEARCH_HTML,
            ".grid.gap-3 > div",
            vec![field(
                "cover",
                fallback(
                    attr(first(Expr::SelfRef, "img"), "data-src"),
                    attr(first(Expr::SelfRef, "img"), "src"),
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["cover"], "https://cdn.example.com/1234.jpg");
        assert_eq!(rows[1]["cover"], "https://cdn.example.com/5678.jpg");
    }

    #[tokio::test]
    async fn lazy_cover_search_title() {
        let rows = html_rows(
            LAZY_COVER_SEARCH_HTML,
            ".grid.gap-3 > div",
            vec![field("title", text(first(Expr::SelfRef, ".line-clamp-2")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["title"], "Action Hero");
        assert_eq!(rows[1]["title"], "Romance Story");
    }

    const LAZY_COVER_DETAILS_HTML: &str = r#"<html><body>
      <h1 class="font-bold">One Piece</h1>
      <p class="text-sm">A story about pirates.</p>
      <img data-src="https://cdn.example.com/op.jpg">
      <div class="grid">
        <div>
          <div><div>Type</div><div class="type-val">Manga</div></div>
          <div><div>Status</div><div class="status-val">publishing</div></div>
          <div><div>Year</div><div>1999</div></div>
        </div>
      </div>
      <div class="mb-3">
        <a class="text-sm">Action</a>
        <a class="text-sm">Adventure</a>
        <a class="text-sm">  </a>
      </div>
    </body></html>"#;

    #[tokio::test]
    async fn lazy_cover_details_title() {
        let rows = html_rows(
            LAZY_COVER_DETAILS_HTML,
            ":root",
            vec![field("title", text(dom("h1.font-bold")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["title"], "One Piece");
    }

    #[tokio::test]
    async fn lazy_cover_details_description() {
        let rows = html_rows(
            LAZY_COVER_DETAILS_HTML,
            ":root",
            vec![field("desc", text(dom("p.text-sm")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["desc"], "A story about pirates.");
    }

    #[tokio::test]
    async fn lazy_cover_details_cover() {
        let rows = html_rows(
            LAZY_COVER_DETAILS_HTML,
            ":root",
            vec![field("cover", attr(dom("img"), "data-src"))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["cover"], "https://cdn.example.com/op.jpg");
    }

    #[tokio::test]
    async fn lazy_cover_details_status_lookup() {
        let rows = html_rows(
            LAZY_COVER_DETAILS_HTML,
            ":root",
            vec![field(
                "status",
                fallback(
                    lookup(
                        lower(trim(text(dom(".status-val")))),
                        vec![
                            ("publishing", "ongoing"),
                            ("finished", "completed"),
                            ("on hiatus", "hiatus"),
                            ("discontinued", "cancelled"),
                            ("not yet published", "hiatus"),
                        ],
                    ),
                    lit("unknown"),
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["status"], "ongoing");
    }

    #[tokio::test]
    async fn lazy_cover_details_tags_join() {
        let rows = html_rows(
            LAZY_COVER_DETAILS_HTML,
            ":root",
            vec![field(
                "tags",
                join(
                    filter(
                        map(
                            select(Expr::SelfRef, "div.mb-3 a.text-sm"),
                            trim(text(var("$item"))),
                        ),
                        matches(var("$item"), r"[^\s]"),
                    ),
                    ", ",
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["tags"], "Action, Adventure");
    }

    const COMPACT_CHAPTER_HTML: &str = r#"<html><body>
      <div class="grid">
        <a class="border" href="/chapters/1234567">Chapter 42</a>
        <a class="border" href="/chapters/1234568">Chapter 1.5</a>
        <a class="border" href="/chapters/1234569">Chapter Volume 1</a>
      </div>
    </body></html>"#;

    #[tokio::test]
    async fn compact_html_chapter_id_from_href() {
        let rows = html_rows(
            COMPACT_CHAPTER_HTML,
            "div.grid a.border",
            vec![field("id", at(split(attr(Expr::SelfRef, "href"), "/"), 2))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "1234567");
        assert_eq!(rows[1]["id"], "1234568");
    }

    #[tokio::test]
    async fn compact_html_chapter_number_from_capture() {
        let chapter_text = trim(text(Expr::SelfRef));
        let rows = html_rows(
            COMPACT_CHAPTER_HTML,
            "div.grid a.border",
            vec![field(
                "number",
                fallback(
                    parse_float(fallback(
                        at(
                            capture(chapter_text.clone(), r"Chapter\s+(\d+(?:\.\d+)?)"),
                            1,
                        ),
                        at(split(chapter_text.clone(), " "), -1),
                    )),
                    num(0.0),
                ),
            )],
            vec![],
        )
        .await;
        assert!((rows[0]["number"].as_f64().unwrap() - 42.0).abs() < 1e-9);
        assert!((rows[1]["number"].as_f64().unwrap() - 1.5).abs() < 1e-9);
        assert!((rows[2]["number"].as_f64().unwrap() - 1.0).abs() < 1e-9);
    }

    const COMPACT_PAGES_HTML: &str = r#"<html><body>
      <img class="js-page" data-src="https://cdn.example.com/p/001.jpg">
      <img class="js-page" data-src="https://cdn.example.com/p/002.jpg">
    </body></html>"#;

    #[tokio::test]
    async fn compact_html_pages_url_and_index() {
        let rows = html_rows(
            COMPACT_PAGES_HTML,
            "img.js-page",
            vec![
                field("url", attr(Expr::SelfRef, "data-src")),
                field("index", to_string_e(Expr::Index)),
            ],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["url"], "https://cdn.example.com/p/001.jpg");
        assert_eq!(rows[1]["url"], "https://cdn.example.com/p/002.jpg");
        assert_eq!(rows[0]["index"], "0");
        assert_eq!(rows[1]["index"], "1");
    }

    const SEMANTIC_SEARCH_HTML: &str = r#"<html><body>
      <article>
        <a class="line-clamp-1" href="/series/01JKABCDEF/dungeon-hero">Dungeon Hero</a>
        <img src="https://cdn.example.com/covers/1.jpg">
      </article>
      <article>
        <a class="line-clamp-1" href="/series/01JKGHIJKL/spirit-blade">Spirit Blade</a>
        <img src="https://cdn.example.com/covers/2.jpg">
      </article>
    </body></html>"#;

    #[tokio::test]
    async fn semantic_html_search_id_from_href() {
        let rows = html_rows(
            SEMANTIC_SEARCH_HTML,
            "body > article",
            vec![field(
                "id",
                at(
                    split(attr(first(Expr::SelfRef, "a.line-clamp-1"), "href"), "/"),
                    -2,
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "01JKABCDEF");
        assert_eq!(rows[1]["id"], "01JKGHIJKL");
    }

    #[tokio::test]
    async fn semantic_html_search_title_and_cover() {
        let rows = html_rows(
            SEMANTIC_SEARCH_HTML,
            "body > article",
            vec![
                field("title", text(first(Expr::SelfRef, "a.line-clamp-1"))),
                field("cover", attr(first(Expr::SelfRef, "img"), "src")),
            ],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["title"], "Dungeon Hero");
        assert_eq!(rows[0]["cover"], "https://cdn.example.com/covers/1.jpg");
    }

    const SEMANTIC_DETAILS_HTML: &str = r#"<html><body>
      <h1 class="hidden">Spirit Blade Chronicle</h1>
      <p class="whitespace-pre-wrap">A sword saint travels the land.</p>
      <ul class="flex-col">
        <li>
          <span><a>John Doe</a><a>Jane Smith</a></span>
        </li>
        <li>
          <span><a>Action</a><a>Fantasy</a></span>
        </li>
        <li><a>Manga</a></li>
        <li><a>Ongoing</a></li>
        <li><span>Shonen</span></li>
      </ul>
      <section class="flex" style="nth-child(3)">
        <picture><img src="/cover-small.jpg"><img src="https://cdn.example.com/cover.jpg"></picture>
      </section>
    </body></html>"#;

    #[tokio::test]
    async fn semantic_html_details_title() {
        let rows = html_rows(
            SEMANTIC_DETAILS_HTML,
            ":root",
            vec![field("title", text(dom("h1.hidden")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["title"], "Spirit Blade Chronicle");
    }

    #[tokio::test]
    async fn semantic_html_details_description() {
        let rows = html_rows(
            SEMANTIC_DETAILS_HTML,
            ":root",
            vec![field("desc", text(dom(".whitespace-pre-wrap")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["desc"], "A sword saint travels the land.");
    }

    #[tokio::test]
    async fn semantic_html_details_status() {
        let rows = html_rows(
            SEMANTIC_DETAILS_HTML,
            ":root",
            vec![field("status", text(dom("ul.flex-col li:nth-child(4) a")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["status"], "Ongoing");
    }

    #[tokio::test]
    async fn semantic_html_details_authors_as_list() {
        let rows = html_rows(
            SEMANTIC_DETAILS_HTML,
            ":root",
            vec![field(
                "authors",
                join(
                    map(
                        select(Expr::SelfRef, "ul.flex-col li:first-child span a"),
                        text(var("$item")),
                    ),
                    ", ",
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["authors"], "John Doe, Jane Smith");
    }

    const SEMANTIC_CHAPTERS_HTML: &str = r#"<html><body>
      <div>
        <a href="https://example.com/chapters/01JKCH0001">
          <span class="gap-2"><span>Chapter 1</span></span>
        </a>
        <time datetime="2024-03-15T12:00:00+00:00">Mar 15, 2024</time>
      </div>
      <div>
        <a href="https://example.com/chapters/01JKCH0002">
          <span class="gap-2"><span>Chapter 2</span></span>
        </a>
        <time datetime="2024-03-22T12:00:00+00:00">Mar 22, 2024</time>
      </div>
    </body></html>"#;

    #[tokio::test]
    async fn semantic_html_chapter_id_last_segment() {
        let rows = html_rows(
            SEMANTIC_CHAPTERS_HTML,
            "body > div",
            vec![field(
                "id",
                at(split(attr(first(Expr::SelfRef, "a"), "href"), "/"), -1),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "01JKCH0001");
        assert_eq!(rows[1]["id"], "01JKCH0002");
    }

    #[tokio::test]
    async fn semantic_html_chapter_number_last_word() {
        let rows = html_rows(
            SEMANTIC_CHAPTERS_HTML,
            "body > div",
            vec![field(
                "number",
                parse_float(at(
                    split(text(first(Expr::SelfRef, "span.gap-2 span")), " "),
                    -1,
                )),
            )],
            vec![],
        )
        .await;
        assert!((rows[0]["number"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert!((rows[1]["number"].as_f64().unwrap() - 2.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn semantic_html_chapter_date_rfc3339() {
        let rows = html_rows(
            SEMANTIC_CHAPTERS_HTML,
            "body > div",
            vec![field(
                "date",
                rfc3339(attr(first(Expr::SelfRef, "time"), "datetime")),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["date"].as_i64().unwrap(), 1710504000);
    }

    const SEMANTIC_PAGES_HTML: &str = r#"<html><body>
      <section>
        <img src="https://cdn.example.com/p/001.jpg">
        <img src="https://cdn.example.com/p/002.jpg">
      </section>
    </body></html>"#;

    #[tokio::test]
    async fn semantic_html_pages_src_and_index() {
        let rows = html_rows(
            SEMANTIC_PAGES_HTML,
            "section img",
            vec![
                field("url", attr(Expr::SelfRef, "src")),
                field("index", to_string_e(Expr::Index)),
            ],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["url"], "https://cdn.example.com/p/001.jpg");
        assert_eq!(rows[1]["url"], "https://cdn.example.com/p/002.jpg");
        assert_eq!(rows[0]["index"], "0");
    }

    const RELATIONAL_POPULAR_JSON: &str = r#"{
      "data": [
        {
          "id": "uuid-manga-1",
          "attributes": {
            "title": {"en": "Blue Lock", "ja": "ブルーロック"},
            "status": "ongoing",
            "tags": [
              {"attributes": {"name": {"en": "Sports", "ja": "スポーツ"}}},
              {"attributes": {"name": {"en": "Action"}}}
            ]
          },
          "relationships": [
            {"type": "cover_art",  "attributes": {"fileName": "blue-lock.jpg"}},
            {"type": "author",     "attributes": {"name": "Muneyuki Kaneshiro"}},
            {"type": "artist",     "attributes": {"name": "Yusuke Nomura"}}
          ]
        },
        {
          "id": "uuid-manga-2",
          "attributes": {
            "title": {"ja-ro": "Chainsaw Man"},
            "status": "completed",
            "tags": [
              {"attributes": {"name": {"en": "Action"}}}
            ]
          },
          "relationships": [
            {"type": "cover_art", "attributes": {"fileName": "csm.jpg"}}
          ]
        }
      ],
      "total": 2
    }"#;

    #[tokio::test]
    async fn relational_json_popular_id() {
        let rows = json_rows(
            RELATIONAL_POPULAR_JSON,
            "/data",
            vec![field("id", jstr(ptr_e(Expr::SelfRef, "/id")))],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "uuid-manga-1");
        assert_eq!(rows[1]["id"], "uuid-manga-2");
    }

    #[tokio::test]
    async fn relational_json_localized_title_en_fallback_ja_ro() {
        let rows = json_rows(
            RELATIONAL_POPULAR_JSON,
            "/data",
            vec![field(
                "title",
                fallback(
                    jstr(get(ptr_e(Expr::SelfRef, "/attributes/title"), lit("en"))),
                    jstr(get(ptr_e(Expr::SelfRef, "/attributes/title"), lit("ja-ro"))),
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["title"], "Blue Lock");
        assert_eq!(rows[1]["title"], "Chainsaw Man");
    }

    #[tokio::test]
    async fn relational_json_cover_url_from_relationships_find() {
        let rows = json_rows(
            RELATIONAL_POPULAR_JSON,
            "/data",
            vec![field(
                "cover_file",
                jstr(ptr_e(
                    find(
                        ptr_e(Expr::SelfRef, "/relationships"),
                        lit("type"),
                        lit("cover_art"),
                    ),
                    "/attributes/fileName",
                )),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["cover_file"], "blue-lock.jpg");
        assert_eq!(rows[1]["cover_file"], "csm.jpg");
    }

    #[tokio::test]
    async fn relational_json_tags_map_and_join() {
        let rows = json_rows(
            RELATIONAL_POPULAR_JSON,
            "/data",
            vec![field(
                "tags",
                join(
                    filter(
                        map(
                            ptr_e(Expr::SelfRef, "/attributes/tags"),
                            fallback(
                                jstr(get(ptr_e(var("$item"), "/attributes/name"), lit("en"))),
                                Expr::Null,
                            ),
                        ),
                        Expr::BinaryOperation {
                            op: Op::Ne,
                            lhs: Box::new(var("$item")),
                            rhs: Box::new(Expr::Null),
                        },
                    ),
                    ", ",
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["tags"], "Sports, Action");
        assert_eq!(rows[1]["tags"], "Action");
    }

    #[tokio::test]
    async fn relational_json_authors_from_relationships() {
        let rows = json_rows(
            RELATIONAL_POPULAR_JSON,
            "/data",
            vec![field(
                "authors",
                join(
                    filter(
                        map(
                            ptr_e(Expr::SelfRef, "/relationships"),
                            if_expr(
                                Expr::BinaryOperation {
                                    op: Op::Eq,
                                    lhs: Box::new(jstr(ptr_e(var("$item"), "/type"))),
                                    rhs: Box::new(lit("author")),
                                },
                                jstr(ptr_e(var("$item"), "/attributes/name")),
                                Expr::Null,
                            ),
                        ),
                        Expr::BinaryOperation {
                            op: Op::Ne,
                            lhs: Box::new(var("$item")),
                            rhs: Box::new(Expr::Null),
                        },
                    ),
                    ", ",
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["authors"], "Muneyuki Kaneshiro");
        assert_eq!(rows[1]["authors"], "");
    }

    #[tokio::test]
    async fn relational_json_status_lookup() {
        let rows = json_rows(
            RELATIONAL_POPULAR_JSON,
            "/data",
            vec![field(
                "status",
                lookup(
                    jstr(ptr_e(Expr::SelfRef, "/attributes/status")),
                    vec![
                        ("ongoing", "ongoing"),
                        ("completed", "completed"),
                        ("hiatus", "hiatus"),
                        ("cancelled", "cancelled"),
                    ],
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["status"], "ongoing");
        assert_eq!(rows[1]["status"], "completed");
    }

    const RELATIONAL_CHAPTER_JSON: &str = r#"{
      "data": [
        {
          "id": "ch-uuid-1",
          "attributes": {
            "chapter": "42",
            "volume": "4",
            "title": "The Showdown",
            "translatedLanguage": "en",
            "createdAt": "2024-01-15T08:00:00+00:00"
          },
          "relationships": [
            {"type": "scanlation_group", "attributes": {"name": "ScanGroup A"}}
          ]
        }
      ],
      "total": 1
    }"#;

    #[tokio::test]
    async fn relational_json_chapter_number_parse_float() {
        let rows = json_rows(
            RELATIONAL_CHAPTER_JSON,
            "/data",
            vec![field(
                "number",
                fallback(
                    parse_float(jstr(ptr_e(Expr::SelfRef, "/attributes/chapter"))),
                    num(0.0),
                ),
            )],
            vec![],
        )
        .await;
        assert!((rows[0]["number"].as_f64().unwrap() - 42.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn relational_json_chapter_date_rfc3339() {
        let rows = json_rows(
            RELATIONAL_CHAPTER_JSON,
            "/data",
            vec![field(
                "date",
                rfc3339(jstr(ptr_e(Expr::SelfRef, "/attributes/createdAt"))),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["date"].as_i64().unwrap(), 1705305600);
    }

    #[tokio::test]
    async fn relational_json_scanlator_from_relationships_find() {
        let rows = json_rows(
            RELATIONAL_CHAPTER_JSON,
            "/data",
            vec![field(
                "scanlator",
                jstr(ptr_e(
                    find(
                        ptr_e(Expr::SelfRef, "/relationships"),
                        lit("type"),
                        lit("scanlation_group"),
                    ),
                    "/attributes/name",
                )),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["scanlator"], "ScanGroup A");
    }

    const RELATIONAL_HOST_JSON: &str = r#"{
      "baseUrl": "https://uploads.example.com",
      "chapter": {
        "hash": "abc123def456",
        "data": ["001-x1.jpg", "002-x2.jpg", "003-x3.jpg"]
      }
    }"#;

    #[tokio::test]
    async fn relational_json_pages_construct_url() {
        let rows = json_rows(
            RELATIONAL_HOST_JSON,
            "/chapter/data",
            vec![
                field(
                    "url",
                    append(
                        append(
                            append(append(var("$base"), lit("/data/")), var("$hash")),
                            lit("/"),
                        ),
                        jstr(Expr::SelfRef),
                    ),
                ),
                field("index", to_string_e(Expr::Index)),
            ],
            vec![
                bind("$base", jstr(json("/baseUrl"))),
                bind("$hash", jstr(json("/chapter/hash"))),
            ],
        )
        .await;
        assert_eq!(
            rows[0]["url"],
            "https://uploads.example.com/data/abc123def456/001-x1.jpg"
        );
        assert_eq!(
            rows[1]["url"],
            "https://uploads.example.com/data/abc123def456/002-x2.jpg"
        );
        assert_eq!(rows[2]["index"], "2");
    }

    const OBJECT_LIST_JSON: &str = r#"{
      "result": {
        "items": [
          {"hash_id": "abc123", "title": "Hero Academia", "poster": {"large": "https://cdn.example.net/ha.jpg"}},
          {"hash_id": "def456", "title": "Solo Leveling"}
        ],
        "pagination": {"last_page": 5, "current_page": 1}
      }
    }"#;

    #[tokio::test]
    async fn object_json_list_id_and_title() {
        let rows = json_rows(
            OBJECT_LIST_JSON,
            "/result/items",
            vec![
                field("id", jstr(ptr_e(Expr::SelfRef, "/hash_id"))),
                field("title", jstr(ptr_e(Expr::SelfRef, "/title"))),
            ],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "abc123");
        assert_eq!(rows[0]["title"], "Hero Academia");
        assert_eq!(rows[1]["id"], "def456");
    }

    #[tokio::test]
    async fn object_json_optional_cover_url() {
        let rows = json_rows(
            OBJECT_LIST_JSON,
            "/result/items",
            vec![opt_field(
                "cover",
                jstr(ptr_e(Expr::SelfRef, "/poster/large")),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["cover"], "https://cdn.example.net/ha.jpg");
        assert_eq!(rows[1]["cover"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn object_json_pagination_last_page() {
        let rows = json_rows(
            OBJECT_LIST_JSON,
            "",
            vec![field(
                "last_page",
                to_string_e(jint(json("/result/pagination/last_page"))),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["last_page"], "5");
    }

    const OBJECT_DETAIL_JSON: &str = r#"{
      "result": {
        "title": "Hero Academia",
        "synopsis": "A story of heroes.",
        "status": "RELEASING",
        "poster": {"large": "https://cdn.example.net/ha-large.jpg"},
        "type": "Manga",
        "genre": [
          {"title": "Action"},
          {"title": "School Life"}
        ],
        "author": [{"title": "Kohei Horikoshi"}],
        "artist": [{"title": "Kohei Horikoshi"}]
      }
    }"#;

    #[tokio::test]
    async fn object_json_details_title_and_description() {
        let rows = json_rows(
            OBJECT_DETAIL_JSON,
            "",
            vec![
                field("title", jstr(json("/result/title"))),
                field("desc", jstr(json("/result/synopsis"))),
            ],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["title"], "Hero Academia");
        assert_eq!(rows[0]["desc"], "A story of heroes.");
    }

    #[tokio::test]
    async fn object_json_details_status_lookup() {
        let rows = json_rows(
            OBJECT_DETAIL_JSON,
            "",
            vec![field(
                "status",
                lookup(
                    jstr(json("/result/status")),
                    vec![
                        ("RELEASING", "ongoing"),
                        ("FINISHED", "completed"),
                        ("HIATUS", "hiatus"),
                        ("DISCONTINUED", "cancelled"),
                    ],
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["status"], "ongoing");
    }

    #[tokio::test]
    async fn object_json_details_tags_from_genre_array() {
        let rows = json_rows(
            OBJECT_DETAIL_JSON,
            "",
            vec![field(
                "tags",
                join(
                    map(json("/result/genre"), jstr(ptr_e(var("$item"), "/title"))),
                    ", ",
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["tags"], "Action, School Life");
    }

    #[tokio::test]
    async fn object_json_details_authors() {
        let rows = json_rows(
            OBJECT_DETAIL_JSON,
            "",
            vec![field(
                "authors",
                join(
                    map(json("/result/author"), jstr(ptr_e(var("$item"), "/title"))),
                    ", ",
                ),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["authors"], "Kohei Horikoshi");
    }

    const OBJECT_CHAPTER_JSON: &str = r#"{
      "result": {
        "items": [
          {
            "chapter_id": 98765,
            "number": 42.0,
            "name": "Showdown",
            "volume": 4,
            "created_at": 1710504000,
            "language": "en",
            "scanlation_group": {"name": "OfficialRip"}
          }
        ],
        "pagination": {"last_page": 3}
      }
    }"#;

    #[tokio::test]
    async fn object_json_chapter_id_as_string() {
        let rows = json_rows(
            OBJECT_CHAPTER_JSON,
            "/result/items",
            vec![field(
                "id",
                to_string_e(Expr::JsonInt {
                    target: Box::new(ptr_e(Expr::SelfRef, "/chapter_id")),
                }),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["id"], "98765");
    }

    #[tokio::test]
    async fn object_json_chapter_scanlator() {
        let rows = json_rows(
            OBJECT_CHAPTER_JSON,
            "/result/items",
            vec![field(
                "scanlator",
                jstr(ptr_e(Expr::SelfRef, "/scanlation_group/name")),
            )],
            vec![],
        )
        .await;
        assert_eq!(rows[0]["scanlator"], "OfficialRip");
    }
}

#[cfg(test)]
mod dsl_v2_tests {
    #![allow(clippy::unwrap_used)]
    use super::helpers::*;
    use kani_shared::ast::*;

    #[tokio::test]
    async fn split_n_basic() {
        let expr = Expr::SplitN {
            target: Box::new(Expr::Literal("a,b,c,d".into())),
            delimiter: ",".into(),
            n: 2,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["a", "b,c,d"]));
    }

    #[tokio::test]
    async fn split_n_n_equals_one() {
        let expr = Expr::SplitN {
            target: Box::new(Expr::Literal("a,b,c".into())),
            delimiter: ",".into(),
            n: 1,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["a,b,c"]));
    }

    #[tokio::test]
    async fn split_n_n_exceeds_parts() {
        let expr = Expr::SplitN {
            target: Box::new(Expr::Literal("a,b".into())),
            delimiter: ",".into(),
            n: 10,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["a", "b"]));
    }

    #[tokio::test]
    async fn take_first_two() {
        let expr = Expr::Take {
            target: Box::new(Expr::List(vec![
                Expr::Literal("a".into()),
                Expr::Literal("b".into()),
                Expr::Literal("c".into()),
            ])),
            n: 2,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["a", "b"]));
    }

    #[tokio::test]
    async fn take_n_exceeds_length() {
        let expr = Expr::Take {
            target: Box::new(Expr::List(vec![
                Expr::Literal("x".into()),
                Expr::Literal("y".into()),
            ])),
            n: 100,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["x", "y"]));
    }

    #[tokio::test]
    async fn skip_first_two() {
        let expr = Expr::Skip {
            target: Box::new(Expr::List(vec![
                Expr::Literal("a".into()),
                Expr::Literal("b".into()),
                Expr::Literal("c".into()),
            ])),
            n: 2,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["c"]));
    }

    #[tokio::test]
    async fn reverse_list() {
        let expr = Expr::Reverse {
            target: Box::new(Expr::List(vec![
                Expr::Literal("a".into()),
                Expr::Literal("b".into()),
                Expr::Literal("c".into()),
            ])),
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["c", "b", "a"]));
    }

    #[tokio::test]
    async fn sort_by_string_key() {
        let json = r#"{"items": ["banana", "apple", "cherry"]}"#;
        let rows = json_rows(json, "/items", vec![field("v", Expr::SelfRef)], vec![]).await;
        let list_expr = Expr::List(vec![
            Expr::Literal("banana".into()),
            Expr::Literal("apple".into()),
            Expr::Literal("cherry".into()),
        ]);
        let sorted = Expr::SortBy {
            target: Box::new(list_expr),
            key: Box::new(Expr::Var("$item".into())),
        };
        let v = json_eval(sorted).await;
        assert_eq!(v, serde_json::json!(["apple", "banana", "cherry"]));
        let _ = rows;
    }

    #[tokio::test]
    async fn sort_by_mixed_types_non_comparable_to_end() {
        let sorted = Expr::SortBy {
            target: Box::new(Expr::List(vec![
                Expr::Null,
                Expr::Number(2.0),
                Expr::Number(1.0),
            ])),
            key: Box::new(Expr::Var("$item".into())),
        };
        let v = json_eval(sorted).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0].as_f64().unwrap(), 1.0);
        assert_eq!(arr[1].as_f64().unwrap(), 2.0);
        assert_eq!(arr[2], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn unique_removes_duplicates_preserves_order() {
        let expr = Expr::Unique {
            target: Box::new(Expr::List(vec![
                Expr::Literal("a".into()),
                Expr::Literal("b".into()),
                Expr::Literal("a".into()),
                Expr::Literal("c".into()),
                Expr::Literal("b".into()),
            ])),
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["a", "b", "c"]));
    }

    /// `.unique()` over values that came out of a JSON document, rather than
    /// out of `Expr::Literal`. The literal form is `Value::Str`; this one is
    /// `Value::Json`, which is the shape every JSON source actually produces.
    #[tokio::test]
    async fn unique_removes_duplicate_json_values() {
        let doc = r#"{"tags": ["a", "b", "a", "c", "b"]}"#;
        let expr = Expr::Unique {
            target: Box::new(Expr::SelfRef.ptr("/tags")),
        };
        let rows = json_rows(doc, "", vec![field("v", expr)], vec![]).await;
        assert_eq!(
            rows[0]["v"],
            serde_json::json!(["a", "b", "c"]),
            "unique() must deduplicate JSON-derived values, not just literals"
        );
    }

    #[tokio::test]
    async fn unique_already_unique_unchanged() {
        let expr = Expr::Unique {
            target: Box::new(Expr::List(vec![
                Expr::Literal("x".into()),
                Expr::Literal("y".into()),
                Expr::Literal("z".into()),
            ])),
        };
        let v = json_eval(expr).await;
        assert_eq!(v, serde_json::json!(["x", "y", "z"]));
    }

    #[tokio::test]
    async fn encoded_field_passthrough_joins_subfields() {
        use kani_shared::ast::IdEncoding;
        let expr = Expr::EncodedField {
            subfields: vec![
                ("a".into(), Box::new(Expr::Literal("manga123".into()))),
                ("b".into(), Box::new(Expr::Literal("ch456".into()))),
            ],
            delimiter: "|".into(),
            encoding: IdEncoding::Passthrough,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, "manga123|ch456");
    }

    #[tokio::test]
    async fn encoded_field_base64url_encodes() {
        use base64::{Engine, engine::general_purpose};
        use kani_shared::ast::IdEncoding;
        let expr = Expr::EncodedField {
            subfields: vec![
                ("slug".into(), Box::new(Expr::Literal("my/manga".into()))),
                ("ch".into(), Box::new(Expr::Literal("1".into()))),
            ],
            delimiter: "|".into(),
            encoding: IdEncoding::Base64Url,
        };
        let v = json_eval(expr).await;
        let expected = general_purpose::URL_SAFE_NO_PAD.encode("my/manga|1");
        assert_eq!(v, expected);
    }

    #[tokio::test]
    async fn url_encode_spaces_and_special_chars() {
        let expr = Expr::UrlEncode {
            target: Box::new(Expr::Literal("hello world & more".into())),
        };
        let v = json_eval(expr).await;
        assert_eq!(v, "hello%20world%20%26%20more");
    }

    #[tokio::test]
    async fn url_decode_encoded_string() {
        let expr = Expr::UrlDecode {
            target: Box::new(Expr::Literal("hello%20world%20%26%20more".into())),
        };
        let v = json_eval(expr).await;
        assert_eq!(v, "hello world & more");
    }

    #[tokio::test]
    async fn url_decode_bad_percent_encoding_passthrough() {
        let expr = Expr::UrlDecode {
            target: Box::new(Expr::Literal("bad%ZZencoding".into())),
        };
        let v = json_eval(expr).await;
        assert!(!v.as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn format_padded_left_align() {
        let expr = Expr::FormatPadded {
            target: Box::new(Expr::Literal("hi".into())),
            width: 5,
            fill: '-',
            align: PadAlign::Left,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, "hi---");
    }

    #[tokio::test]
    async fn format_padded_right_align() {
        let expr = Expr::FormatPadded {
            target: Box::new(Expr::Literal("hi".into())),
            width: 5,
            fill: '0',
            align: PadAlign::Right,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, "000hi");
    }

    #[tokio::test]
    async fn format_padded_center_align() {
        let expr = Expr::FormatPadded {
            target: Box::new(Expr::Literal("hi".into())),
            width: 6,
            fill: '-',
            align: PadAlign::Center,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, "--hi--");
    }

    #[tokio::test]
    async fn format_padded_width_less_than_input_unchanged() {
        let expr = Expr::FormatPadded {
            target: Box::new(Expr::Literal("hello".into())),
            width: 3,
            fill: '-',
            align: PadAlign::Left,
        };
        let v = json_eval(expr).await;
        assert_eq!(v, "hello");
    }

    #[tokio::test]
    async fn scalar_override_reads_document_level_scalar() {
        use crate::evaluator::json_eval::extract_json;
        use crate::wasm::HostState;
        use kani_shared::ast::{Blueprint, FieldDef};

        let json = r#"{"items": [{"id": 1}, {"id": 2}], "total": 42}"#;
        let mut state = HostState::default();
        let doc: serde_json::Value = serde_json::from_str(json).unwrap();
        let handle = state.next_doc_handle;
        state.next_doc_handle += 1;
        state.json_docs.insert(handle, doc);

        let bp = Blueprint {
            request: None,
            container: "/items".into(),
            fields: vec![
                FieldDef {
                    name: "id".into(),
                    expr: Expr::JsonPtr {
                        target: Box::new(Expr::SelfRef),
                        pointer: "/id".into(),
                    },
                    optional: false,
                },
                FieldDef {
                    name: "total".into(),
                    expr: Expr::ScalarOverride {
                        name: "total".into(),
                    },
                    optional: false,
                },
            ],
            bindings: vec![],
            scalars: vec![FieldDef {
                name: "total".into(),
                expr: Expr::Json("/total".into()),
                optional: false,
            }],
            pagination: None,
        };
        let out = extract_json(&mut state, Some(handle), &bp).await.unwrap();
        let rows = out["rows"].as_array().unwrap();
        assert_eq!(rows[0]["total"], 42);
        assert_eq!(rows[1]["total"], 42);
    }
}
