use std::collections::HashMap;

use rhai::{AST, Dynamic, Engine, Scope};

use crate::evaluator::shared::Value;
use crate::scripting::engine::make_pure_sandbox;

pub struct PureFunctionRegistry {
    engine: Engine,
    asts: HashMap<String, AST>,
}

impl PureFunctionRegistry {
    pub fn compile(scripts: &std::collections::BTreeMap<String, String>) -> Result<Self, String> {
        let engine = make_pure_sandbox();
        let mut asts = HashMap::with_capacity(scripts.len());
        for (name, src) in scripts {
            let ast = engine
                .compile(src)
                .map_err(|e| format!("Rhai compile error in script '{name}': {e}"))?;
            asts.insert(name.clone(), ast);
        }
        Ok(Self { engine, asts })
    }

    pub fn call(&self, name: &str, args: &[Value]) -> Result<Value, String> {
        let Some(ast) = self.asts.get(name) else {
            return Err(format!("Rhai function '{name}' not found in registry"));
        };
        if args
            .first()
            .map(|v| matches!(v, Value::Null))
            .unwrap_or(false)
        {
            return Ok(Value::Null);
        }
        let rhai_args: Vec<Dynamic> = args
            .iter()
            .map(dsl_to_dynamic)
            .collect::<Result<Vec<_>, _>>()?;
        let result: Dynamic = self
            .engine
            .call_fn::<Dynamic>(&mut Scope::new(), ast, name, rhai_args)
            .map_err(|e| format!("Rhai error in '{name}': {e}"))?;
        dynamic_to_dsl(result)
    }
}

fn dsl_to_dynamic(v: &Value) -> Result<Dynamic, String> {
    match v {
        Value::Str(s) => Ok(Dynamic::from(s.clone())),
        Value::Num(f) => Ok(Dynamic::from(*f)),
        Value::Int(i) => Ok(Dynamic::from(*i)),
        Value::Bool(b) => Ok(Dynamic::from(*b)),
        Value::Null => Ok(Dynamic::from(())),
        Value::List(items) => {
            let arr: Result<Vec<Dynamic>, String> = items.iter().map(dsl_to_dynamic).collect();
            Ok(Dynamic::from_array(arr?))
        }
        Value::Json(j) => Ok(json_to_dynamic(j)),
        Value::HtmlElement { .. } => {
            Err("HtmlElement cannot be passed to a Rhai script".to_string())
        }
    }
}

