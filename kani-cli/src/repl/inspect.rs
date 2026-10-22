use crate::error::CliError;
use crate::yaml::{
    model::{ValidatedEndpoint, ValidatedHnp, ValidatedPopular, ValidatedTotalPages},
    schema::YamlExtension,
    validate,
};
use std::path::Path;

pub fn run(file: &str) -> Result<(), CliError> {
    let path = Path::new(file);
    let src = std::fs::read_to_string(path)?;
    let ext: YamlExtension = serde_yaml::from_str(&src)
        .map_err(|e| CliError::Other(format!("YAML parse error: {e}")))?;
    let validated = validate::validate(&ext, &src, path).map_err(|errors| {
        for e in &errors {
            eprintln!("  {e}");
        }
        CliError::Other(format!("{} validation error(s)", errors.len()))
    })?;

    println!("Extension: {} ({})", validated.name, validated.id);
    println!("Version:   {}", validated.version);
    println!("Base URL:  {}", validated.base_url);
    println!("Language:  {}", validated.language);
    if validated.nsfw {
        println!("NSFW:      yes");
    }

    println!("\nEndpoints:");
    if let Some(popular) = &validated.popular {
        match popular {
            ValidatedPopular::Delegated { delegate_to, .. } => {
                println!("  popular  → delegates to {delegate_to}");
            }
            ValidatedPopular::Full(ep) => {
                println!("  popular  GET {}", ep.route);
                print_endpoint_fields(ep, "    ");
            }
        }
    }
    if let Some(ep) = &validated.search {
        println!("  search   {} {}", ep.method.to_uppercase(), ep.route);
        print_endpoint_fields(ep, "    ");
    }
    if let Some(ep) = &validated.manga_details {
        println!("  details  {} {}", ep.method.to_uppercase(), ep.route);
        print_endpoint_fields(ep, "    ");
    }
    if let Some(ep) = &validated.chapter_list {
        println!("  chapters {} {}", ep.method.to_uppercase(), ep.route);
        print_endpoint_fields(ep, "    ");
    }
    if let Some(ep) = &validated.pages {
        println!("  pages    {} {}", ep.method.to_uppercase(), ep.route);
        print_endpoint_fields(ep, "    ");
    }

    if !validated.filters.is_empty() {
        println!("\nFilters ({}):", validated.filters.len());
        for f in &validated.filters {
            println!("  {} — {} ({:?})", f.id, f.name, f.kind);
        }
    }

    if !validated.preferences.is_empty() {
        println!("\nPreferences ({}):", validated.preferences.len());
        for p in &validated.preferences {
            println!("  {} — {}", p.key, p.label);
        }
    }

    Ok(())
}

fn print_endpoint_fields(ep: &ValidatedEndpoint, indent: &str) {
    println!("{indent}container: {:?}", ep.container);
    println!("{indent}response:  {:?}", ep.response_type);
    let hnp = match &ep.has_next_page {
        ValidatedHnp::Static(b) => format!("static({b})"),
        ValidatedHnp::Scalar(_) => "expr".to_string(),
        ValidatedHnp::Default => "derived(total_pages, then pagination, else false)".to_string(),
    };
    let tp = match &ep.total_pages {
        ValidatedTotalPages::Static(n) => format!("static({n})"),
        ValidatedTotalPages::Scalar(_) => "expr".to_string(),
        ValidatedTotalPages::None => "none".to_string(),
    };
    println!("{indent}hnp:       {hnp}  total_pages: {tp}");

    if !ep.fields.is_empty() {
        println!("{indent}fields ({}):", ep.fields.len());
        for f in &ep.fields {
            let opt = if f.optional { " (optional)" } else { "" };
            println!("{indent}  {}{opt}", f.name);
        }
    }
}
