# Contributing to Nuupa

Thanks for your interest in contributing! This document covers the basics.

## Issues

- Bugs and feature requests go through
  [GitHub Issues](https://github.com/zademy/nuupa/issues).
- Search existing issues before opening a new one.
- Include your OS, the package managers you have installed, and the
  Nuupa version when reporting bugs.

## Development setup

Prerequisites:

- [Rust](https://www.rust-lang.org/) (stable) and the
  [Tauri 2 platform dependencies](https://tauri.app/start/prerequisites/)
- Node.js 24

```sh
git clone https://github.com/zademy/nuupa.git
cd nuupa
npm install
cargo tauri dev
```

## Workflow

1. Fork the repository and create a branch from `master`.
2. Make your change. Keep it small and focused — one change per PR.
3. Add or update tests for changed behavior (store tests live next to the
   store in `src/` and run with Vitest).
4. Make sure everything passes:

   ```sh
   npm test
   cargo check --manifest-path src-tauri/Cargo.toml
   ```

5. Open a pull request against `master` describing **what** changed and
   **why**.

## Code organization

| Path | Contents |
| --- | --- |
| `src/` | Vue 3 frontend: app shell, manager panels, store and its tests |
| `src-tauri/src/` | Rust backend: one module per package manager (`npm`, `pnpm`, `bun`), plus exclusions |
| `.github/workflows/` | CI: multi-platform builds and automated releases |

Domain vocabulary is defined in `CONTEXT.md` — use those terms in issues,
PRs, tests and code so discussions stay unambiguous.

## Commits

Write commit messages in the imperative mood, with a short subject line:

```
Add bun global list pagination
Fix outdated detection for scoped packages
```

## Releases

Releases are automated: a tag `v*` matching the version in
`src-tauri/tauri.conf.json` triggers builds for all supported platforms
and a published GitHub release with checksums. Don't bump versions in PRs
unless the change is meant to be released.

## License

By contributing, you agree that your contributions will be licensed under
the [AGPL-3.0](LICENSE).
