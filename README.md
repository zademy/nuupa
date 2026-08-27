# Nuupa

[![build](https://github.com/zademy/nuupa/actions/workflows/build.yml/badge.svg)](https://github.com/zademy/nuupa/actions/workflows/build.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)

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

| Platform | Installers |
| --- | --- |
| macOS (universal) | `.dmg` |
| Linux x64 / arm64 | `.deb`, `.rpm`, `.AppImage` |
| Windows x64 / arm64 | `.msi`, `.exe` (NSIS) |

Every release ships a `SHASUMS256.txt` with the checksums of all artifacts.

### Unsigned builds

> **IMPORTANT (UNSIGNED APPS)**: Nuupa releases are currently **not signed
> or notarized** (Apple Developer ID and Windows code signing are still
> pending), so your OS may block the app from running.

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

### Project layout

```
src/               Vue 3 frontend (App, panels, store + tests)
src-tauri/src/     Rust backend (one module per manager: npm, pnpm, bun)
.github/           CI: multi-platform builds + automated releases
```

## Building and releasing

`cargo tauri build` produces local bundles. Releases are automated by CI:

1. Push a tag `v*` matching the version in `src-tauri/tauri.conf.json`.
2. A sanity job validates the tag against the project versions.
3. Five platform builds run in parallel and upload their installers.
4. A publish job validates that every installer family is present,
   generates release notes with all commits since the last release,
   adds checksums, and publishes.

Tags containing `rc`, `beta` or `alpha` are published as pre-releases.

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
