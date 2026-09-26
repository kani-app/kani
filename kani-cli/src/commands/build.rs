use crate::error::CliError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEV_EXTENSIONS: &[&str] = &[
    "kani-example",
    "kani-test-abi",
    "kani-fixture-source",
    "kani-fixture-gen",
];

/// Where the extension crates live.
///
/// Site-targeting extensions are published from their own repository, so the
/// usual working arrangement is two checkouts side by side rather than one
/// tree. Resolution, most explicit first:
///
/// 1. `--ext-dir`
/// 2. `$KANI_EXT_DIR`
/// 3. `kani-extensions/` under the current directory
/// 4. `../kani-extensions/` — the sibling checkout
///
/// The sibling step only applies when that directory actually exists, and the
/// caller is told which one was chosen, so a build never silently compiles from
/// somewhere the developer did not mean.
fn resolve_dir(
    flag: Option<&str>,
    env_var: &str,
    default_name: &str,
    exists: &dyn Fn(&Path) -> bool,
) -> (PathBuf, DirSource) {
    if let Some(p) = flag {
        return (PathBuf::from(p), DirSource::Flag);
    }
    if let Ok(p) = std::env::var(env_var)
        && !p.trim().is_empty()
    {
        return (PathBuf::from(p), DirSource::Env);
    }
    let local = PathBuf::from(default_name);
    if exists(&local) {
        return (local, DirSource::Local);
    }
    let sibling = PathBuf::from("..").join(default_name);
    if exists(&sibling) {
        return (sibling, DirSource::Sibling);
    }
    (local, DirSource::Local)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum DirSource {
    Flag,
    Env,
    Local,
    Sibling,
}

impl DirSource {
    const fn describe(self) -> &'static str {
        match self {
            Self::Flag => "--ext-dir",
            Self::Env => "KANI_EXT_DIR",
            Self::Local => "the working directory",
            Self::Sibling => "a sibling checkout",
        }
    }
}

