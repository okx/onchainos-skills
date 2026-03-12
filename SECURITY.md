# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 1.x     | Yes       |
| < 1.0   | No        |

## Reporting a Vulnerability

If you discover a security vulnerability in onchainos-skills, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email: **security@okx.com**

Include:
- A description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

We will acknowledge receipt within 48 hours and aim to provide a fix or mitigation plan within 7 business days.

## Scope

This policy covers:
- The `onchainos` CLI binary and its dependencies
- The skill definitions (SKILL.md files)
- The install scripts (`install.sh`, `install.ps1`)
- The CI/CD workflows

## Security Practices

- All release binaries include SHA256 checksums for verification
- Dependencies are audited with `cargo audit` in CI
- The CLI uses rustls-tls (no OpenSSL dependency)
- API credentials are never embedded in the binary
