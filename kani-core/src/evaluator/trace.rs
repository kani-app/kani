use std::cell::RefCell;

#[derive(Debug, Clone)]
pub struct TraceStep {
    pub expr_kind: String,
    pub output: String,
}

#[derive(Debug, Default, Clone)]
pub struct EvalTrace {
    pub steps: Vec<TraceStep>,
}

impl EvalTrace {
    pub fn push(&mut self, expr_kind: impl Into<String>, output: impl Into<String>) {
        self.steps.push(TraceStep {
            expr_kind: expr_kind.into(),
            output: output.into(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

thread_local! {
    static TRACE: RefCell<Option<EvalTrace>> = const { RefCell::new(None) };
}

pub fn trace_enable() {
    TRACE.with(|t| *t.borrow_mut() = Some(EvalTrace::default()));
}

pub fn trace_push(expr_kind: impl Into<String>, output: impl Into<String>) {
    TRACE.with(|t| {
        if let Some(trace) = t.borrow_mut().as_mut() {
            trace.push(expr_kind, output);
        }
    });
}

pub fn trace_take() -> Option<EvalTrace> {
    TRACE.with(|t| t.borrow_mut().take())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn trace_enable_take_roundtrip() {
        trace_enable();
        trace_push("TestExpr", "some-output");
        let trace = trace_take().unwrap();
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.steps[0].expr_kind, "TestExpr");
        assert_eq!(trace.steps[0].output, "some-output");
        assert!(trace_take().is_none());
    }

    #[test]
    fn trace_disabled_is_noop() {
        trace_push("Ghost", "phantom");
        assert!(trace_take().is_none());
    }
}
