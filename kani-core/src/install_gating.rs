use crate::http::{SmartClient, SolverCapability};

/// Capabilities the host provides unconditionally. `browser_payload` is not
/// among them: it depends on the solver that is configured at the time, so it
/// is resolved per-install rather than compiled in.
pub const HOST_CAPABILITIES: &[&str] = &["unrestricted_http", "rhai_scripting", "scoped_cache"];

pub const BROWSER_PAYLOAD: &str = "browser_payload";

/// Refuses an extension whose recorded blueprint schema this host cannot read. `None` (built
/// before the version was recorded) passes; such an extension is checked on its first extraction.
pub fn check_dsl_schema_version(version: Option<u32>) -> Result<(), String> {
    match version {
        Some(v) if !kani_shared::ast::is_readable_dsl_schema_version(v) => Err(format!(
            "extension was built for blueprint schema version {v}, but this Kani reads {} to {}; \
             rebuild it with a matching kani-cli",
            kani_shared::ast::MIN_READABLE_DSL_SCHEMA_VERSION,
            kani_shared::ast::DSL_SCHEMA_VERSION
        )),
        _ => Ok(()),
    }
}

pub fn check_min_kani_version(min_version: Option<&str>, host_version: &str) -> Result<(), String> {
    let Some(min_version) = min_version else {
        return Ok(());
    };
    let required = semver::Version::parse(min_version)
        .map_err(|e| format!("invalid min_kani_version '{min_version}': {e}"))?;
    let host = semver::Version::parse(host_version)
        .map_err(|e| format!("invalid host version '{host_version}': {e}"))?;
    if host < required {
        return Err(format!(
            "extension requires kani >= {min_version}, but this host is running {host_version}"
        ));
    }
    Ok(())
}

fn browser_payload_refusal(solver: SolverCapability) -> Option<String> {
    let message = match solver {
        SolverCapability::Capture => return None,
        SolverCapability::Basic => {
            "This source needs a solver that can run capture scripts. Yours solves HTTP \
             challenges but cannot run scripts — switch it to the \
             ghcr.io/kani-app/flaresolverr image in Settings > Advanced."
        }
        SolverCapability::Unauthorized => {
            "This source needs a solver that can run capture scripts, but the solver rejected \
             Kani's key. Check that KANI_SOLVER_SECRET matches the solver's API_KEY."
        }
        SolverCapability::Unreachable => {
            "This source needs a solver that can run capture scripts, but no solver answered at \
             the configured URL. Check the solver URL in Settings > Advanced."
        }
        SolverCapability::NotConfigured => {
            "This source needs a solver that can run capture scripts. Set a solver URL in \
             Settings > Advanced."
        }
    };
    Some(message.to_string())
}

pub fn check_required_capabilities(
    required: &[String],
    solver: SolverCapability,
) -> Result<(), String> {
    for cap in required {
        if cap == BROWSER_PAYLOAD {
            if let Some(refusal) = browser_payload_refusal(solver) {
                return Err(refusal);
            }
            continue;
        }
        if !HOST_CAPABILITIES.contains(&cap.as_str()) {
            return Err(format!("extension requires unsupported capability '{cap}'"));
        }
    }
    Ok(())
}

/// The solver state to check `required` against: probed only when the extension asks for
/// `browser_payload`, so ordinary installs stay offline.
pub async fn solver_for(required: &[String], http: &SmartClient) -> SolverCapability {
    if required.iter().any(|cap| cap == BROWSER_PAYLOAD) {
        http.solver_capability().await
    } else {
        SolverCapability::Capture
    }
}

/// Ids kept for the in-tree development extensions.
pub const RESERVED_EXTENSION_IDS: &[&str] = &["example", "test-abi"];

pub fn check_extension_id(id: &str) -> Result<(), String> {
    if !kani_shared::types::is_valid_extension_id(id) {
        return Err(format!("Extension id '{id}' must match [a-z][a-z0-9-]*"));
    }
    if RESERVED_EXTENSION_IDS.contains(&id) {
        return Err(format!(
            "Extension ID '{id}' is reserved for development use and cannot be installed"
        ));
    }
    Ok(())
}

/// What an artifact declares about itself, whichever format it is.
pub struct ArtifactFacts<'a> {
    pub id: &'a str,
    pub min_kani_version: Option<&'a str>,
    pub dsl_schema_version: Option<u32>,
    pub requires_capabilities: &'a [String],
}

