use crate::error::CliError;
use crate::repl::{resolve, test_cmd};

pub fn run(
    file: &str,
    endpoint: &str,
    args: &[String],
    filters: &[String],
    with_hooks: bool,
) -> Result<(), CliError> {
    let (ext, ep) = test_cmd::load_endpoint(file, endpoint)?;
    let resolved = resolve::resolve(&ext, &ep, endpoint, args, filters)?;

    println!(
        "{} {}",
        resolved.request.method.to_uppercase(),
        resolved.url()
    );
    println!();
    println!("route:   {}", ep.route);
    if !resolved.filters.is_empty() {
        println!("filters:");
        for f in &resolved.filters {
            println!("  {} = {:?}", f.filter_name, f.state);
        }
    }
    println!("queries:");
    for (k, v) in &resolved.request.queries {
        println!("  {k} = {v}");
    }
    if with_hooks {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| CliError::Other(format!("runtime: {e}")))?;
        let hooked = runtime.block_on(async {
            let state = test_cmd::host_state_for(&ext)?;
            crate::repl::resolve::apply_pre_request(&state, &resolved.request)
        })?;
        println!();
        println!("after pre_request:");
        println!("  {} {}", hooked.method.to_uppercase(), hooked_url(&hooked));
    }

    if !resolved.request.headers.is_empty() {
        println!("headers:");
        for (k, v) in &resolved.request.headers {
            println!("  {k}: {v}");
        }
    }
    Ok(())
}

fn hooked_url(req: &kani_shared::ast::RequestDef) -> String {
    resolve::ResolvedRequest {
        request: req.clone(),
        filters: Vec::new(),
    }
    .url()
}