pub fn run(
    extension: Option<&str>,
    all: bool,
    dev: bool,
    set_version: Option<&str>,
    ext_dir: Option<&str>,
    out_dir: Option<&str>,
    debug: bool,
) -> Result<(), CliError> {
    if extension.is_none() && !all && !dev {
        return Err(CliError::Other(
            "specify an extension name, --all, or --dev".into(),
        ));
    }

    if let Some(ext_arg) = extension
        && (ext_arg.ends_with(".yaml") || ext_arg.ends_with(".yml"))
    {
        let (out_dir, _) = resolve_dir(out_dir, "KANI_OUT_DIR", "wasm_sources", &|_| true);
        let (ext_root, _) =
            resolve_dir(ext_dir, "KANI_EXT_DIR", "kani-extensions", &|p| p.exists());
        return build_factory_yaml(ext_arg, &ext_root, set_version, &out_dir, debug);
    }

    let (extensions_dir, from) =
        resolve_dir(ext_dir, "KANI_EXT_DIR", "kani-extensions", &|p| p.exists());
    if !extensions_dir.exists() {
        return Err(CliError::Other(format!(
            "{} not found (looked via {}). Extensions live in their own \
             repository: clone it beside this one, or point at it with \
             --ext-dir or KANI_EXT_DIR.",
            extensions_dir.display(),
            from.describe()
        )));
    }
    if from == DirSource::Sibling || from == DirSource::Env {
        println!(
            "Building from {} ({})",
            extensions_dir.display(),
            from.describe()
        );
    }

    let (out_dir, _) = resolve_dir(out_dir, "KANI_OUT_DIR", "wasm_sources", &|_| true);

    // Repository metadata and build output are not extension directories.
    let dirs: Vec<String> = fs::read_dir(&extensions_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| e.path().join("Cargo.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    let to_build: Vec<String> = if dev {
        dirs.into_iter()
            .filter(|name| DEV_EXTENSIONS.contains(&name.as_str()))
            .collect()
    } else {
        let available: Vec<String> = dirs
            .into_iter()
            .filter(|name| !DEV_EXTENSIONS.contains(&name.as_str()))
            .collect();

        if all {
            available
        } else {
            let name = extension.expect("checked above");
            if !available.contains(&name.to_owned()) {
                return Err(CliError::ExtensionNotFound(name.to_owned()));
            }
            vec![name.to_owned()]
        }
    };

    for name in &to_build {
        build_one(
            name,
            &extensions_dir.join(name),
            set_version,
            &out_dir,
            debug,
        )?;
    }

    Ok(())
}

fn build_factory_yaml(
    yaml_path: &str,
    ext_root: &Path,
    set_version: Option<&str>,
    out_dir: &Path,
    debug: bool,
) -> Result<(), CliError> {
    use crate::yaml::{schema::YamlExtension, validate};

    let path = Path::new(yaml_path);
    let source = fs::read_to_string(path)
        .map_err(|e| CliError::Other(format!("cannot read {}: {e}", path.display())))?;

    let ext: YamlExtension = serde_yaml::from_str(&source)
        .map_err(|e| CliError::Other(format!("YAML parse error in {}: {e}", path.display())))?;

    let factory = ext.factory.as_ref().ok_or_else(|| {
        CliError::Other(format!(
            "{} has no `factory:` block — use `kani-cli build <crate-name>` for non-factory builds",
            path.display()
        ))
    })?;

    let factory_errors = validate::validate_factory(factory);
    if !factory_errors.is_empty() {
        for e in &factory_errors {
            eprintln!("error: {e}");
        }
        return Err(CliError::Other(format!(
            "{} factory validation error(s)",
            factory_errors.len()
        )));
    }

    let base_value: serde_yaml::Value = serde_yaml::from_str(&source)
        .map_err(|e| CliError::Other(format!("YAML re-parse error: {e}")))?;

    let sources = factory.sources.clone();
    for source_def in &sources {
        println!("── Factory source: {}", source_def.id);

        let expanded = apply_factory_overrides(base_value.clone(), source_def);

        let expanded_src = serde_yaml::to_string(&expanded).map_err(|e| {
            CliError::Other(format!(
                "re-serialise error for source '{}': {e}",
                source_def.id
            ))
        })?;

        let expanded_ext: YamlExtension = serde_yaml::from_value(expanded).map_err(|e| {
            CliError::Other(format!(
                "override produced invalid YAML for source '{}': {e}",
                source_def.id
            ))
        })?;

        let validated = validate::validate(&expanded_ext, &expanded_src, path).map_err(|errs| {
            for e in &errs {
                eprintln!("error (source '{}'): {e}", source_def.id);
            }
            CliError::Other(format!(
                "source '{}': {} validation error(s)",
                source_def.id,
                errs.len()
            ))
        })?;

        super::generate::reject_interpreted_only(&validated)?;
        let generated = crate::codegen::generate(&validated, false);

        let crate_dir = ext_root.join(format!("kani-{}", generated.id));

        fs::create_dir_all(crate_dir.join("src"))?;
        fs::write(crate_dir.join("Cargo.toml"), &generated.cargo_toml)?;
        fs::write(crate_dir.join("src").join("lib.rs"), &generated.lib_rs)?;
        if !generated.browser_scripts.is_empty() || !generated.pure_scripts.is_empty() {
            let scripts_dir = crate_dir.join("src").join("scripts");
            fs::create_dir_all(&scripts_dir)?;
            for (name, src) in &generated.browser_scripts {
                fs::write(scripts_dir.join(format!("{name}.js")), src)?;
            }
            for (name, src) in &generated.pure_scripts {
                fs::write(scripts_dir.join(format!("{name}.rhai")), src)?;
            }
        }
        println!("   Generated: {}", crate_dir.display());

        let crate_name = format!("kani-{}", generated.id);
        build_one(&crate_name, &crate_dir, set_version, out_dir, debug)?;
    }

    Ok(())
}

/// Apply per-source field overrides onto the base YAML value tree.
///
/// Precedence (highest first):
/// 1. `source.overrides` dot-path map
/// 2. Named top-level fields (id, name, base_url, language, mihon_source_id)
pub fn apply_factory_overrides(
    mut base: serde_yaml::Value,
    source: &crate::yaml::schema::FactorySource,
) -> serde_yaml::Value {
    {
        let map = match &mut base {
            serde_yaml::Value::Mapping(m) => m,
            _ => return base,
        };

        map.insert(
            serde_yaml::Value::String("id".to_string()),
            serde_yaml::Value::String(source.id.clone()),
        );
        map.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(source.name.clone()),
        );
        map.insert(
            serde_yaml::Value::String("base_url".to_string()),
            serde_yaml::Value::String(source.base_url.clone()),
        );
        map.insert(
            serde_yaml::Value::String("language".to_string()),
            serde_yaml::Value::String(source.language.clone()),
        );
        match source.mihon_source_id {
            Some(id) => {
                map.insert(
                    serde_yaml::Value::String("mihon_source_id".to_string()),
                    serde_yaml::Value::Number(serde_yaml::Number::from(id)),
                );
            }
            None => {
                map.remove("mihon_source_id");
            }
        }
    }

    for (dot_path, value) in &source.overrides {
        set_dot_path(&mut base, dot_path, value.clone());
    }

    base
}

