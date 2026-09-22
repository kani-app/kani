use std::sync::Arc;

use async_trait::async_trait;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::authentication::Credentials,
};

use crate::models::Settings;

/// Provider-agnostic email sending interface.
/// Implement this trait to add a new email provider.
#[async_trait]
pub(crate) trait EmailTransport: Send + Sync {
    async fn send(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        html_body: &str,
    ) -> Result<(), String>;
}

/// General-purpose email service. Holds a provider backend and the configured from-address.
/// Cheap to clone — the transport is behind an Arc.
#[derive(Clone)]
pub struct EmailService {
    transport: Arc<dyn EmailTransport>,
    pub from_address: String,
}

impl EmailService {
    /// Constructs from current settings. Returns `None` if email is disabled or unconfigured.
    pub(crate) fn from_settings(s: &Settings) -> Option<Self> {
        if !s.email_enabled || s.email_from_address.is_empty() {
            return None;
        }
        let config: serde_json::Value =
            serde_json::from_str(&s.email_provider_config).unwrap_or_default();
        let transport: Arc<dyn EmailTransport> = match s.email_provider.as_str() {
            "smtp" | "" => Arc::new(SmtpEmailTransport::from_config(&config)?),
            unknown => {
                tracing::warn!("Unknown email provider: {unknown}");
                return None;
            }
        };
        Some(Self {
            transport,
            from_address: s.email_from_address.clone(),
        })
    }

    pub async fn send(&self, to: &str, subject: &str, html: &str) -> Result<(), String> {
        self.transport
            .send(&self.from_address, to, subject, html)
            .await
    }

    /// Builds a service over a capturing transport, for tests.
    ///
    /// `from_settings` is the only other constructor and it always builds SMTP,
    /// so without this the trait has one implementation and no way to reach the
    /// boundary at all. Takes the concrete type rather than `dyn EmailTransport`
    /// because the trait is crate-private and an integration test is not.
    #[cfg(any(test, feature = "test-util"))]
    pub fn with_capture(from_address: String, transport: Arc<CapturingEmailTransport>) -> Self {
        Self {
            transport,
            from_address,
        }
    }
}

/// An `EmailTransport` that records what it was asked to send.
#[cfg(any(test, feature = "test-util"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentEmail {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub html_body: String,
}

#[cfg(any(test, feature = "test-util"))]
#[derive(Default)]
pub struct CapturingEmailTransport {
    sent: std::sync::Mutex<Vec<SentEmail>>,
    fail_with: Option<String>,
    notify: tokio::sync::Notify,
}

