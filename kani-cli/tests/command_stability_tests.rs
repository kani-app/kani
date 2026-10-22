#![allow(clippy::unwrap_used)]

//! The tier a subcommand advertises is the one users act on, so the check runs against the help
//! clap actually renders rather than against the source. Both directions matter: an unmarked
//! unstable command over-promises, and a marked stable one under-promises.

use clap::CommandFactory;
use kani_cli::commands::{Cli, STABLE_COMMANDS, UNSTABLE_MARKER};

fn subcommand_help() -> Vec<(String, String)> {
    Cli::command()
        .get_subcommands()
        .map(|c| {
            let about = c.get_about().map(|a| a.to_string()).unwrap_or_default();
            (c.get_name().to_string(), about)
        })
        .collect()
}

#[test]
fn every_subcommand_advertises_its_tier() {
    let mut problems = Vec::new();
    for (name, about) in subcommand_help() {
        let declared_stable = STABLE_COMMANDS.contains(&name.as_str());
        let marked_unstable = about.contains(UNSTABLE_MARKER);

        if declared_stable && marked_unstable {
            problems.push(format!(
                "  {name} is declared stable but its help says unstable"
            ));
        }
        if !declared_stable && !marked_unstable {
            problems.push(format!(
                "  {name} is not in STABLE_COMMANDS but its help does not say {UNSTABLE_MARKER}"
            ));
        }
        if about.is_empty() {
            problems.push(format!("  {name} has no help text to carry a tier"));
        }
    }
    problems.sort();

    assert!(
        problems.is_empty(),
        "{} subcommand(s) advertise the wrong compatibility tier:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn the_stable_list_names_only_commands_that_exist() {
    let names: Vec<String> = subcommand_help().into_iter().map(|(n, _)| n).collect();
    let mut unknown: Vec<&str> = STABLE_COMMANDS
        .iter()
        .filter(|s| !names.iter().any(|n| n == *s))
        .copied()
        .collect();
    unknown.sort_unstable();

    assert!(
        unknown.is_empty(),
        "STABLE_COMMANDS names {unknown:?}, which the CLI does not define"
    );
}

#[test]
fn the_split_is_not_vacuous() {
    let help = subcommand_help();
    let unstable = help
        .iter()
        .filter(|(_, a)| a.contains(UNSTABLE_MARKER))
        .count();

    assert!(
        help.len() > 10,
        "only {} subcommands enumerated, so the scan is not reading clap",
        help.len()
    );
    assert!(
        !STABLE_COMMANDS.is_empty() && unstable > 0,
        "the tier split must partition the CLI, got {} stable and {unstable} unstable",
        STABLE_COMMANDS.len()
    );
}

#[test]
fn the_authoring_pipeline_stays_stable() {
    for name in ["new", "validate", "generate", "build"] {
        assert!(
            STABLE_COMMANDS.contains(&name),
            "{name} is part of the documented extension-authoring pipeline"
        );
    }
}

#[test]
fn archive_verify_stays_stable() {
    assert!(
        STABLE_COMMANDS.contains(&"archive-verify"),
        "archive-verify is the export promise users run when Kani itself will not start"
    );
}

/// `-o` after the positional arguments must still bind to `output`. A trailing
/// var-arg would swallow it into `args`, silently writing the default path.
#[test]
fn record_output_flag_binds_after_positional_arguments() {
    use clap::Parser;
    use kani_cli::commands::{Command, ReplCommand};

    let cli = Cli::try_parse_from([
        "kani-cli",
        "repl",
        "record",
        "ext.yaml",
        "popular",
        "page=1",
        "-o",
        "chosen.har",
    ])
    .expect("record args parse");

    let Command::Repl(ReplCommand::Record { output, args, .. }) = cli.command else {
        panic!("expected repl record");
    };

    assert_eq!(output, "chosen.har", "-o did not bind; args were {args:?}");
    assert_eq!(args, vec!["page=1".to_string()]);
}
