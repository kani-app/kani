#![allow(clippy::unwrap_used)]

//! SPECIFICATION.md §3.6 claims to list every key the YAML format accepts. For each mapping in
//! that sketch, this puts an unknown key at the same place in a document and reads the keys the
//! parser says it expected there, then requires the two lists to match.

use std::collections::{BTreeMap, BTreeSet};

use kani_cli::yaml::schema::YamlExtension;
use serde_yaml::{Mapping, Value};

const PROBE: &str = "zz_probe";

struct Node {
    key: String,
    value: Option<String>,
    items: Option<Vec<Vec<Node>>>,
    children: Vec<Node>,
}

struct Line {
    indent: usize,
    dash: bool,
    key: String,
    value: Option<String>,
}

#[derive(Clone)]
enum Seg {
    Key(String),
    Item,
}

fn sketch() -> String {
    let spec = include_str!("../../SPECIFICATION.md");
    let section = &spec[spec
        .find("### 3.6 Complete Schema Reference")
        .expect("§3.6 heading")..];
    let start = section.find("```yaml\n").expect("§3.6 yaml block") + "```yaml\n".len();
    let len = section[start..].find("```").expect("end of §3.6 block");
    section[start..start + len].to_string()
}

fn lines(block: &str) -> Vec<Line> {
    block
        .lines()
        .filter_map(|raw| {
            let text = match raw.find('#') {
                Some(i) if raw[..i].trim().is_empty() || raw[..i].ends_with(' ') => &raw[..i],
                _ => raw,
            };
            let text = text.trim_end();
            if text.trim().is_empty() {
                return None;
            }
            let indent = text.len() - text.trim_start().len();
            let (dash, body) = match text.trim_start().strip_prefix("- ") {
                Some(rest) => (true, rest),
                None => (false, text.trim_start()),
            };
            let (key, value) = match body.split_once(": ") {
                Some((k, v)) => (k, Some(v.trim().to_string())),
                None => (body.strip_suffix(':').expect("a `key:` line"), None),
            };
            Some(Line {
                indent: if dash { indent + 2 } else { indent },
                dash,
                key: key.to_string(),
                value,
            })
        })
        .collect()
}

fn parse_mapping(lines: &[Line], i: &mut usize, indent: usize, first_is_item: bool) -> Vec<Node> {
    let mut nodes = Vec::new();
    while *i < lines.len() && lines[*i].indent == indent {
        if lines[*i].dash && !(first_is_item && nodes.is_empty()) {
            break;
        }
        let line = &lines[*i];
        *i += 1;
        let mut node = Node {
            key: line.key.clone(),
            value: line.value.clone(),
            items: None,
            children: Vec::new(),
        };
        if node.value.is_none() && *i < lines.len() && lines[*i].indent > indent {
            let child_indent = lines[*i].indent;
            if lines[*i].dash {
                let mut items = Vec::new();
                while *i < lines.len() && lines[*i].dash && lines[*i].indent == child_indent {
                    items.push(parse_mapping(lines, i, child_indent, true));
                }
                node.items = Some(items);
            } else {
                node.children = parse_mapping(lines, i, child_indent, false);
            }
        }
        nodes.push(node);
    }
    nodes
}

fn is_placeholder(key: &str) -> bool {
    key.starts_with('<') && key.ends_with('>')
}

fn document(path: &[Seg], leaf: Value) -> Value {
    path.iter().rev().fold(leaf, |inner, seg| match seg {
        Seg::Key(k) => {
            let mut m = Mapping::new();
            m.insert(Value::String(k.clone()), inner);
            Value::Mapping(m)
        }
        Seg::Item => Value::Sequence(vec![inner]),
    })
}