fn set_dot_path(root: &mut serde_yaml::Value, path: &str, value: serde_yaml::Value) {
    let mut parts = path.splitn(2, '.');
    let key = parts.next().expect("non-empty path");
    let rest = parts.next();

    match root {
        serde_yaml::Value::Mapping(map) => {
            let k = serde_yaml::Value::String(key.to_string());
            if let Some(tail) = rest {
                let child = map
                    .entry(k)
                    .or_insert(serde_yaml::Value::Mapping(Default::default()));
                set_dot_path(child, tail, value);
            } else {
                map.insert(k, value);
            }
        }
        _ => {
            eprintln!("warning: override path '{path}' traverses a non-mapping node; skipping");
        }
    }
}

/// Compile one extension crate to a WASM component.
///
/// `crate_dir` matters: extensions live in their own repository now, so the
/// crate is usually *not* a member of the workspace kani-cli was invoked from.
/// `cargo build -p <name>` would resolve against the caller's workspace and
/// fail with "package not found" — every cargo call here is anchored to the
/// crate's own manifest instead, and the artifact is located through the
/// `target_directory` cargo reports rather than assuming `./target`.
fn build_one(
    name: &str,
    crate_dir: &Path,
    set_version: Option<&str>,
    out_dir: &Path,
    debug: bool,
) -> Result<(), CliError> {
    let profile = if debug { "wasm-debug" } else { "wasm-release" };
    println!("── Building {name} [{profile}]");

    let manifest = crate_dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(CliError::Other(format!(
            "{} not found — is {name} an extension crate?",
            manifest.display()
        )));
    }
    let manifest_str = manifest.to_string_lossy().into_owned();

    let mut cargo_args = vec![
        "build",
        "--target",
        "wasm32-unknown-unknown",
        "--profile",
        profile,
        "--manifest-path",
        &manifest_str,
    ];
    // Own the generated argument until `cargo_args` is consumed.
    let version_config;
    if let Some(ver) = set_version {
        version_config = format!("package.version=\"{ver}\"");
        cargo_args.extend(["--config", &version_config]);
    }

    let status = Command::new("cargo").args(&cargo_args).status()?;
    if !status.success() {
        return Err(CliError::BuildFailed(name.to_owned()));
    }

    let metadata_output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            &manifest_str,
        ])
        .output()?;

    let mut ext_id = name.to_string();
    let mut target_dir = PathBuf::from("target");

    if metadata_output.status.success()
        && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&metadata_output.stdout)
    {
        if let Some(dir) = json["target_directory"].as_str() {
            target_dir = PathBuf::from(dir);
        }
        if let Some(pkg) = json["packages"]
            .as_array()
            .and_then(|pkgs| pkgs.iter().find(|p| p["name"] == name))
            && let Some(id) = pkg["metadata"]["id"]
                .as_str()
                .or_else(|| pkg["metadata"]["kani"]["id"].as_str())
        {
            ext_id = id.to_string();
        }
    }

    fs::create_dir_all(out_dir)?;

    let src = target_dir
        .join("wasm32-unknown-unknown")
        .join(profile)
        .join(format!("{}.wasm", name.replace('-', "_")));

    let dest = out_dir.join(format!("{ext_id}.wasm"));
    fs::copy(&src, &dest)?;

    let raw_wasm = fs::read(&src)?;
    if !check_talc_present(&raw_wasm) {
        eprintln!(
            "   warning: extension {} does not use talc allocator; consider adding kani_shared::guest_alloc!() to lib.rs",
            name
        );
    }

    if is_available("wasm-opt") {
        println!("   running wasm-opt");
        let tmp = tmp_path(&dest);
        let mut wasm_opt_args = vec![
            "-Oz",
            "--enable-bulk-memory",
            "--enable-nontrapping-float-to-int",
            "--enable-sign-ext",
        ];
        if debug {
            wasm_opt_args.push("--debuginfo");
        }
        let status = Command::new("wasm-opt")
            .args(&wasm_opt_args)
            .args(["-o"])
            .arg(&tmp)
            .arg(&dest)
            .status()?;
        if status.success() {
            fs::rename(&tmp, &dest)?;
        } else {
            eprintln!("   warning: wasm-opt failed, keeping original");
            if tmp.exists() {
                fs::remove_file(&tmp)?;
            }
        }
    } else {
        println!("   wasm-opt not found, skipping size optimisation");
    }

    if !is_available("wasm-tools") {
        return Err(CliError::WasmToolsMissing);
    }
    println!("   converting to WASM component");
    let tmp = tmp_path(&dest);
    let status = Command::new("wasm-tools")
        .args(["component", "new"])
        .arg(&dest)
        .arg("-o")
        .arg(&tmp)
        .status()?;
    if !status.success() {
        return Err(CliError::BuildFailed(format!(
            "{name}: wasm-tools component conversion failed"
        )));
    }
    fs::rename(&tmp, &dest)?;

    let size_kb = fs::metadata(&dest)?.len() as f64 / 1024.0;
    println!("   done — {size_kb:.2} KB\n");

    Ok(())
}

