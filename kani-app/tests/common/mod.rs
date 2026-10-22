#![allow(clippy::unwrap_used, dead_code)]
//! Re-exports shared fixtures from the kani-shared-test crate.

pub use kani_shared_test::*;

/// Starts a local HTTP/1.1 server that responds to every GET with a small fake JPEG.
/// Returns the bound port. The server runs until the test process exits.
pub async fn start_mock_page_server() -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const FAKE_BYTES: &[u8] = b"\xff\xd8\xff\xe0FAKE_IMAGE_DATA";

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    FAKE_BYTES.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.write_all(FAKE_BYTES).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    port
}

/// Pins a repository at `url` whose cached index is `index`, signed by a fresh maintainer key
/// that replaces any `maintainer_key` the index names, as a successful add would store it.
pub async fn seed_signed_repo(
    db: &sqlx::SqlitePool,
    url: &str,
    mut index: serde_json::Value,
) -> i64 {
    use kani_app::source::signing::{pubkey_b64, sign_artifact, signature_b64};
    let key = ed25519_dalek::SigningKey::from_bytes(&rand::random::<[u8; 32]>());
    let pk = pubkey_b64(&key);
    index["maintainer_key"] = serde_json::Value::String(pk.clone());
    let name = index["name"].as_str().unwrap_or("Test Repo").to_string();
    let text = index.to_string();
    let sig = signature_b64(&sign_artifact(text.as_bytes(), &key));
    sqlx::query_scalar(
        "INSERT INTO repo_trust (url, name, maintainer_key, index_cache, index_sig) \
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(url)
    .bind(name)
    .bind(pk)
    .bind(text)
    .bind(sig)
    .fetch_one(db)
    .await
    .unwrap()
}