fn accepted_keys(path: &[Seg], mapping: &[Node]) -> BTreeSet<String> {
    let mut probe = Mapping::new();
    for node in mapping {
        let shape = match (node.key.as_str(), node.value.as_deref()) {
            ("kind", Some(kind)) if kind.chars().all(|c| c.is_ascii_lowercase() || c == '_') => {
                Some(kind.to_string())
            }
            ("delegate_to", _) => Some("search".to_string()),
            ("expr", _) => Some("x".to_string()),
            _ => None,
        };
        if let Some(v) = shape {
            probe.insert(Value::String(node.key.clone()), Value::String(v));
        }
    }
    probe.insert(Value::String(PROBE.into()), Value::from(0));
    let doc = serde_yaml::to_string(&document(path, Value::Mapping(probe))).unwrap();
    let error = match serde_yaml::from_str::<YamlExtension>(&doc) {
        Ok(_) => panic!("the probe was accepted:\n{doc}"),
        Err(e) => e.to_string(),
    };
    let at = error
        .find(&format!("unknown field `{PROBE}`, expected"))
        .unwrap_or_else(|| panic!("the probe did not reach its mapping:\n{doc}\n{error}"));
    error[at..]
        .split('`')
        .skip(3)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn check(
    path: Vec<Seg>,
    mapping: &[Node],
    types: &BTreeMap<String, Vec<Node>>,
    problems: &mut Vec<String>,
    probed: &mut usize,
) {
    let placeholders = mapping.iter().all(|n| is_placeholder(&n.key));
    if !placeholders {
        let listed: BTreeSet<String> = mapping.iter().map(|n| n.key.clone()).collect();
        let accepted = accepted_keys(&path, mapping);
        *probed += 1;
        let at = describe(&path);
        for key in listed.difference(&accepted) {
            problems.push(format!(
                "{at}: §3.6 lists `{key}`, which the parser refuses"
            ));
        }
        for key in accepted.difference(&listed) {
            problems.push(format!(
                "{at}: the parser accepts `{key}`, which §3.6 omits"
            ));
        }
    }
    for node in mapping {
        let key = if is_placeholder(&node.key) {
            "x".to_string()
        } else {
            node.key.clone()
        };
        let mut here = path.clone();
        here.push(Seg::Key(key));
        if let Some(items) = &node.items {
            let mut item = here.clone();
            item.push(Seg::Item);
            for fields in items {
                check(item.clone(), fields, types, problems, probed);
            }
        } else if !node.children.is_empty() {
            check(here, &node.children, types, problems, probed);
        } else if let Some(value) = &node.value {
            for shape in value.split('|').map(str::trim) {
                if let Some(fields) = types.get(shape) {
                    check(here.clone(), fields, types, problems, probed);
                } else if let Some(fields) = shape
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .and_then(|name| types.get(name))
                {
                    let mut item = here.clone();
                    item.push(Seg::Item);
                    check(item, fields, types, problems, probed);
                }
            }
        }
    }
}

fn describe(path: &[Seg]) -> String {
    if path.is_empty() {
        return "top level".to_string();
    }
    path.iter()
        .map(|s| match s {
            Seg::Key(k) => format!(".{k}"),
            Seg::Item => "[]".to_string(),
        })
        .collect::<String>()
        .trim_start_matches('.')
        .to_string()
}

#[test]
fn the_complete_schema_lists_exactly_the_keys_the_parser_accepts() {
    let lines = lines(&sketch());
    let mut i = 0;
    let root = parse_mapping(&lines, &mut i, 0, false);
    assert_eq!(i, lines.len(), "every line of the sketch was read");

    let (types, keys): (Vec<Node>, Vec<Node>) = root
        .into_iter()
        .partition(|n| n.key.starts_with(|c: char| c.is_ascii_uppercase()));
    let types: BTreeMap<String, Vec<Node>> =
        types.into_iter().map(|n| (n.key, n.children)).collect();

    let mut problems = Vec::new();
    let mut probed = 0;
    check(Vec::new(), &keys, &types, &mut problems, &mut probed);
    assert!(
        probed > 30,
        "the sketch reached only {probed} mappings; the parser above misread it"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
