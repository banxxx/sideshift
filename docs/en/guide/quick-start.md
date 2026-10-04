# Quick Start

From picking a modpack to holding a server package — four steps.

## Step 1: Pick a modpack

Select the file to convert on the home page. Three formats are supported:

- `.mrpack` — Modrinth modpacks
- CurseForge official export `.zip`
- Plain `.zip` archives that contain a `mods` folder

## Step 2: Review the automatic classification

The app sorts every mod into one of three groups:

| Group | Meaning |
| --- | --- |
| Keep | Evidence says the server needs it |
| Remove | Evidence says it is client-only (minimaps, shaders…) |
| Needs review | Cannot be decided — parked in the Remove group and highlighted, waiting for you |

Every row shows its reasoning. You can override any row — **your choice always wins over the automatic verdict**.

## Step 3: Add missing mods (optional)

Use "Add mods from network" to search Modrinth and CurseForge and add missing mods.
A mod's detail page lists its dependencies; click one to jump straight to it and add it too.

## Step 4: Build

Confirm the output folder and build options, then start. When it finishes you get a server archive:

1. Unpack it into your server folder
2. Accept the EULA (follow the instructions inside the package)
3. Run the start script and play

## If something goes wrong

- Every row on the convert page shows its reasoning and hints; rows needing your attention are highlighted
- When a build fails, the task details explain the cause and what to do next
