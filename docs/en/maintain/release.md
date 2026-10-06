# Release Guide

The release-side half of the in-app update chain: sign the build → upload to a GitHub Release.
The client half (detection, download, signature verification, install, reconciliation) lives in
`src-tauri/src/core/update/`. The contract between the two halves is the asset pair on a release:
`*-setup.exe` plus a same-name `.sig`.

## One-time setup

### 1. Key pair

Update packages are signed with a minisign key pair. **The public key is already committed** in two
places that must stay identical (a test pins this):

- `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`
- `src-tauri/src/core/update/mod.rs` → `UPDATER_PUBKEY`

If you hold the private key matching that public key, go to step 2. Otherwise generate a new pair
and follow "Rotating keys" below to update both places in the same change:

```bash
pnpm tauri signer generate -w .keys/sideshift.key
```

The private key file **never enters the repository** (keep `.keys/` gitignored). Losing it means
every released version can no longer be signed — the only way out is a key rotation.

### 2. GitHub Secrets

Repository Settings → Secrets and variables → Actions, two repository secrets:

| Name | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | The **contents** of the private key file (pasting a path does not work) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password chosen at generation; leave an empty string if none |

### 3. Verify the whole chain

No real release needed: run the **Release** workflow manually (workflow_dispatch). It builds and
signs but does not create a release; the artifacts land in the run's artifacts. Install the
`*-setup.exe`, run "Check for updates", and confirm the verdict is installable.

## Every release

1. **Bump the version**: the single source is the root `package.json` (`tauri.conf.json` reads it
   dynamically); sync `src-tauri/Cargo.toml` and `installer/Cargo.toml` by hand so `CARGO_PKG_VERSION`
   does not lie.
2. **The pre-release suffix is the channel**: `1.0.0-beta.2` follows the Beta line, `1.2.0` follows
   Stable (judged from the tag's semver, not the GitHub checkbox).
3. **Write the changelog — two files, two audiences**:

   **Short notes = `release-notes/<version>.md`** (no `v` in the filename: `release-notes/1.0.0-beta.3.md`). Its contents become the GitHub Release body, i.e. the few lines the in-app "check for updates" dialog shows. **One file per release, never overwritten** — the workflow fails when the file is missing, so shipping last release's notes is no longer possible. `node scripts/check-release-notes.mjs` enforces the shape (run it before tagging):

   | Rule | Why |
   |---|---|
   | 4 lines at most, 3 recommended | the dialog's note is `line-clamp-4` — a 5th line is never seen; the spare line absorbs font/DPI differences |
   | One bullet per line, split instead of running long | a hard line break is a visual line; a line holds ~34 CJK glyphs (418px of content at 12px), and a wrapped line eats the next one's budget |
   | Start every line with `- ` | it renders as a list on the release page and reads as a bullet in the dialog; use no other markdown |
   | No blank lines | the dialog renders `whitespace-pre-wrap`, so a blank line shows as 18px of nothing |
   | No headings, `<!-- comments -->`, `---` front matter | the dialog does not render markdown — those characters appear verbatim to users |
   | Don't repeat the version, date or size | the title already says "Update to v…", and the sub-line already carries size · date · current version |
   | Order: added → changed → fixed | the first thing a reader judges is whether their item is in it |

   **Full history = `docs/guide/changelog.md`** (where the "Changelog" link at the bottom-left of the dialog lands): **prepend** a section per release, older ones stay, nothing gets edited. That heading must be **the bare version number** (`## v1.0.0-beta.3`) with the date on the first body line — the link builds the anchor from the version (`v1.0.0-beta.3` → `#v1-0-0-beta-3`), so any extra character in the heading breaks the jump.

   **Order matters**: land both files on master and let Pages finish deploying **before** tagging. Pages builds on push→master, the Release on tag push — tag first and the link lands on a page that has no entry for the version yet.

4. **Tag and push** (the tag starts with `v` and must match package.json exactly — the workflow
   fails otherwise):

   ```bash
   git tag v1.0.0-beta.3
   git push origin v1.0.0-beta.3
   ```

5. The workflow checks that `release-notes/<version>.md` exists and has a valid shape, builds and
   signs, produces the shell and portable packages, creates the Release (body from that file,
   prerelease flag decided automatically) and uploads four artifacts.

## The four release assets

| File | Who uses it |
| --- | --- |
| `SideShift_<version>_x64-setup.exe` | The **in-app update package** (signature covers it) |
| `SideShift_<version>_x64-setup.exe.sig` | The paired signature; missing it degrades the whole flow (`NoSignature`) |
| `SideShift-Setup.exe` | The installer shell for **first-time installs** from the release page |
| `SideShift-<version>-portable-x64.zip` | The portable build (in-app updates degrade to the release page) |

Note that `SideShift-Setup.exe` (the shell) is **not** part of in-app updates — the updater
installs the signed pair.

## Rotating keys

A key rotation must change both the config pubkey and `UPDATER_PUBKEY` in the same change, and must
ship one release built solely for the rotation: packages signed by the new key fail verification on
old clients, which fall back to the release page for one manual install. After that build, the chain recovers.

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| Workflow fails at the tag check | Tag does not match package.json (the package.json is the single source) |
| Workflow fails at "notes filled" check | `release-notes/<version>.md` is missing (one file per release, no `v` in the name), or its shape is invalid: blank line, markdown, or more than 4 visual lines — run `node scripts/check-release-notes.mjs` locally to see which line |
| The dialog's note is cut off mid-sentence | the file hits the 4-line clamp and its last bullet wraps; shorten that bullet and put the detail in `docs/guide/changelog.md` |
| Build error "A public key has been found, but no private key" | Secrets missing or wrong: `TAURI_SIGNING_PRIVATE_KEY` must be the key file contents |
| Users stuck on "missing signature file" | The `.sig` was not uploaded (check the artifact collection step) |
| Users stuck on "signature mismatch" | The public key changed without a client rotation — follow "Rotating keys" |
| "Check for updates" keeps failing | The API JSON has no public mirror (proxy services reject `api.github.com`); it goes direct with a 10-second timeout, twice. **Downloads** do have a mirror chain (ghfast.top / ghproxy.net, official last) — when one rots, remove it from `ASSET_MIRRORS` in `core/update/mod.rs` |