fn json_to_dynamic(j: &serde_json::Value) -> Dynamic {
    match j {
        serde_json::Value::Null => Dynamic::from(()),
        serde_json::Value::Bool(b) => Dynamic::from(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from(i)
            } else {
                Dynamic::from(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Dynamic::from(s.clone()),
        serde_json::Value::Array(arr) => {
            Dynamic::from_array(arr.iter().map(json_to_dynamic).collect())
        }
        serde_json::Value::Object(obj) => {
            let map: rhai::Map = obj
                .iter()
                .map(|(k, v)| (k.as_str().into(), json_to_dynamic(v)))
                .collect();
            Dynamic::from_map(map)
        }
    }
}

fn dynamic_to_dsl(d: Dynamic) -> Result<Value, String> {
    if let Some(s) = d.clone().try_cast::<String>() {
        return Ok(Value::Str(s));
    }
    if let Some(i) = d.clone().try_cast::<i64>() {
        return Ok(Value::Int(i));
    }
    if let Some(f) = d.clone().try_cast::<f64>() {
        return Ok(Value::Num(f));
    }
    if let Some(b) = d.clone().try_cast::<bool>() {
        return Ok(Value::Bool(b));
    }
    if d.is_unit() {
        return Ok(Value::Null);
    }
    if let Some(arr) = d.clone().try_cast::<rhai::Array>() {
        let items: Result<Vec<Value>, String> = arr.into_iter().map(dynamic_to_dsl).collect();
        return Ok(Value::List(items?));
    }
    if let Some(map) = d.try_cast::<rhai::Map>() {
        let obj: Result<serde_json::Map<String, serde_json::Value>, String> = map
            .into_iter()
            .map(|(k, v)| Ok((k.to_string(), dynamic_to_json(v)?)))
            .collect();
        return Ok(Value::Json(serde_json::Value::Object(obj?)));
    }
    Err("Rhai returned an unsupported type".to_string())
}

fn dynamic_to_json(d: Dynamic) -> Result<serde_json::Value, String> {
    if let Some(s) = d.clone().try_cast::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Some(i) = d.clone().try_cast::<i64>() {
        return Ok(serde_json::Value::Number(i.into()));
    }
    if let Some(f) = d.clone().try_cast::<f64>() {
        return Ok(serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null));
    }
    if let Some(b) = d.clone().try_cast::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if d.is_unit() {
        return Ok(serde_json::Value::Null);
    }
    Ok(serde_json::Value::String(d.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn make_registry(name: &str, src: &str) -> PureFunctionRegistry {
        PureFunctionRegistry::compile(&[(name.to_string(), src.to_string())].into_iter().collect())
            .unwrap()
    }

    #[test]
    fn null_receiver_propagates() {
        let reg = make_registry("test_fn", "fn test_fn(s) { s + \"!\" }");
        assert_eq!(reg.call("test_fn", &[Value::Null]).unwrap(), Value::Null);
    }

    #[test]
    fn string_roundtrip() {
        let reg = make_registry("upper", "fn upper(s) { s.to_upper() }");
        assert_eq!(
            reg.call("upper", &[Value::Str("hello".into())]).unwrap(),
            Value::Str("HELLO".into())
        );
    }

    #[test]
    fn int_to_float_coercion() {
        let reg = make_registry("to_f", "fn to_f(i) { i * 1.5 }");
        let result = reg.call("to_f", &[Value::Int(4)]).unwrap();
        assert!(matches!(result, Value::Num(n) if (n - 6.0).abs() < f64::EPSILON));
    }

    #[test]
    fn unknown_fn_returns_err() {
        let reg = make_registry("foo", "fn foo(s) { s }");
        assert!(reg.call("bar", &[Value::Str("x".into())]).is_err());
    }

    #[test]
    fn op_limit_terminates_runaway_script() {
        let reg = make_registry(
            "loop_forever",
            "fn loop_forever(s) { let i = 0; loop { i += 1; } s }",
        );
        assert!(reg.call("loop_forever", &[Value::Str("x".into())]).is_err());
    }

    #[test]
    fn compile_rejects_syntax_error() {
        let result = PureFunctionRegistry::compile(
            &[(
                "bad".to_string(),
                "fn bad(s) { s.to_upper( // missing paren".to_string(),
            )]
            .into_iter()
            .collect(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn json_string_value_coerces() {
        let reg = make_registry("passthrough", "fn passthrough(j) { j }");
        let json = Value::Json(serde_json::Value::String("hello".into()));
        let result = reg.call("passthrough", &[json]).unwrap();
        assert_eq!(result, Value::Str("hello".into()));
    }

    #[test]
    fn null_second_arg_becomes_unit() {
        let reg = make_registry("check_unit", "fn check_unit(a, b) { b == () }");
        let result = reg
            .call("check_unit", &[Value::Str("x".into()), Value::Null])
            .unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn bool_roundtrip() {
        let reg = make_registry("flip", "fn flip(b) { !b }");
        assert_eq!(
            reg.call("flip", &[Value::Bool(true)]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn list_roundtrip() {
        let reg = make_registry("first_elem", "fn first_elem(arr) { arr[0] }");
        let list = Value::List(vec![Value::Str("a".into()), Value::Str("b".into())]);
        assert_eq!(
            reg.call("first_elem", &[list]).unwrap(),
            Value::Str("a".into())
        );
    }

    #[test]
    fn html_element_arg_is_rejected() {
        let reg = make_registry("foo", "fn foo(s) { s }");
        let fake_doc = std::sync::Arc::new(std::sync::Mutex::new(
            crate::wasm::SafeHtml::parse_document("<p>hi</p>"),
        ));
        let node_id = fake_doc.lock().unwrap().0.tree.root().id();
        let val = Value::HtmlElement {
            doc: fake_doc,
            node_id,
        };
        assert!(reg.call("foo", &[val]).is_err());
    }
}
