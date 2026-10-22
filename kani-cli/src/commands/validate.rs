use crate::error::{CliError, report_dsl_errors};
use crate::yaml::{schema::YamlExtension, validate::validate};
use std::path::Path;

pub fn run(file: &str) -> Result<(), CliError> {
    let path = Path::new(file);
    let source = std::fs::read_to_string(path)?;

    let ext: YamlExtension = serde_yaml::from_str(&source)
        .map_err(|e| CliError::Other(format!("YAML parse error: {e}")))?;

    match validate(&ext, &source, path) {
        Ok(validated) => {
            println!("✓ {} ({}) — valid", validated.name, validated.id);
            Ok(())
        }
        Err(errors) => {
            for e in &errors {
                match e {
                    kani_yaml::YamlError::DslParse {
                        field_path,
                        expression,
                        errors,
                    } => {
                        report_dsl_errors(file, expression, Some(field_path), errors);
                    }
                    _ => eprintln!("error: {e}"),
                }
            }
            Err(CliError::Other(format!(
                "{} validation error(s)",
                errors.len()
            )))
        }
    }
}