fn check_talc_present(wasm_bytes: &[u8]) -> bool {
    wasm_bytes.windows(5).any(|w| w == b"talc-")
}

fn tmp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .expect("path should have a file name")
        .to_string_lossy();
    path.with_file_name(format!("{file_name}.tmp"))
}

fn is_available(tool: &str) -> bool {
    match Command::new(tool).arg("--version").output() {
        Ok(_) => true,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

#[cfg(test)]
mod dir_resolution_tests {
    use super::{DirSource, resolve_dir};
    use std::path::{Path, PathBuf};

    fn with_env<T>(key: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let previous = std::env::var(key).ok();
        // SAFETY: no other thread may access the environment during the write. These are the
        // only env mutations in the crate and each test passes a distinct key.
        match value {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        let out = f();
        // SAFETY: as above; restores the value observed before the write.
        match previous {
            Some(p) => unsafe { std::env::set_var(key, p) },
            None => unsafe { std::env::remove_var(key) },
        }
        out
    }

    const NOTHING_EXISTS: &dyn Fn(&Path) -> bool = &|_: &Path| false;

    #[test]
    fn the_flag_wins_over_everything() {
        with_env("KANI_TEST_EXT_DIR", Some("/from/env"), || {
            let (p, src) = resolve_dir(
                Some("/from/flag"),
                "KANI_TEST_EXT_DIR",
                "kani-extensions",
                &|_| true,
            );
            assert_eq!(p, PathBuf::from("/from/flag"));
            assert_eq!(src, DirSource::Flag);
        });
    }

    #[test]
    fn the_env_var_wins_over_a_directory_that_exists_locally() {
        with_env("KANI_TEST_EXT_DIR2", Some("/from/env"), || {
            let (p, src) = resolve_dir(None, "KANI_TEST_EXT_DIR2", "kani-extensions", &|_| true);
            assert_eq!(p, PathBuf::from("/from/env"));
            assert_eq!(src, DirSource::Env);
        });
    }

    #[test]
    fn an_empty_env_var_is_ignored_rather_than_used_as_a_path() {
        with_env("KANI_TEST_EXT_DIR3", Some("   "), || {
            let (p, src) = resolve_dir(None, "KANI_TEST_EXT_DIR3", "kani-extensions", &|_| true);
            assert_eq!(p, PathBuf::from("kani-extensions"));
            assert_eq!(src, DirSource::Local);
        });
    }

    #[test]
    fn a_local_directory_is_preferred_to_the_sibling() {
        with_env("KANI_TEST_EXT_DIR4", None, || {
            let (p, src) = resolve_dir(None, "KANI_TEST_EXT_DIR4", "kani-extensions", &|p| {
                p == Path::new("kani-extensions")
            });
            assert_eq!(p, PathBuf::from("kani-extensions"));
            assert_eq!(src, DirSource::Local);
        });
    }

    #[test]
    fn the_sibling_checkout_is_found_when_there_is_no_local_one() {
        with_env("KANI_TEST_EXT_DIR5", None, || {
            let (p, src) = resolve_dir(None, "KANI_TEST_EXT_DIR5", "kani-extensions", &|p| {
                p == Path::new("../kani-extensions")
            });
            assert_eq!(p, PathBuf::from("../kani-extensions"));
            assert_eq!(src, DirSource::Sibling);
        });
    }

    #[test]
    fn with_neither_present_it_reports_the_local_path_so_the_error_names_it() {
        with_env("KANI_TEST_EXT_DIR6", None, || {
            let (p, src) = resolve_dir(
                None,
                "KANI_TEST_EXT_DIR6",
                "kani-extensions",
                NOTHING_EXISTS,
            );
            assert_eq!(p, PathBuf::from("kani-extensions"));
            assert_eq!(src, DirSource::Local);
        });
    }
}
