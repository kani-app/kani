use crate::codegen;
use crate::error::CliError;
use crate::yaml::{schema::YamlExtension, validate::validate};
use std::path::{Path, PathBuf};

pub fn run(file: &str, force: bool, embedded_bytes: bool) -> Result<PathBuf, CliError> {
    let path = Path::new(file);
    let source = std::fs::read_to_string(path)?;

    let ext: YamlExtension = serde_yaml::from_str(&source)
        .map_err(|e| CliError::Other(format!("YAML parse error: {e}")))?;

    let validated = validate(&ext, &source, path).map_err(|errors| {
        for e in &errors {
            eprintln!("error: {e}");
        }
        CliError::Other(format!(
            "{} validation error(s) — generation aborted",
            errors.len()
        ))
    })?;

    reject_interpreted_only(&validated)?;

    let generated = codegen::generate(&validated, embedded_bytes);

    let workspace_root = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists())
        .unwrap_or_else(|| Path::new("."));

    let out_dir = workspace_root
        .join("kani-extensions")
        .join(format!("kani-{}", generated.id));

    if out_dir.exists() && !force {
        return Err(CliError::Other(format!(
            "{} already exists — pass --force to overwrite",
            out_dir.display()
        )));
    }

    std::fs::create_dir_all(out_dir.join("src"))?;
    std::fs::write(out_dir.join("Cargo.toml"), &generated.cargo_toml)?;
    std::fs::write(out_dir.join("src").join("lib.rs"), &generated.lib_rs)?;

    write_scripts(&out_dir, &generated)?;

    println!("Generated: {}", out_dir.display());
    Ok(out_dir)
}

/// Writes a generated crate's `src/scripts/`, refusing any name that could leave that directory.
pub(crate) fn write_scripts(
    crate_dir: &Path,
    generated: &codegen::GeneratedCrate,
) -> Result<(), CliError> {
    if generated.browser_scripts.is_empty() && generated.pure_scripts.is_empty() {
        return Ok(());
    }
    let scripts_dir = crate_dir.join("src").join("scripts");
    std::fs::create_dir_all(&scripts_dir)?;
    let files = generated
        .browser_scripts
        .iter()
        .map(|(name, src)| (name, "js", src))
        .chain(
            generated
                .pure_scripts
                .iter()
                .map(|(name, src)| (name, "rhai", src)),
        );
    for (name, ext, src) in files {
        if !kani_yaml::yaml::validate::is_script_name(name) {
            return Err(CliError::Other(format!(
                "script name {name:?} must match [a-z][a-z0-9_]*"
            )));
        }
        std::fs::write(scripts_dir.join(format!("{name}.{ext}")), src)?;
    }
    Ok(())
}

/// Refuses features that only the interpreted YAML backend implements, rather than emitting a
/// crate that silently ignores them. See the backend capability table in `SPECIFICATION.md` §5.
pub fn reject_interpreted_only(
    validated: &crate::yaml::model::ValidatedExtension,
) -> Result<(), CliError> {
    // `deduplicate_by` is applied host-side, where the rows and a DSL evaluator
    // both exist. A generated extension has neither, so refuse rather than emit
    // a crate that silently ignores the key.
    let dropped: Vec<String> = [
        "popular",
        "search",
        "manga_details",
        "chapter_list",
        "pages",
    ]
    .iter()
    .filter_map(|name| validated.endpoint_by_name(name).map(|ep| (name, ep)))
    .flat_map(|(name, ep)| {
        ep.for_each_steps
            .iter()
            .filter(|s| s.deduplicate_by.is_some())
            .map(move |s| format!("{name}.for_each[{}]", s.merge_as))
    })
    .collect();
    if !dropped.is_empty() {
        return Err(CliError::Other(format!(
            "`deduplicate_by` is not supported in generated extensions, only in \
             interpreted YAML sources: {}. Remove it, or run this source \
             interpreted.",
            dropped.join(", ")
        )));
    }

    let hooks = validated
        .pre_request
        .iter()
        .map(|body| ("pre_request".to_string(), body))
        .chain(
            validated
                .on_status
                .iter()
                .map(|(status, body)| (format!("on_status.{status}"), body)),
        )
        .chain(
            validated
                .endpoint_pre_request
                .iter()
                .map(|(ep, body)| (format!("endpoints.{ep}.pre_request"), body)),
        )
        .chain(validated.endpoint_on_status.iter().flat_map(|(ep, map)| {
            map.iter()
                .map(move |(status, body)| (format!("endpoints.{ep}.on_status.{status}"), body))
        }));
    let refreshing: Vec<String> = hooks
        .filter(|(_, body)| {
            kani_core::scripting::script_calls(body, "refresh_auth").unwrap_or(false)
        })
        .map(|(location, _)| location)
        .collect();
    if !refreshing.is_empty() {
        return Err(CliError::Other(format!(
            "`refresh_auth` is not supported in generated extensions, only in interpreted YAML \
             sources: {}. Run this source interpreted.",
            refreshing.join(", ")
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_hook_that_calls_refresh_auth_is_refused_for_a_generated_extension() {
        let yaml = |hook: &str| {
            format!(
                "id: refresh\nname: Refresh\nversion: \"0.1.0\"\nbase_url: \"https://e.com\"\n{hook}\n\
                 endpoints:\n  search:\n    route: /s\n    container: \".i\"\n    fields:\n      \
                 id: 'self.attr(\"i\")'\n      title: 'self.attr(\"t\")'\n"
            )
        };
        let validate = |text: String| {
            kani_yaml::parse_and_validate(&text, std::path::Path::new("r.yaml")).unwrap()
        };
        let calling = validate(yaml(
            "on_status:\n  \"401\": |\n    refresh_auth(\"search\")",
        ));
        let err = reject_interpreted_only(&calling).unwrap_err().to_string();
        assert!(
            err.contains("refresh_auth") && err.contains("on_status.401"),
            "{err}"
        );

        let mentioning = validate(yaml(
            "on_status:\n  \"401\": |\n    let s = \"refresh_auth(x)\"; proceed()",
        ));
        assert!(reject_interpreted_only(&mentioning).is_ok());
    }

    #[test]
    fn a_script_name_that_escapes_the_scripts_dir_is_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let crate_dir = dir.path().join("crate");
        let generated = codegen::GeneratedCrate {
            id: "x".into(),
            cargo_toml: String::new(),
            lib_rs: String::new(),
            browser_scripts: [("../../escaped".to_string(), "passPayload(1)".to_string())]
                .into_iter()
                .collect(),
            pure_scripts: Default::default(),
        };

        let err = write_scripts(&crate_dir, &generated).unwrap_err();

        assert!(err.to_string().contains("must match"), "{err}");
        assert!(!dir.path().join("escaped.js").exists());
        assert!(!crate_dir.join("escaped.js").exists());
    }
}
