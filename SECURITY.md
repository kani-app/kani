# Security Policy

## Reporting a Vulnerability

Do **not** report security vulnerabilities in public GitHub issues.

Report a vulnerability through:

- **GitHub private security advisory:** [Report a vulnerability](../../security/advisories/new)

### What to include

- A clear description of the vulnerability and its potential impact
- Steps to reproduce (version, configuration, request/response if applicable)
- Any proof-of-concept code, if available

### Response timeline

| Milestone | Target |
|-----------|--------|
| Acknowledgement | 72 hours |
| Initial assessment | 14 days |
| Fix / coordinated disclosure | 90 days from report |

Do not publish vulnerability details until a fix is released or 90 days have passed since the
report, whichever occurs first.

## Scope

In scope: the Kani server binary, REST API, auth layer, WASM runtime host, and Docker image.

Out of scope: third-party extensions installed by users, the content of external websites accessed
through extensions, and user-created configuration.