/// Every check an artifact must pass to install on `host_version`, in the order the server
/// applies them. Installs stop at the first problem; `kani-cli check` reports them all.
pub fn check_artifact(
    facts: &ArtifactFacts<'_>,
    host_version: &str,
    solver: SolverCapability,
) -> Vec<String> {
    [
        check_extension_id(facts.id),
        check_min_kani_version(facts.min_kani_version, host_version),
        check_dsl_schema_version(facts.dsl_schema_version),
        check_required_capabilities(facts.requires_capabilities, solver),
    ]
    .into_iter()
    .filter_map(Result::err)
    .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn only_readable_blueprint_versions_pass() {
        for ok in [None, Some(5), Some(kani_shared::ast::DSL_SCHEMA_VERSION)] {
            assert!(check_dsl_schema_version(ok).is_ok(), "{ok:?}");
        }
        for bad in [4, kani_shared::ast::DSL_SCHEMA_VERSION + 1] {
            let err = check_dsl_schema_version(Some(bad)).unwrap_err();
            assert!(
                err.contains(&format!("schema version {bad}")) && err.contains("rebuild"),
                "{err}"
            );
        }
    }

    fn caps(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    const STATES: [SolverCapability; 5] = [
        SolverCapability::Capture,
        SolverCapability::Basic,
        SolverCapability::Unauthorized,
        SolverCapability::Unreachable,
        SolverCapability::NotConfigured,
    ];

    #[test]
    fn check_artifact_reports_every_problem_in_install_order() {
        let caps = caps(&["teleport"]);
        let problems = check_artifact(
            &ArtifactFacts {
                id: "example",
                min_kani_version: Some("99.0.0"),
                dsl_schema_version: Some(kani_shared::ast::DSL_SCHEMA_VERSION + 1),
                requires_capabilities: &caps,
            },
            "1.0.0",
            SolverCapability::Capture,
        );
        let expected = ["reserved", "99.0.0", "schema version", "teleport"];
        assert_eq!(problems.len(), expected.len(), "{problems:?}");
        for (problem, needle) in problems.iter().zip(expected) {
            assert!(
                problem.contains(needle),
                "{problem:?} should mention {needle:?}"
            );
        }
    }

    #[test]
    fn a_malformed_id_is_refused_before_the_reserved_list() {
        assert!(
            check_extension_id("Bad_Id")
                .unwrap_err()
                .contains("must match")
        );
        assert!(
            check_extension_id("test-abi")
                .unwrap_err()
                .contains("reserved")
        );
        assert!(check_extension_id("comix").is_ok());
    }

    #[test]
    fn min_version_none_always_passes() {
        assert!(check_min_kani_version(None, "0.1.0").is_ok());
    }

    #[test]
    fn min_version_satisfied_passes() {
        assert!(check_min_kani_version(Some("0.1.0"), "0.1.0").is_ok());
        assert!(check_min_kani_version(Some("0.1.0"), "1.0.0").is_ok());
    }

    #[test]
    fn min_version_unsatisfied_fails() {
        let err = check_min_kani_version(Some("0.5.0"), "0.1.0").unwrap_err();
        assert!(err.contains("0.5.0"));
        assert!(err.contains("0.1.0"));
    }

    #[test]
    fn min_version_invalid_semver_fails() {
        assert!(check_min_kani_version(Some("not-a-version"), "0.1.0").is_err());
    }

    #[test]
    fn no_required_capabilities_passes_in_every_solver_state() {
        for state in STATES {
            assert!(check_required_capabilities(&[], state).is_ok());
        }
    }

    #[test]
    fn a_static_capability_ignores_the_solver_state() {
        for state in STATES {
            assert!(check_required_capabilities(&caps(&["unrestricted_http"]), state).is_ok());
        }
    }

    #[test]
    fn browser_payload_needs_a_capture_capable_solver() {
        assert!(
            check_required_capabilities(&caps(&[BROWSER_PAYLOAD]), SolverCapability::Capture)
                .is_ok()
        );
    }

    #[test]
    fn browser_payload_is_refused_in_every_other_solver_state() {
        for state in [
            SolverCapability::Basic,
            SolverCapability::Unauthorized,
            SolverCapability::Unreachable,
            SolverCapability::NotConfigured,
        ] {
            let err = check_required_capabilities(&caps(&[BROWSER_PAYLOAD]), state)
                .expect_err("browser capture is unavailable in this state");
            assert!(
                err.contains("capture scripts"),
                "the refusal names what is missing, got: {err}"
            );
            assert!(
                err.contains("Settings > Advanced") || err.contains("KANI_SOLVER_SECRET"),
                "the refusal names where to fix it, got: {err}"
            );
        }
    }

    #[test]
    fn each_refusal_state_reads_differently() {
        let messages: std::collections::HashSet<String> = [
            SolverCapability::Basic,
            SolverCapability::Unauthorized,
            SolverCapability::Unreachable,
            SolverCapability::NotConfigured,
        ]
        .iter()
        .map(|state| check_required_capabilities(&caps(&[BROWSER_PAYLOAD]), *state).unwrap_err())
        .collect();
        assert_eq!(messages.len(), 4, "each state needs its own diagnosis");
    }

    #[test]
    fn an_unknown_capability_still_fails() {
        let err =
            check_required_capabilities(&caps(&["does_not_exist"]), SolverCapability::Capture)
                .unwrap_err();
        assert!(err.contains("does_not_exist"));
    }
}
