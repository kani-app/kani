//! Checks an extension artifact the way the server does before installing it, so a repository
//! never publishes one that Kani would refuse or that fails once it runs.

use std::path::Path;

use kani_core::http::SolverCapability;
use kani_core::install_gating::{ArtifactFacts, check_artifact};
use kani_core::scripting::{HookRegistry, HookScripts, PureFunctionRegistry};

use crate::error::CliError;

const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(file: &str) -> Result<(), CliError> {
    let problems = problems(Path::new(file))?;
    if problems.is_empty() {
        println!("✓ {file} would install on Kani {HOST_VERSION}");
        return Ok(());
    }
    for problem in &problems {
        eprintln!("error: {problem}");
    }
    Err(CliError::Other(format!(
        "{} problem(s) in {file}",
        problems.len()
    )))
}

/// Everything that would stop the server installing the extension at `path`, or make its
/// scripts fail once they run. Empty when there is nothing to report.
pub fn problems(path: &Path) -> Result<Vec<String>, CliError> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("yaml" | "yml") => check_yaml(path),
        Some("wasm") => check_wasm(path),
        _ => Err(CliError::Other(format!(
            "{}: expected a .yaml or .wasm extension",
            path.display()
        ))),
    }
}

fn check_yaml(path: &Path) -> Result<Vec<String>, CliError> {
    let text = std::fs::read_to_string(path)?;
    let ext = match kani_yaml::parse_and_validate(&text, path) {
        Ok(ext) => ext,
        Err(errors) => return Ok(errors.iter().map(ToString::to_string).collect()),
    };
    let mut problems = check_artifact(
        &ArtifactFacts {
            id: &ext.id,
            min_kani_version: ext.min_kani_version.as_deref(),
            dsl_schema_version: None,
            requires_capabilities: &ext.requires_capabilities,
        },
        HOST_VERSION,
        SolverCapability::Capture,
    );
    let hooks = HookScripts {
        shared: ext.pure_scripts.clone(),
        pre_request: ext.pre_request.clone(),
        on_status: ext.on_status.clone(),
        endpoint_pre_request: ext.endpoint_pre_request.clone(),
        endpoint_on_status: ext.endpoint_on_status.clone(),
        cache: ext.cache_limits(),
    };
    problems.extend(check_scripts(&hooks));
    Ok(problems)
}

fn check_wasm(path: &Path) -> Result<Vec<String>, CliError> {
    let bytes = std::fs::read(path)?;
    let runtime = kani_core::wasm::WasmRuntime::new_on_demand()
        .map_err(|e| CliError::Other(format!("WASM runtime: {e}")))?;
    let component = match runtime.compile_component(&bytes) {
        Ok(component) => component,
        Err(e) => return Ok(vec![format!("not a loadable WASM component: {e}")]),
    };
    if let Err(e) = runtime.instantiate_pre(&component) {
        return Ok(vec![format!(
            "does not link against Kani {HOST_VERSION}'s host interface: {e}"
        )]);
    }

    let client = kani_core::http::SmartClient::new(None)
        .map_err(|e| CliError::Other(format!("HTTP client: {e}")))?;
    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::Other(format!("runtime: {e}")))?;
    let raw = tokio.block_on(async {
        let mut instance = kani_core::sources::SourceInstance::new(client, None, false);
        instance
            .load(runtime.engine(), &component, runtime.linker())
            .await?;
        instance.get_metadata().await
    });
    let raw = match raw {
        Ok(raw) => raw,
        Err(e) => {
            return Ok(vec![format!(
                "could not read the extension's metadata: {e}"
            )]);
        }
    };
    let metadata: kani_shared::ExtensionMetadata = match serde_json::from_str(&raw) {
        Ok(metadata) => metadata,
        Err(e) => return Ok(vec![format!("invalid extension metadata: {e}")]),
    };

    let mut problems = check_artifact(
        &ArtifactFacts {
            id: &metadata.id,
            min_kani_version: metadata.min_kani_version.as_deref(),
            dsl_schema_version: metadata.dsl_schema_version,
            requires_capabilities: &metadata.requires_capabilities,
        },
        HOST_VERSION,
        SolverCapability::Capture,
    );
    problems.extend(check_scripts(&HookScripts::from_metadata(&metadata)));
    Ok(problems)
}

/// Compiles the scripts on the engines they run on, and reports calls nothing defines.
fn check_scripts(hooks: &HookScripts) -> Vec<String> {
    let mut problems = Vec::new();
    if !hooks.shared.is_empty()
        && let Err(e) = PureFunctionRegistry::compile(&hooks.shared)
    {
        problems.push(format!("scripts: {e}"));
    }
    if hooks.is_empty() {
        return problems;
    }
    match HookRegistry::compile(hooks) {
        Ok(registry) => {
            problems.extend(registry.unresolved_calls().into_iter().map(|(hook, name)| {
                format!("{hook} calls `{name}`, which neither the scripts nor Kani define")
            }))
        }
        Err(e) => problems.push(e),
    }
    problems
}
