use rhai::Engine;
use std::sync::OnceLock;

struct RhaiLimits {
    max_ops: u64,
    max_string: usize,
    max_array: usize,
}

static RHAI_LIMITS: OnceLock<RhaiLimits> = OnceLock::new();

fn rhai_limits() -> &'static RhaiLimits {
    RHAI_LIMITS.get_or_init(|| {
        let max_ops = parse_env("KANI_RHAI_MAX_OPS", 100_000u64);
        let max_string = parse_env("KANI_RHAI_MAX_STRING", 1_000_000usize);
        let max_array = parse_env("KANI_RHAI_MAX_ARRAY", 10_000usize);

        if max_ops > 100_000 {
            tracing::warn!(
                "KANI_RHAI_MAX_OPS={} exceeds the recommended default of 100000",
                max_ops
            );
        }
        if max_string > 1_000_000 {
            tracing::warn!(
                "KANI_RHAI_MAX_STRING={} exceeds the recommended default of 1000000",
                max_string
            );
        }
        if max_array > 10_000 {
            tracing::warn!(
                "KANI_RHAI_MAX_ARRAY={} exceeds the recommended default of 10000",
                max_array
            );
        }

        RhaiLimits {
            max_ops,
            max_string,
            max_array,
        }
    })
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub fn make_pure_sandbox() -> Engine {
    let limits = rhai_limits();
    let mut engine = Engine::new();

    engine.set_max_operations(limits.max_ops);
    engine.set_max_expr_depths(64, 32);
    engine.set_max_call_levels(16);
    engine.set_max_string_size(limits.max_string);
    engine.set_max_array_size(limits.max_array);
    engine.set_max_map_size(1_000);
    crate::scripting::bytes::register_byte_bindings(&mut engine);

    engine.disable_symbol("eval");
    engine.disable_symbol("import");
    engine.disable_symbol("export");

    engine
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn sandbox_blocks_eval() {
        let engine = make_pure_sandbox();
        assert!(engine.eval::<()>("eval(\"1+1\")").is_err());
    }

    #[test]
    fn operation_limit_terminates_infinite_loop() {
        let engine = make_pure_sandbox();
        let result = engine.eval::<i64>("let i = 0; loop { i += 1; } i");
        assert!(result.is_err());
    }

    #[test]
    fn import_is_disabled() {
        let engine = make_pure_sandbox();
        assert!(engine.eval::<()>("import \"module\";").is_err());
    }
}
