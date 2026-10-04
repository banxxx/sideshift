# Local Development

## Requirements

- Node.js 22 and pnpm
- Rust stable (MSVC toolchain on Windows)
- Windows (the app currently packages for Windows only)

## Common commands

| Command | Purpose |
| --- | --- |
| `pnpm install` | Install dependencies |
| `pnpm dev` | Frontend only (browser preview with mocked backend data) |
| `pnpm tauri dev` | The full desktop app (Rust backend + frontend) |
| `pnpm build` | Frontend type check and build |
| `pnpm i18n:check` | UI translation coverage check |
| `pnpm docs:dev` | Preview this documentation site locally |
| `pnpm docs:build` | Build the documentation site |

Rust tests run with `cargo test` inside `src-tauri`.

## Project layout

| Folder | Contents |
| --- | --- |
| `src/` | Frontend (React + Vite): UI, state, the IPC wrapper around the backend |
| `src-tauri/` | Backend (Rust): modpack parsing, classification, downloads, the build pipeline, app updates |
| `installer/` | The installer shell (first-install bootstrapper) |
| `docs/` | Documentation site (VitePress — this site) |

## Commits and checks

Pushes to the main branch run CI automatically (Rust tests + frontend build + translation coverage).
Running those three locally before pushing saves a round of waiting.

## Related pages

- [Release Guide](/en/maintain/release): what to do around cutting a release
