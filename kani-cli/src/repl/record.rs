use crate::error::CliError;
use crate::repl::{har, test_cmd};

pub fn run(
    file: &str,
    endpoint: &str,
    args: &[String],
    filters: &[String],
    output: &str,
) -> Result<(), CliError> {
    let (ext, ep) = test_cmd::load_endpoint(file, endpoint)?;
    let resolved = crate::repl::resolve::resolve(&ext, &ep, endpoint, args, filters)?;
    let url = resolved.url();
    let request = resolved.request;

    println!("Fetching: {url}");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::Other(format!("runtime: {e}")))?;

    let body = runtime.block_on(async {
        let mut state = test_cmd::host_state_for(&ext)?;
        kani_core::evaluator::shared::fetch_body(&mut state, &request)
            .await
            .map_err(CliError::Other)
    })?;

    let mime = match ep.response_type {
        crate::yaml::schema::ResponseType::Json => "application/json",
        crate::yaml::schema::ResponseType::Html => "text/html",
    };
    println!("Recorded {} byte(s) as {mime}", body.len());

    let har_doc = har::Har {
        log: har::HarLog {
            entries: vec![har::HarEntry {
                request: har::HarRequest {
                    method: ep.method.to_uppercase(),
                    url,
                },
                response: har::HarResponse {
                    status: 200,
                    content: har::HarContent {
                        mime_type: mime.to_string(),
                        text: Some(body),
                    },
                },
            }],
        },
    };

    let json =
        serde_json::to_string_pretty(&har_doc).map_err(|e| CliError::Other(e.to_string()))?;
    std::fs::write(output, json)?;
    println!("Recorded to {output}");
    Ok(())
}
