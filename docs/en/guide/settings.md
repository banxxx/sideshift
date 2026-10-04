# Settings

## Paths

| Setting | Description |
| --- | --- |
| Output folder | Where the server archive is saved |
| Cache folder | Temporary files for downloads and builds; safe to clean anytime |

## Conversion

| Setting | Description |
| --- | --- |
| Remove client-only resources | Automatically drops pure client mods from the server package (on by default; off = manual mode, evidence only) |
| Verify after build | Offline check of the produced package (mods, dependencies, configs) |
| Install loader locally | Produces a directly startable server for Forge / NeoForge packages |
| Reuse loader installs | Skips re-downloading the loader for repeated conversions of the same version (on by default) |

## Network

| Setting | Description |
| --- | --- |
| Download source | Version manifests and loaders: official / BMCLAPI (CN mirror) |
| Concurrent downloads | Parallel download threads (1–16) |
| Modrinth mirror | Modrinth queries prefer the mcimirror proxy with official fallback (on by default) |
| Look up side info online | Off = classification relies only on in-pack evidence and the local cache |
| Side lookup source | Modrinth official / Minekuai mirror (faster in CN, off by default) |
| Wiki fallback | When every platform source misses, ask the MC Wiki entry (off by default) |

CurseForge data always comes through a mirror — **no API key required**.

## App

| Setting | Description |
| --- | --- |
| Update channel | Follow current build / Stable / Beta (see [App Updates](/en/guide/update)) |
| Interface language | 简体中文 / 繁體中文 / English, applied instantly |

::: tip
Not sure what to pick? Keep the defaults — they are tuned for the "CN network + unzip and play" scenario.
:::
