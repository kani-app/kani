fn escape_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn base_layout(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
  body {{ font-family: sans-serif; background: #0f0f17; color: #ddddf0; margin: 0; padding: 32px 16px; }}
  .card {{ max-width: 520px; margin: 0 auto; background: #18181f; border-radius: 12px; padding: 32px; }}
  .header {{ font-size: 22px; font-weight: 700; color: #e8545a; margin-bottom: 24px; }}
  p {{ line-height: 1.6; color: #b0b0c8; }}
  .footer {{ margin-top: 28px; font-size: 12px; color: #7878a0; }}
  a {{ color: #e8545a; }}
</style>
</head>
<body>
<div class="card">
  <div class="header">Kani</div>
  {body}
  <div class="footer">You received this email from your Kani instance.</div>
</div>
</body>
</html>"#
    )
}

fn action_button(label: &str, url: &str) -> String {
    let url = escape_html(url);
    format!(
        r#"<p style="text-align:center;margin:28px 0;">
  <a href="{url}" style="background:#e8545a;color:#fff;text-decoration:none;padding:12px 28px;border-radius:8px;font-weight:600;display:inline-block;">{label}</a>
</p>
<p style="text-align:center;font-size:12px;color:#7878a0;">Or copy this link: <a href="{url}">{url}</a></p>"#
    )
}

pub(crate) fn password_reset_email(username: &str, reset_url: &str) -> (String, String) {
    let username = escape_html(username);
    let subject = "Reset your Kani password".to_string();
    let body = format!(
        r#"<p>Hi <strong>{username}</strong>,</p>
<p>We received a request to reset your password. Click the button below to set a new one. This link expires in <strong>1 hour</strong>.</p>
{btn}
<p>If you did not request a password reset, you can safely ignore this email — your password will not change.</p>"#,
        btn = action_button("Reset Password", reset_url)
    );
    (subject, base_layout("Reset your Kani password", &body))
}

pub(crate) fn email_verification_email(username: &str, verify_url: &str) -> (String, String) {
    let username = escape_html(username);
    let subject = "Verify your Kani email address".to_string();
    let body = format!(
        r#"<p>Hi <strong>{username}</strong>,</p>
<p>Please verify your email address by clicking the button below. This link expires in <strong>24 hours</strong>.</p>
{btn}
<p>If you did not create a Kani account, you can safely ignore this email.</p>"#,
        btn = action_button("Verify Email", verify_url)
    );
    (subject, base_layout("Verify your email address", &body))
}

pub(crate) fn password_changed_email(username: &str) -> (String, String) {
    let username = escape_html(username);
    let subject = "Your Kani password was changed".to_string();
    let body = format!(
        r#"<p>Hi <strong>{username}</strong>,</p>
<p>This is a confirmation that your Kani password was successfully changed.</p>
<p>If you did not make this change, please contact your administrator immediately.</p>"#
    );
    (subject, base_layout("Password changed", &body))
}

pub(crate) fn admin_password_reset_email(username: &str) -> (String, String) {
    let username = escape_html(username);
    let subject = "An administrator reset your Kani password".to_string();
    let body = format!(
        r#"<p>Hi <strong>{username}</strong>,</p>
<p>An administrator has initiated a password reset for your account. You should receive a separate email with a reset link shortly.</p>
<p>If you did not request this, please contact your administrator.</p>"#
    );
    (
        subject,
        base_layout("Admin-initiated password reset", &body),
    )
}

pub(crate) fn welcome_email(username: &str) -> (String, String) {
    let username = escape_html(username);
    let subject = "Welcome to Kani".to_string();
    let body = format!(
        r#"<p>Hi <strong>{username}</strong>,</p>
<p>Your Kani account has been created. You can now sign in and start exploring your library.</p>"#
    );
    (subject, base_layout("Welcome to Kani", &body))
}

pub(crate) fn test_email() -> (String, String) {
    let subject = "Kani email test".to_string();
    let body = "<p>This is a test email from your Kani instance. If you received it, your email configuration is working correctly.</p>".to_string();
    (subject, base_layout("Email test", &body))
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://kani.example.com/reset?token=deadbeef";

    fn all_templates() -> Vec<(String, String)> {
        vec![
            password_reset_email("reader", URL),
            email_verification_email("reader", URL),
            password_changed_email("reader"),
            admin_password_reset_email("reader"),
            welcome_email("reader"),
            test_email(),
        ]
    }

    #[test]
    fn every_subject_is_a_single_header_safe_line() {
        for (subject, _) in all_templates() {
            assert!(!subject.trim().is_empty());
            assert!(
                !subject.contains('\r') && !subject.contains('\n'),
                "subject spans lines: {subject:?}"
            );
        }
    }

    #[test]
    fn every_body_is_a_complete_html_document() {
        for (_, html) in all_templates() {
            assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
            assert!(html.contains("<meta charset=\"UTF-8\">"));
            assert!(html.ends_with("</html>"));
            assert!(html.contains("You received this email from your Kani instance."));
        }
    }

    #[test]
    fn an_action_link_is_reachable_without_rendering_the_button() {
        let (_, html) = password_reset_email("reader", URL);
        assert!(html.contains(&format!("href=\"{URL}\"")));
        assert!(
            html.contains(&format!("<a href=\"{URL}\">{URL}</a>")),
            "a client that strips the styled button still needs a copyable link: {html}"
        );
    }

    #[test]
    fn a_username_cannot_inject_markup_into_the_body() {
        let hostile = "<img src=x onerror=\"alert(1)\">Bob & 'co'";
        for (_, html) in [
            password_reset_email(hostile, URL),
            email_verification_email(hostile, URL),
            password_changed_email(hostile),
            admin_password_reset_email(hostile),
            welcome_email(hostile),
        ] {
            assert!(
                !html.contains("<img"),
                "raw markup reached the body: {html}"
            );
            assert!(!html.contains("onerror=\""), "{html}");
            assert!(html.contains("&lt;img src=x onerror=&quot;alert(1)&quot;&gt;"));
            assert!(html.contains("Bob &amp; &#39;co&#39;"));
        }
    }

    #[test]
    fn a_url_cannot_break_out_of_the_href_attribute() {
        let hostile = "https://kani.example.com/\" onmouseover=\"alert(1)";
        let (_, html) = password_reset_email("reader", hostile);
        assert!(!html.contains("\" onmouseover="), "{html}");
        assert!(html.contains("&quot; onmouseover=&quot;alert(1)"));
    }

    #[test]
    fn escaping_leaves_ordinary_text_alone() {
        assert_eq!(escape_html("Sakura-chan 1234"), "Sakura-chan 1234");
        assert_eq!(escape_html("日本語"), "日本語");
    }
}
