# Users and Roles

Kani uses inherited roles made from `resource:action` permissions. Users may hold more than one
role; their effective permissions are the union of direct role permissions and inherited parent
roles.

## Built-in roles

| Role | Parent | Purpose |
|---|---|---|
| `user` | none | Normal authenticated access to the library, sources, and personal settings |
| `admin` | `user` | Server, source-installation, account-management, diagnostics, audit, job, and repository administration |

The exact seeded permissions are migration-controlled and can gain new entries as features are
added. Inspect the role editor on the running version rather than copying an old permission list.

## Manage users

Open **Accounts** to create, deactivate, reactivate, or edit users and assign roles. A deactivated
account cannot authenticate. Use session revocation as well when access must end immediately.

The first account created through setup is an administrator. Later self-registrations receive the
normal user role; registration policy does not grant administrative access.

## Custom roles

Create a role with a unique slug, optional parent, description, and direct permission set. A child
inherits all permissions from its parent, so removing a direct permission cannot subtract one that
is still inherited.

Before assigning a custom role, test the intended UI with an account that has only that role. Kani
contract-tests permission names used by the frontend, but an operator can still create a role that
is too broad or too narrow for its purpose.

## Permission families

| Family | Controls |
|---|---|
| `library:*` | View, add, remove, refresh, and manage library data |
| `chapter:*` | Download and delete chapter files |
| `source:*` | Browse, install, remove, enable, and configure sources |
| `settings:*` | View settings and edit download, scan, or advanced groups |
| `user:manage` | Users and roles |
| `server:manage` | Server-wide operations and diagnostics |
| `admin:*` | Logs, audit, administration, and background jobs |
| `repo:*` | Add, remove, trust, and refresh extension repositories |
| `opds:*` | Catalogue reading and progress updates |
| `token:*` | Create OPDS or general API tokens |
| `metrics:read` | Scrape Prometheus metrics |
| `theme:publish` | Publish shared UI themes |

Route authorization and UI visibility use the same permission strings. Possessing a token scope is
not sufficient if the token owner no longer holds that permission.

## Registration and password recovery

Registration can be disabled in server settings or seeded with `KANI_ALLOW_REGISTRATION=false`.
Password reset requires working SMTP configuration. Email verification, password policy, captcha,
lockout, TOTP, and session controls are configured under the server and security settings.

Kani does not currently provide a complete OIDC sign-in flow. Do not configure an issuer variable
and assume SSO is active merely because a capability field exists.
