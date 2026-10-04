# Introduction

SideShift is a desktop app that converts Minecraft modpacks into **ready-to-run server packages**:
it decides whether each mod is needed on the server, fills in the required components,
and packages everything into an archive you can upload and start.

## What it does

- Reads three common modpack formats:
    - `.mrpack` (Modrinth modpacks)
    - CurseForge official export `.zip`
    - Plain `.zip` archives that contain a `mods` folder
- Decides whether each mod belongs on the server, and shows the reasoning
- Searches and adds mods from Modrinth and CurseForge — no API key required
- Fills in server-side basics (such as Fabric API) and can install loaders locally (Forge / NeoForge / Fabric)
- Generates start scripts and common server configs (EULA, port, JVM flags)
- Checks for app updates in place, with one-click upgrades

## When to use it

You downloaded a modpack to play with friends, but a server needs a different mod list than the client —
minimaps and shaders are useless server-side, while missing dependency libraries break the startup.
Picking mods by hand is slow and error-prone. SideShift does that filtering and assembly for you.

## What it does not do

- It never modifies the contents of any mod
- It is not a game launcher and does not run the server for you (how you start the unpacked server is up to you)

## Keep reading

- [Quick Start](/en/guide/quick-start): four steps to a server package
- [Auto Classification](/en/guide/classification): how the decision is made
