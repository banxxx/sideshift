# Auto Classification

"Does the server need this mod?" is the core question of every conversion. SideShift collects
evidence from several sources and uses the most reliable one as the verdict — **it never guesses
without evidence**.

## Where the evidence comes from

From most to least reliable:

1. **Self-declared by the mod**: Fabric / Quilt manifest files state which side they run on
2. **Platform build info**: the official statement for this exact build on Modrinth and CurseForge (queried through a mirror, no key needed)
3. **Project-level declaration**: what the mod author says about the whole project
4. **The modpack's own statement**: environment info written by the packager (discarded entirely when it is clearly a lazy fill)
5. **File-name patterns**: when nothing above answers, common name patterns are used as a last resort (shaders, minimaps…)

## Three outcomes

| Outcome | Meaning |
| --- | --- |
| Keep | Evidence says the server needs it |
| Remove | Evidence says it is client-only |
| Needs review | No source could answer — parked in the Remove group and highlighted, **your call** |

"Needs review" is deliberate: it is better to ask you once than to silently delete or keep a mod.

## Two safety nets

- **Conflicts are flagged**: when two sources disagree, the row is marked "conflicting verdict" so you can look at it yourself
- **Dependencies are never removed by mistake**: a library required by a kept mod stays, even when marked client-only, and is flagged

## Your overrides always win

Automatic verdicts are drafts. Every row can be overridden, and the build follows exactly what you confirmed.
Turning off "remove client-only resources" switches to manual mode: the app only shows evidence and changes nothing.