#[cfg(any(test, feature = "test-util"))]
impl CapturingEmailTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the send as usual, then reports the given error to the caller.
    pub fn failing(message: impl Into<String>) -> Self {
        Self {
            fail_with: Some(message.into()),
            ..Self::default()
        }
    }

    pub fn sent(&self) -> Vec<SentEmail> {
        self.sent.lock().expect("capture lock poisoned").clone()
    }

    /// Waits until at least `n` messages have been recorded.
    ///
    /// `send_email_bg` dispatches from a spawned task, so a test that reads
    /// `sent()` straight after the call races the runtime and passes or fails
    /// on timing.
    pub async fn wait_for(&self, n: usize, within: std::time::Duration) -> Vec<SentEmail> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let got = self.sent();
            if got.len() >= n {
                return got;
            }
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero()
                || tokio::time::timeout(left, self.notify.notified())
                    .await
                    .is_err()
            {
                return self.sent();
            }
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl EmailTransport for CapturingEmailTransport {
    async fn send(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        html_body: &str,
    ) -> Result<(), String> {
        self.sent
            .lock()
            .expect("capture lock poisoned")
            .push(SentEmail {
                from: from.to_owned(),
                to: to.to_owned(),
                subject: subject.to_owned(),
                html_body: html_body.to_owned(),
            });
        self.notify.notify_waiters();
        match &self.fail_with {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

struct SmtpEmailTransport {
    inner: AsyncSmtpTransport<Tokio1Executor>,
}

impl SmtpEmailTransport {
    fn from_config(cfg: &serde_json::Value) -> Option<Self> {
        let host = cfg
            .get("host")
            .and_then(|v| v.as_str())
            .filter(|h| !h.is_empty())?;
        let port = cfg.get("port").and_then(|v| v.as_u64()).unwrap_or(587) as u16;
        let username = cfg
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let password = cfg
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let tls_mode = cfg
            .get("tls_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("starttls");

        let creds = if !username.is_empty() {
            Some(Credentials::new(username.to_string(), password.to_string()))
        } else {
            None
        };

        let transport = match tls_mode {
            "tls" => {
                let mut b = AsyncSmtpTransport::<Tokio1Executor>::relay(host).ok()?;
                b = b.port(port);
                if let Some(c) = creds {
                    b = b.credentials(c);
                }
                b.build()
            }
            "none" | "plain" => {
                let mut b = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host);
                b = b.port(port);
                if let Some(c) = creds {
                    b = b.credentials(c);
                }
                b.build()
            }
            _ => {
                let mut b = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host).ok()?;
                b = b.port(port);
                if let Some(c) = creds {
                    b = b.credentials(c);
                }
                b.build()
            }
        };

        Some(Self { inner: transport })
    }
}

pub(crate) fn build_message(
    from: &str,
    to: &str,
    subject: &str,
    html_body: &str,
) -> Result<lettre::Message, String> {
    let from_mb: Mailbox = from
        .parse()
        .map_err(|e| format!("Invalid from address: {e}"))?;
    let to_mb: Mailbox = to.parse().map_err(|e| format!("Invalid to address: {e}"))?;

    let message_id = format!("<{}@{}>", uuid::Uuid::new_v4(), from_mb.email.domain());

    lettre::Message::builder()
        .from(from_mb)
        .to(to_mb)
        .message_id(Some(message_id))
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(html_body.to_string())
        .map_err(|e| format!("Failed to build email: {e}"))
}

#[async_trait]
impl EmailTransport for SmtpEmailTransport {
    async fn send(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        html_body: &str,
    ) -> Result<(), String> {
        let message = build_message(from, to, subject, html_body)?;

        self.inner
            .send(message)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Generates a (raw_token, sha256_hash) pair.
/// `raw_token` is sent to the user; only `sha256_hash` is stored in the DB.
pub(crate) fn generate_token() -> (String, String) {
    use sha2::{Digest, Sha256};

    let bytes: [u8; 32] = rand::random();
    let raw = hex::encode(bytes);
    let hash = hex::encode(Sha256::digest(raw.as_bytes()));
    (raw, hash)
}

/// SHA-256 hex of the given raw token string.
pub(crate) fn hash_token(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(raw.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_parser::{MessageParser, MimeHeaders};

    const FROM: &str = "Kani <kani@example.com>";
    const TO: &str = "reader@example.com";

    fn formatted(from: &str, to: &str, subject: &str, html: &str) -> Vec<u8> {
        build_message(from, to, subject, html)
            .expect("message should build")
            .formatted()
    }

    #[tokio::test]
    async fn service_passes_every_field_through_to_the_transport() {
        let transport = Arc::new(CapturingEmailTransport::new());
        let svc = EmailService::with_capture(FROM.to_owned(), Arc::clone(&transport));

        svc.send(TO, "Reset your password", "<p>link</p>")
            .await
            .expect("capturing transport accepts");

        assert_eq!(
            transport.sent(),
            vec![SentEmail {
                from: FROM.to_owned(),
                to: TO.to_owned(),
                subject: "Reset your password".to_owned(),
                html_body: "<p>link</p>".to_owned(),
            }],
            "the service must forward its from-address and all three arguments unchanged"
        );
    }

    #[tokio::test]
    async fn service_propagates_a_transport_failure() {
        let transport = Arc::new(CapturingEmailTransport::failing("relay refused"));
        let svc = EmailService::with_capture(FROM.to_owned(), Arc::clone(&transport));

        let err = svc
            .send(TO, "Verify your email", "<p>code</p>")
            .await
            .expect_err("a failing transport must surface its error");

        assert_eq!(err, "relay refused");
        assert_eq!(
            transport.sent().len(),
            1,
            "the attempt still reaches the transport"
        );
    }

    fn header_block(raw: &[u8]) -> Vec<u8> {
        let end = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("a message must separate headers from body with a blank line");
        raw[..end].to_vec()
    }

    fn header_names(raw: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(&header_block(raw))
            .split("\r\n")
            .filter(|line| !line.starts_with(' ') && !line.starts_with('\t'))
            .filter_map(|line| line.split_once(':'))
            .map(|(name, _)| name.to_ascii_lowercase())
            .collect()
    }

    #[test]
    fn a_rendered_email_is_valid_mime() {
        let (subject, html) = crate::service::email_templates::email_verification_email(
            "reader",
            "https://kani.example.com/verify-email?token=abc123",
        );
        let raw = formatted(FROM, TO, &subject, &html);

        let parsed = MessageParser::default()
            .parse(&raw)
            .expect("a mail client must be able to parse the message");

        assert_eq!(
            parsed
                .from()
                .and_then(|a| a.first())
                .and_then(|a| a.address()),
            Some("kani@example.com")
        );
        assert_eq!(
            parsed
                .to()
                .and_then(|a| a.first())
                .and_then(|a| a.address()),
            Some(TO)
        );
        assert_eq!(parsed.subject(), Some(subject.as_str()));
        assert!(parsed.date().is_some(), "a message needs a Date header");
        let message_id = parsed
            .message_id()
            .expect("a message needs a Message-ID header, or spam filters penalise it");
        assert!(
            message_id.ends_with("@example.com"),
            "the Message-ID domain should match the From domain: {message_id}"
        );
        assert_eq!(parsed.content_type().map(|c| c.ctype()), Some("text"));
        assert_eq!(
            parsed.content_type().and_then(|c| c.subtype()),
            Some("html")
        );

        let body = parsed
            .body_html(0)
            .expect("the html part must decode")
            .to_string();
        assert!(body.contains("Verify Email"), "decoded body: {body}");
    }

    #[test]
    fn a_subject_with_non_ascii_is_encoded_word_wrapped() {
        let subject = "Vérifiez votre adresse ✉";
        let raw = formatted(FROM, TO, subject, "<p>hi</p>");

        let headers = header_block(&raw);
        assert!(
            headers.is_ascii(),
            "headers must not carry raw non-ASCII bytes: {}",
            String::from_utf8_lossy(&headers)
        );
        assert!(
            String::from_utf8_lossy(&headers)
                .to_ascii_lowercase()
                .contains("=?utf-8?"),
            "a non-ASCII subject must be RFC 2047 encoded"
        );

        let parsed = MessageParser::default()
            .parse(&raw)
            .expect("the message must parse");
        assert_eq!(parsed.subject(), Some(subject));
    }

    #[test]
    fn header_injection_via_a_display_name_is_impossible() {
        for injected in [
            "Kani\r\nBcc: attacker@example.com <kani@example.com>",
            "\"Kani\r\nBcc: attacker@example.com\" <kani@example.com>",
        ] {
            assert!(
                build_message(injected, TO, "Hello", "<p>hi</p>").is_err(),
                "a from-address carrying CRLF must be rejected: {injected:?}"
            );
        }

        assert!(
            build_message(
                FROM,
                "reader@example.com\r\nBcc: attacker@example.com",
                "Hello",
                "<p>hi</p>"
            )
            .is_err(),
            "a recipient carrying CRLF must be rejected"
        );
    }

    #[test]
    fn header_injection_via_the_subject_is_impossible() {
        let clean = header_names(&formatted(FROM, TO, "Reset", "<p>hi</p>"));
        let injected = header_names(&formatted(
            FROM,
            TO,
            "Reset\r\nBcc: attacker@example.com",
            "<p>hi</p>",
        ));
        assert_eq!(
            injected, clean,
            "a CRLF in the subject changed the header set"
        );
        assert!(!injected.iter().any(|name| name == "bcc"));

        let raw = formatted(FROM, TO, "Reset\r\nBcc: attacker@example.com", "<p>hi</p>");
        let parsed = MessageParser::default()
            .parse(&raw)
            .expect("the message must parse");
        assert!(
            parsed.bcc().is_none(),
            "a CRLF in the subject produced a Bcc recipient"
        );
    }

    #[test]
    fn the_header_name_scan_sees_an_extra_header() {
        let raw = b"From: a@b.c\r\nBcc: attacker@example.com\r\nSubject: x\r\n\r\nbody";
        assert_eq!(header_names(raw), vec!["from", "bcc", "subject"]);
        let folded = b"From: a@b.c\r\nSubject: long\r\n Bcc: attacker@example.com\r\n\r\nbody";
        assert_eq!(header_names(folded), vec!["from", "subject"]);
    }

    #[test]
    fn a_verification_link_survives_transfer_encoding() {
        let token = "a".repeat(64);
        let url = format!("https://kani.example.com/verify-email?token={token}");
        let (subject, html) =
            crate::service::email_templates::email_verification_email("reader", &url);
        let raw = formatted(FROM, TO, &subject, &html);

        let parsed = MessageParser::default()
            .parse(&raw)
            .expect("the message must parse");
        let body = parsed
            .body_html(0)
            .expect("the html part must decode")
            .to_string();

        assert!(
            body.contains(&format!("href=\"{url}\"")),
            "the link did not survive encoding: {body}"
        );
        assert!(
            body.matches(url.as_str()).count() >= 2,
            "the copyable link must survive alongside the button"
        );
    }

    #[test]
    fn the_message_declares_how_its_body_is_encoded() {
        let raw = formatted(FROM, TO, "Hello", &"<p>padding</p>".repeat(50));
        let headers = String::from_utf8_lossy(&header_block(&raw)).to_ascii_lowercase();
        assert!(
            headers.contains("content-transfer-encoding:"),
            "a client cannot decode a body whose encoding is undeclared: {headers}"
        );
    }

    #[test]
    fn generated_tokens_are_unique_and_only_their_hash_is_storable() {
        let (raw_a, hash_a) = generate_token();
        let (raw_b, hash_b) = generate_token();
        assert_ne!(raw_a, raw_b);
        assert_ne!(hash_a, hash_b);
        assert_eq!(raw_a.len(), 64);
        assert_eq!(hash_a, hash_token(&raw_a));
        assert_ne!(hash_a, raw_a);
    }
}
