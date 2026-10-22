# Email and SMTP

SMTP enables test mail, password reset, and email-verification workflows. Kani does not need SMTP
for local-password login when those features are disabled.

## Configure transport

Open **Settings → Email** with advanced-settings permission and enter the host, port, credentials,
sender identity, and transport-security mode shown by the running version. Common ports are 587
for STARTTLS and 465 for implicit TLS, but use the provider's documented values.

The SMTP password is encrypted at rest when the credential-encryption key is available. Back up
that key; changing it without migrating credentials makes the stored password unreadable.

## Test delivery

Save the configuration, then send a test message to an address you control. A successful SMTP
submission does not prove final inbox delivery. Check the provider's logs and spam policy when a
test is accepted but never arrives.

Typical failures include:

- Using implicit TLS on a STARTTLS port or the reverse.
- A sender address the provider does not permit.
- Missing DNS or outbound network access from the container.
- Credentials that require an application password.
- Restoring `kani.db` without the matching credential key.

## Account workflows

Password-reset requests intentionally avoid revealing whether an address has an account. Email
verification and registration behavior are controlled by security/server settings. Test those
flows with a non-administrator account before requiring them for users.

Kani's current email service is not a general new-chapter notification system; use webhooks for
event delivery unless the running UI explicitly offers an email notification feature.
