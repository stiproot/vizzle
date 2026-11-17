# Atoms: extract, overlay, draw — planned parts over real code

**Status:** proposed 2026-11-17 (agree before code).

## 1. Why

A design document needs a picture of code that does not exist yet: the parts a change will add, beside the real
parts it touches. Today every author re-solves this by hand (hand-written Mermaid, a separate SVG generator, a
one-off script), and the hand-drawn parts rot. Measured on 2026-11-17: vizzle's interactive page is a **pure function
of its graph JSON** (`html.build_component_html` reads only `components[].change`, `classes[].change`,
`members[].change`, `edges`, `classRelations`, `stats.diff`), so "real graph + planned parts" is a merge before
rendering. A 40-line prototype rendered a real proposal as a normal component page, with the planned classes inside
the modified component's drill-down (`web/inspect-component.mjs` confirmed: 4 changed classes, the unchanged ones
folded).

The same split serves CI: a planned part that names a real symbol must keep naming one, or the document is stale.

## 2. The atoms

Three small commands, JSON in and out, each usable alone by an agent or a CI step:

| Atom | Command | In | Out |
|---|---|---|---|
| **extract** | `vizzle component|class|diff … -f json` | code | the graph JSON the renderers already use |
| **overlay** | `vizzle overlay GRAPH.json OVERLAY.json [-o OUT] [--check]` | a graph + planned parts | a merged graph, diff marks on |
| **draw** | `vizzle draw GRAPH.json [-f html|mermaid] [-o OUT]` | a graph | the page or the Mermaid text |

`vizzle component … --overlay OVERLAY.json` is sugar for extract → overlay → draw in one process.

## 3. The overlay format

```json
{
  "add": {
    "components": [{ "path": "src/lib/controllers", "name": "controllers", "group": "src/lib", "files": ["src/lib/controllers/contract.ts"] }],
    "classes": [{ "qualified": "…contract.Controller", "component": "src/lib/controllers", "file": "…/contract.ts",
                  "annotation": "interface", "members": [{ "name": "start", "detail": "job", "returns": "Result", "isMethod": true }] }],
    "edges": [{ "from": "src/lib/activities", "to": "src/lib/controllers" }],
    "classRelations": [{ "from": "…SandboxSteps", "to": "…Controller", "kind": "realization" }]
  },
  "modify": [{ "class": "…recipe-runner-activities", "members": ["makeRecipeRunnerActivities"] }],
  "remove": [{ "class": "…OldThing" }]
}
```

- `add.*` entries get `change: "added"`; omitted fields take the extract defaults (`visibility: "+"`, `lang` from the
  file suffix, counts recomputed).
- `modify` marks a REAL class (and the named members) `modified`; its component becomes `modified` and lists the file.
- `remove` marks a real class `removed` (drawn struck, as a diff draws it).
- `stats` (`changes`, counts, `diff: true`) is recomputed after the merge, never hand-written.

## 4. `--check` (the CI atom)

Exit `0` the overlay applies cleanly; `1` stale: a `modify`/`remove` target or a relation end that names no real
symbol, or an `add` that collides with a real one (it was built, so the document should now say `modify` or drop
it); `2` the command could not run. Same split as `vizzle doc --check`.

## 5. Where it lives

Merge and validation in `vizzle-core` (one place owns the change strings, `export::change_str`, and the class JSON
shape, `export::class_json`); a JSON → graph import so `draw -f mermaid` reuses the existing renderers; the CLI only
reads files and calls the core. The page templates do not change.

## 6. Acceptance

- `component -f json | overlay - proposal.json | draw -f html` on a real repo opens with the planned component green,
  the touched one amber, and the planned classes inside the amber one's drill-down (`inspect-component.mjs`).
- `draw -f mermaid` of the same merged graph equals what `component -f mermaid` would print for real code of that
  shape (diff classDefs from `palette.rs`).
- `overlay --check` exits 1 when a `modify` target is renamed in the code, 0 after the overlay is updated.
- The skill (`plugins/vizzle/skills/vizzle-diagrams/SKILL.md`) gains a "designing a change" section, every command
  in it run before commit.

## 7. Next (separate specs)

The hybrid architecture style (layout by a person, held to the code by vizzle passes and cited paths) moves here from
its first home, as `vizzle hybrid`; the code-comprehension diagram skills fold into this plugin.
