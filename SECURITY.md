# Security Policy

## Supported versions

Nuupa is in early development. Security fixes are applied to the latest
release and to `master` only.

| Version | Supported |
| ------- | --------- |
| latest release | ✅ |
| `master` | ✅ |
| older releases | ❌ |

## Reporting a vulnerability

**Please do not report security vulnerabilities through public GitHub
issues.**

Use [GitHub's private vulnerability reporting](https://github.com/zademy/nuupa/security/advisories/new)
instead. Reports sent there go straight to the maintainers and stay
private until a fix is released.

Please include as much of the following as you can:

- The type of issue and its impact.
- Step-by-step instructions or a proof of concept.
- Affected versions / commit.
- Any known workarounds.

## Scope

- Nuupa's own code (the Rust backend in `src-tauri/` and the frontend in
  `src/`).
- The release pipeline in `.github/workflows/`.

Vulnerabilities in third-party dependencies should be reported through
their own channels; if a vulnerable dependency affects Nuupa users,
report it here as well so we can bump or mitigate it.

## Disclosure

We aim to acknowledge reports within 72 hours and will keep you informed
of the progress towards a fix and a public advisory.
