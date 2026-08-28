<h1 align="center">Nuupa</h1>

<p align="center">
  <a href="https://github.com/zademy/nuupa/releases"><img src="https://img.shields.io/github/v/release/zademy/nuupa" alt="release"></a>
  <a href="https://github.com/zademy/nuupa/actions/workflows/build.yml"><img src="https://github.com/zademy/nuupa/actions/workflows/build.yml/badge.svg" alt="build"></a>
  <a href="https://github.com/zademy/nuupa/releases"><img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue" alt="platforms"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-AGPL--3.0-blue.svg" alt="License: AGPL-3.0"></a>
</p>

A desktop app to see and update the global packages of your installed
package managers (**npm**, **pnpm**, **bun**) — no commands required.

## Features

- **All your managers in one place** — Nuupa detects which package managers
  are installed and shows each one's global space, independently.
- **Outdated detection** — every global package is compared against the
  latest published version, so you always know what's behind.
- **Update one or update all** — update a single package, or run the
  update-all queue: outdated packages are updated one at a time,
  sequentially, with live progress.
- **Persistent exclusions** — mark a package as excluded in its manager and
  update-all will skip it forever (per manager + package). Manual updates
  remain available for excluded packages.
- **nvm-aware** — the list reflects the global packages of the Node version
  currently active in nvm.
- **Cross-platform** — macOS (universal Intel + Apple Silicon), Linux
  (x64 + arm64) and Windows (x64 + arm64).

## Download

Grab an installer from the [releases page](https://github.com/zademy/nuupa/releases):

| Platform            | Installers                  |
| ------------------- | --------------------------- |
| macOS (universal)   | `.dmg`                      |
| Linux x64 / arm64   | `.deb`, `.rpm`, `.AppImage` |
| Windows x64 / arm64 | `.msi`, `.exe` (NSIS)       |

Every release ships a `SHASUMS256.txt` with the checksums of all artifacts.

### Unsigned builds

> [!WARNING]
> Nuupa releases are currently **not signed or notarized** (Apple Developer
> ID and Windows code signing are still pending), so your OS may block the
> app from running.

You can still run it:

- **macOS**: after moving the app to `/Applications`, open a terminal and
  run:

  ```sh
  xattr -dr com.apple.quarantine /Applications/Nuupa.app
  ```

- **Windows**: when SmartScreen warns that the app is not signed, choose
  **More info → Run anyway**.

## Supported package managers

npm, pnpm and bun — detected automatically. Only the ones installed on
your machine are shown.

## Development

Prerequisites:

- [Rust](https://www.rust-lang.org/) (stable) and the
  [Tauri 2 platform dependencies](https://tauri.app/start/prerequisites/)
- Node.js 24

```sh
npm install
cargo tauri dev   # full app (Rust + frontend, hot reload)
npm run dev       # frontend only (Vite)
```

### Testing

```sh
npm test          # store tests (Vitest)
```

### Formatting and linting

```sh
npm run format    # frontend (Prettier)
npm run lint      # frontend (ESLint)
cargo fmt         # Rust (rustfmt)
cargo clippy      # Rust lints
```

Prettier owns formatting; ESLint owns code quality (its stylistic rules are
disabled via `eslint-config-prettier`, so they never conflict). CI checks
all of the above on every push.

### Project layout

```
src/               Vue 3 frontend (App, panels, store + tests)
src-tauri/src/     Rust backend (one module per manager: npm, pnpm, bun)
.github/           CI: multi-platform builds + automated releases
```

## Building and releasing

`cargo tauri build` produces local bundles. CI splits the work between the
two long-lived branches — nobody pushes tags by hand:

**`develop` — build and validate:**

1. A sanity job checks that `src-tauri/tauri.conf.json` and
   `src-tauri/Cargo.toml` carry the same version.
2. The Rust and frontend test suites run.
3. Five platform builds run in parallel and upload their installers as
   artifacts.

**`master` — build and publish:**

1. Sanity derives the release version Spring Boot-style: merges never
   carry version bumps. CI takes the latest `v*` tag and increments the
   patch (`v0.3.2` → `v0.3.3`); a **higher** version in the files wins
   (set `0.4.0` in `develop` when you want a minor/major). It stamps
   `tauri.conf.json`, `Cargo.toml` and `Cargo.lock`, and commits the
   stamp with `[skip ci]` so the tag contains its own version.
2. The five platform builds run again and the publish job validates that
   every installer family is present and adds checksums.
3. It creates the tag at the stamped commit and publishes the release
   with notes of all commits since the last release.

Versions containing `rc`, `beta` or `alpha` are published as pre-releases.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Please follow the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Security

Found a vulnerability? Please report it privately — see
[SECURITY.md](SECURITY.md).

## License

Copyright (C) 2026 Nuupa contributors.

This program is free software: you can redistribute it and/or modify it
under the terms of the [GNU Affero General Public License v3.0](LICENSE)
as published by the Free Software Foundation.
