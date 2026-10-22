# Authentication

## Browser sessions

Create a session with credentials:

```http
POST /rest/auth/login
Content-Type: application/json

{
  "username": "admin",
  "password": "correct horse battery staple"
}
```

A successful response sets an HTTP-only session cookie. The server cycles the session identifier
on login and records login and failure events in the audit log. Failed attempts can return `429`
with `Retry-After` and structured lockout information.

Logout invalidates the current session:

```http
POST /rest/auth/logout
```

Users can list and revoke sessions or log out everywhere through the account API and UI.

## CSRF for cookie clients

Read-only requests establish a `kani_csrf` cookie. Cookie-authenticated `POST`, `PUT`, `PATCH`, and
`DELETE` requests must copy its value into the `X-CSRF-Token` header.

```http
X-CSRF-Token: <value from kani_csrf cookie>
```

General bearer-token requests are exempt because a cross-site browser cannot attach a token it
does not possess. Do not disable CSRF or expose the readable CSRF cookie as if it were an API
credential.

## General API tokens

Create a token under **Settings → Clients**. Choose the API kind, a descriptive name, optional
expiry, and the minimum required permission scopes. The raw value begins with `kani_` and is shown
only once.

```http
Authorization: Bearer kani_<token>
```

The token's effective scopes are intersected with the owner's current role permissions on every
authentication. If a role is removed, formerly valid scopes become stale and requests return
forbidden even before the token is revoked.

Token management is available at `/rest/me/api-tokens`; use the current OpenAPI schema for request
fields and responses. Token creation itself requires `token:create_api`.

## OPDS tokens

OPDS tokens are a separate token kind created with `token:create_opds`. Their fixed catalogue and
progress scopes are intended for reader applications and are rejected by the general REST API.
This separation prevents a reader credential from becoming an automation credential.

## TOTP and step-up authentication

The auth surface supports TOTP setup, verification, disablement, backup-code regeneration, and
step-up verification for sensitive actions. Backup codes are single-use and should be treated like
passwords.

API automation should use a scoped token rather than attempting to automate a TOTP-protected
interactive session.

## Registration and recovery

First-run setup, self-registration, password strength, captcha, password reset, and email
verification are public auth workflows with their own validation and rate controls. Public routing
prevents a chicken-and-egg dependency on an existing session; it does not make account creation or
reset unrestricted.

Kani does not currently provide a complete OIDC sign-in flow. There is no supported callback route
to document.
