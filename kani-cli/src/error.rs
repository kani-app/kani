//! CLI failure types and source-span diagnostic rendering.

#[derive(thiserror::Error, Debug)]
/// Failure returned by command orchestration and extension tooling.
pub enum CliError {
    #[error("build failed for `{0}`")]
    BuildFailed(String),
    #[error("`wasm-tools` not found — install it to produce WASM components")]
    WasmToolsMissing,
    #[error("extension not found: `{0}`")]
    ExtensionNotFound(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Conversion error: {message}")]
    DslConversion {
        message: String,
        span: std::ops::Range<usize>,
    },
    #[error("{0}")]
    Other(String),
}

impl From<kani_yaml::YamlError> for CliError {
    fn from(e: kani_yaml::YamlError) -> Self {
        match e {
            kani_yaml::YamlError::DslConversion { message, span } => {
                Self::DslConversion { message, span }
            }
            kani_yaml::YamlError::DslParse { field_path, .. } => {
                Self::Other(format!("DSL parse failed in {field_path}"))
            }
            kani_yaml::YamlError::Validation(msg) => Self::Other(msg),
        }
    }
}

use ariadne::{Color, Label, Report, ReportKind, Source};

pub(crate) fn report_dsl_errors(
    filename: &str,
    source: &str,
    field_path: Option<&str>,
    errors: &[kani_yaml::dsl::DslParseError],
) {
    for error in errors {
        let range = error.span.clone();
        let headline = match field_path {
            Some(path) => format!("DSL parse error in {path}"),
            None => "DSL parse error".to_string(),
        };
        let mut report = Report::build(ReportKind::Error, (filename, range.clone()))
            .with_message(headline)
            .with_label(
                Label::new((filename, range))
                    .with_message(&error.message)
                    .with_color(Color::Red),
            );
        if let Some(help) = &error.help {
            report = report.with_help(help);
        }
        report
            .finish()
            .eprint((filename, Source::from(source)))
            .expect("failed to write diagnostic to stderr");
    }
}

pub(crate) fn report_custom_error(
    filename: &str,
    source: &str,
    message: &str,
    range: std::ops::Range<usize>,
) {
    Report::build(ReportKind::Error, (filename, range.clone()))
        .with_message("DSL conversion error")
        .with_label(
            Label::new((filename, range))
                .with_message(message)
                .with_color(Color::Yellow),
        )
        .finish()
        .eprint((filename, Source::from(source)))
        .expect("failed to write diagnostic to stderr");
}
