//! Clap command schema and top-level dispatch for `kani-cli`.

pub mod archive;
pub mod backup_verify;
pub mod build;
pub mod check;
pub mod css;
pub mod dsl_cmd;
pub mod generate;
pub mod icons;
pub mod keygen;
pub mod lint;
pub mod new;
pub mod publish;
pub mod quality;
pub mod repo;
pub mod setup;
pub mod validate;

use crate::error::CliError;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "kani-cli", about = "Kani extension development tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands covered by the 1.x compatibility promise: their names, arguments and output
/// shape may gain optional additions but may not change meaning or be removed within a major
/// version. Every other subcommand is repo plumbing or a diagnostic and carries no such promise,
/// which its help text marks with `[unstable]`.
pub const STABLE_COMMANDS: &[&str] = &[
    "archive-verify",
    "build",
    "check",
    "generate",
    "new",
    "validate",
];

/// Marker appended to the help text of a subcommand outside [`STABLE_COMMANDS`].
pub const UNSTABLE_MARKER: &str = "[unstable]";

#[derive(Subcommand)]
pub enum Command {
    /// Scaffold a new extension (YAML by default, or a Rust/WASM crate with --rust)
    New {
        /// Extension name (e.g. my-source)
        name: String,
        /// Scaffold a Rust/WASM crate instead of a declarative YAML file
        #[arg(long)]
        rust: bool,
    },
    /// Validate a YAML extension file
    Validate {
        /// Path to the YAML file
        file: String,
    },
    /// Check a .yaml or .wasm extension the way the server does before installing it
    Check {
        /// Path to the extension file
        file: String,
    },
    /// Generate Rust source from a YAML extension file
    Generate {
        /// Path to the YAML file
        file: String,
        /// Overwrite an existing generated crate
        #[arg(long)]
        force: bool,
        /// Embed blueprint DSL as precomputed postcard bytes instead of using BlueprintBuilder
        #[arg(long)]
        embedded_bytes: bool,
    },
    /// Compile extension(s) to WASM
    Build {
        /// Extension crate name (e.g. kani-example, or a catalogue crate with --ext-dir)
        #[arg(conflicts_with_all = ["all", "dev"])]
        extension: Option<String>,
        /// Build all production extensions (excludes dev/test extensions)
        #[arg(long, conflicts_with = "dev")]
        all: bool,
        /// Build dev/test extensions only (kani-example, kani-test-abi, kani-fixture-source); excluded from --all
        #[arg(long)]
        dev: bool,
        /// Override the version embedded in the WASM (e.g. 1.2.3)
        #[arg(long, value_name = "SEMVER")]
        set_version: Option<String>,
        /// Directory containing extension crates. Falls back to $KANI_EXT_DIR,
        /// then kani-extensions/, then ../kani-extensions/
        #[arg(long, value_name = "PATH")]
        ext_dir: Option<String>,
        /// Output directory for compiled .wasm files. Falls back to
        /// $KANI_OUT_DIR, then wasm_sources/
        #[arg(long, value_name = "PATH")]
        out_dir: Option<String>,
        /// Build with debug info (larger binary, readable WASM backtraces)
        #[arg(long)]
        debug: bool,
    },
    /// Build the frontend CSS [unstable]
    Css {
        /// Rebuild automatically on file changes
        #[arg(long, conflicts_with = "prod")]
        watch: bool,
        /// Minified production build
        #[arg(long, conflicts_with = "watch")]
        prod: bool,
    },
    /// Download required build tools and JS vendor files [unstable]
    Setup {
        /// Download only the JS vendor files (Preact, htm)
        #[arg(long)]
        vendors: bool,
        /// Download only the Tailwind CSS standalone CLI
        #[arg(long)]
        tailwind: bool,
        /// Download only the esbuild binary
        #[arg(long)]
        esbuild: bool,
    },
    /// Generate PWA icon PNGs from static/icons/kani-mark.svg [unstable]
    Icons,
    /// Parse a DSL expression and print the resulting Expr AST [unstable]
    Dsl {
        /// DSL expression string
        expression: String,
        /// Path to a YAML extension file whose `scripts.pure` block defines user functions
        #[arg(long, value_name = "FILE")]
        scripts: Option<std::path::PathBuf>,
    },
    /// Run the workspace quality checks (clippy, machete, deny, fmt) in sequence [unstable]
    Lint,
    /// Generate an Ed25519 signing keypair for extension authoring [unstable]
    Keygen {
        /// Directory to write the keypair files into (default: current directory)
        #[arg(long, value_name = "PATH", default_value = ".")]
        out_dir: std::path::PathBuf,
        /// Base name for the generated files (e.g. "author" → author.pub + author.key)
        #[arg(long, default_value = "author")]
        name: String,
    },
    /// Sign an extension and publish it to a local repository [unstable]
    Publish {
        /// Path to the extension file (.yaml or .wasm) to publish
        file: std::path::PathBuf,
        /// Path to the author Ed25519 private key file (.key)
        #[arg(long, value_name = "PATH")]
        sign_key: std::path::PathBuf,
        /// Repository root directory (default: current directory)
        #[arg(long, value_name = "PATH", default_value = ".")]
        repo_dir: std::path::PathBuf,
        /// Path to the maintainer private key for signing index.json
        #[arg(long, value_name = "PATH")]
        repo_sign_key: Option<std::path::PathBuf>,
        /// Minimum Kani version required to install this extension
        #[arg(long, value_name = "SEMVER")]
        min_kani_version: Option<String>,
        /// Extension ID for a .wasm artifact; must match the metadata the extension reports
        #[arg(long, value_name = "ID")]
        ext_id: Option<String>,
        /// Display name for a .wasm artifact
        #[arg(long, value_name = "NAME")]
        ext_name: Option<String>,
        /// Version for a .wasm artifact; compared by semver for update detection
        #[arg(long, value_name = "SEMVER")]
        ext_version: Option<String>,
        /// Description shown in the repository listing for a .wasm artifact
        #[arg(long, value_name = "TEXT")]
        ext_description: Option<String>,
        /// Language code for a .wasm artifact (e.g. en, multi)
        #[arg(long, value_name = "LANG")]
        ext_language: Option<String>,
        /// Mark a .wasm artifact as NSFW
        #[arg(long)]
        ext_nsfw: bool,
    },
    /// Manage a local extension repository [unstable]
    #[command(subcommand)]
    Repo(RepoCommand),
    /// REPL: inspect, explain, test, replay, or record a YAML extension [unstable]
    #[command(subcommand)]
    Repl(ReplCommand),
    /// Re-hash every file a Kani archive export claims, without needing Kani
    ArchiveVerify {
        /// Path to the exported `kani-archive` directory
        #[arg(value_name = "ARCHIVE_DIR")]
        path: std::path::PathBuf,
    },
    /// Print the quality score and per-page dimensions for a CBZ [unstable]
    Quality {
        /// Path to a .cbz file
        #[arg(value_name = "CBZ")]
        path: std::path::PathBuf,
    },
    /// Show what a header probe learns from an image's first few kilobytes [unstable]
    Probe {
        /// Path to an image file
        #[arg(value_name = "IMAGE")]
        path: std::path::PathBuf,
    },
    /// Compare two CBZs page by page with perceptual hashes [unstable]
    PhashCompare {
        /// First .cbz
        #[arg(value_name = "A")]
        a: std::path::PathBuf,
        /// Second .cbz
        #[arg(value_name = "B")]
        b: std::path::PathBuf,
    },
    /// Print the manifest computed from a CBZ on disk [unstable]
    Manifest {
        /// Path to a .cbz file
        #[arg(value_name = "CBZ")]
        path: std::path::PathBuf,
    },
    /// Verify a backup archive can be restored onto this build [unstable]
    BackupVerify {
        /// Path to a backup .zip produced by Kani
        #[arg(value_name = "BACKUP_ZIP")]
        path: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
pub enum RepoCommand {
    /// Initialise a new repository directory with an empty index.json
    Init {
        /// Path to the maintainer public key file (.pub)
        #[arg(long, value_name = "PATH")]
        maintainer_key: std::path::PathBuf,
        /// Human-readable repository name
        #[arg(long)]
        name: String,
        /// Directory to initialise as the repository root (default: current directory)
        #[arg(long, value_name = "PATH", default_value = ".")]
        repo_dir: std::path::PathBuf,
    },
    /// Add an already-signed extension artifact to the repository index
    Add {
        /// Path to the signed extension artifact (.yaml or .wasm); its .sig must be alongside
        artifact: std::path::PathBuf,
        /// Path to the author public key file (.pub) used to verify the artifact signature
        #[arg(long, value_name = "PATH")]
        author_key: std::path::PathBuf,
        /// Repository root directory (default: current directory)
        #[arg(long, value_name = "PATH", default_value = ".")]
        repo_dir: std::path::PathBuf,
        /// Minimum Kani version required to install this extension
        #[arg(long, value_name = "SEMVER")]
        min_kani_version: Option<String>,
        /// Path to the maintainer private key for re-signing index.json after update
        #[arg(long, value_name = "PATH")]
        repo_sign_key: Option<std::path::PathBuf>,
    },
    /// List extensions in a repository's index.json
    List {
        /// Repository root directory (default: current directory)
        #[arg(long, value_name = "PATH", default_value = ".")]
        repo_dir: std::path::PathBuf,
    },
    /// Print the fingerprint of a public key file
    ShowFingerprint {
        /// Path to the public key file (.pub)
        #[arg(long, value_name = "PATH")]
        key: std::path::PathBuf,
    },
    /// Verify all extension signatures and SHA-256 digests in a local repository
    Verify {
        /// Repository root directory (default: current directory)
        #[arg(long, value_name = "PATH", default_value = ".")]
        repo_dir: std::path::PathBuf,
        /// Path to maintainer public key to verify index.json (defaults to key in index.json)
        #[arg(long, value_name = "PATH")]
        repo_key: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum ReplCommand {
    /// Show a structured summary of a YAML extension (endpoints, fields, filters)
    Inspect {
        /// Path to the YAML extension file
        file: String,
    },
    /// Parse a DSL expression and print its evaluation trace as an indented tree
    Explain {
        /// DSL expression to trace (e.g. 'first("a").attr("href").split("/").at(-1)')
        expression: String,
    },
    /// Run an endpoint against a HAR fixture and assert the row count
    Test {
        /// Path to the YAML extension file
        file: String,
        /// Path to the HAR fixture file
        har: String,
        /// Endpoint name: popular, search, manga_details, chapter_list, pages
        endpoint: String,
        /// Expected number of rows
        expected_count: usize,
        /// URL fragment identifying the HAR entry, when the endpoint's route does
        /// not appear in it
        #[arg(long)]
        url_contains: Option<String>,
    },
    /// Run an endpoint against a HAR fixture and diff the output against an expected JSON file
    Replay {
        /// Path to the YAML extension file
        file: String,
        /// Path to the HAR fixture file
        har: String,
        /// Endpoint name: popular, search, manga_details, chapter_list, pages
        endpoint: String,
        /// Path to the expected JSON output file
        expected: String,
        /// URL fragment identifying the HAR entry, when the endpoint's route does
        /// not appear in it
        #[arg(long)]
        url_contains: Option<String>,
    },
    /// Make a live HTTP request to an endpoint and save the response as a HAR file
    /// Resolve an endpoint's request and print it without sending anything
    Request {
        /// Path to the YAML extension file
        file: String,
        /// Endpoint name: popular, search, manga_details, chapter_list, pages
        endpoint: String,
        /// Arguments for route placeholders and query params (e.g. manga_id=abc page=1)
        args: Vec<String>,
        /// Override a filter, repeatable (e.g. --filter content_rating=safe,suggestive)
        #[arg(long = "filter")]
        filters: Vec<String>,
        /// Also run the pre_request hook and show the request it produces
        #[arg(long)]
        with_hooks: bool,
    },
    Record {
        /// Path to the YAML extension file
        file: String,
        /// Endpoint name: popular, search, manga_details, chapter_list, pages
        endpoint: String,
        /// Arguments for route placeholders and query params (e.g. manga_id=abc page=1)
        args: Vec<String>,
        /// Override a filter, repeatable (e.g. --filter content_rating=safe,suggestive)
        #[arg(long = "filter")]
        filters: Vec<String>,
        /// Output HAR file path
        #[arg(long, short, default_value = "recorded.har")]
        output: String,
    },
}

pub fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::New { name, rust } => new::run(&name, rust),
        Command::Validate { file } => validate::run(&file),
        Command::Check { file } => check::run(&file),
        Command::Generate {
            file,
            force,
            embedded_bytes,
        } => generate::run(&file, force, embedded_bytes).map(|_| ()),
        Command::Build {
            extension,
            all,
            dev,
            set_version,
            ext_dir,
            out_dir,
            debug,
        } => build::run(
            extension.as_deref(),
            all,
            dev,
            set_version.as_deref(),
            ext_dir.as_deref(),
            out_dir.as_deref(),
            debug,
        ),
        Command::Css { watch, prod } => css::run(watch, prod),
        Command::Setup {
            vendors,
            tailwind,
            esbuild,
        } => setup::run(vendors, tailwind, esbuild),
        Command::ArchiveVerify { path } => archive::verify(&path),
        Command::Quality { path } => quality::score(&path),
        Command::Probe { path } => quality::probe(&path),
        Command::PhashCompare { a, b } => quality::phash_compare(&a, &b),
        Command::Manifest { path } => archive::manifest(&path),
        Command::Icons => icons::run(),
        Command::Dsl {
            expression,
            scripts,
        } => dsl_cmd::run(&expression, scripts.as_deref()),
        Command::Lint => lint::run(),
        Command::Keygen { out_dir, name } => keygen::run(&out_dir, &name),
        Command::Publish {
            file,
            sign_key,
            repo_dir,
            repo_sign_key,
            min_kani_version,
            ext_id,
            ext_name,
            ext_version,
            ext_description,
            ext_language,
            ext_nsfw,
        } => publish::run(
            &file,
            &sign_key,
            &repo_dir,
            repo_sign_key.as_deref(),
            min_kani_version.as_deref(),
            &publish::WasmMetadata {
                id: ext_id,
                name: ext_name,
                version: ext_version,
                description: ext_description,
                language: ext_language,
                nsfw: ext_nsfw,
            },
        ),
        Command::Repo(repo_cmd) => match repo_cmd {
            RepoCommand::Init {
                maintainer_key,
                name,
                repo_dir,
            } => repo::run_init(&repo_dir, &name, &maintainer_key),
            RepoCommand::Add {
                artifact,
                author_key,
                repo_dir,
                min_kani_version,
                repo_sign_key,
            } => repo::run_add(
                &artifact,
                &author_key,
                &repo_dir,
                min_kani_version.as_deref(),
                repo_sign_key.as_deref(),
            ),
            RepoCommand::List { repo_dir } => repo::run_list(&repo_dir),
            RepoCommand::ShowFingerprint { key } => repo::run_show_fingerprint(&key),
            RepoCommand::Verify { repo_dir, repo_key } => {
                repo::run_verify(&repo_dir, repo_key.as_deref())
            }
        },
        Command::BackupVerify { path } => backup_verify::run(&path),
        Command::Repl(repl_cmd) => match repl_cmd {
            ReplCommand::Inspect { file } => crate::repl::inspect::run(&file),
            ReplCommand::Explain { expression } => crate::repl::explain::run(&expression),
            ReplCommand::Test {
                file,
                har,
                endpoint,
                expected_count,
                url_contains,
            } => crate::repl::test_cmd::run_test(
                &file,
                &har,
                &endpoint,
                expected_count,
                url_contains.as_deref(),
            ),
            ReplCommand::Replay {
                file,
                har,
                endpoint,
                expected,
                url_contains,
            } => crate::repl::test_cmd::run_replay(
                &file,
                &har,
                &endpoint,
                &expected,
                url_contains.as_deref(),
            ),
            ReplCommand::Request {
                file,
                endpoint,
                args,
                filters,
                with_hooks,
            } => crate::repl::request::run(&file, &endpoint, &args, &filters, with_hooks),
            ReplCommand::Record {
                file,
                endpoint,
                args,
                filters,
                output,
            } => crate::repl::record::run(&file, &endpoint, &args, &filters, &output),
        },
    }
}
