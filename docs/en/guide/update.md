# App Updates

## Checking for updates

Two entry points:

- **Manual**: Settings → Appearance & About → "Check for updates"
- **Automatic**: once every 24 hours after startup; when an installable new version is found, a green dot appears on the version number at the bottom-left

## Channels

The default is "follow current build": if your version has a pre-release suffix (such as 1.0.0-beta.2)
you automatically follow the Beta line; a plain version number follows Stable. You can also pin
Stable or Beta in Settings.

## One-click update

Clicking "Update now" in the dialog runs: download the package → verify its signature → prompt to restart.
The restart is handled silently by the official installer, and **your settings and task data are preserved**.

- Downloads try several sources (mirrors first, official as fallback) and switch automatically on failure
- Packages are signature-checked; a failed check is never installed
- Installation waits while a conversion task is running

## Skip this version

The dialog lets you skip the current version: no more reminders until a **newer** version appears.
Checking manually still shows it.

## Portable edition

The portable build does not support in-app updates yet; the dialog points to the release page for a manual replacement.

## FAQ

- **No green dot?** It only appears when an installable new version is found. No network, missing assets on the release, or a skipped version — no dot.
- **Update failed?** Your installed version is untouched. The next launch reports what happened last time; retry as suggested.
- **Download slow?** Downloads switch between sources automatically; you can also grab the installer from the release page.
