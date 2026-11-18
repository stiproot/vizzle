# Atoms: extract, overlay, draw — planned parts over real code

**Status:** implemented 2026-11-17 on `feat/atoms-overlay` (§8 records what the spec left open).
**Established:** 2026-11-17

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

## 8. Decisions made while building (2026-11-17)

What §1–§6 left open, what was chosen, and what would change it.

**Reviewed by the operator 2026-11-18:** all sixteen confirmed as built except **13**, changed (see there).

**Shape of the code.** `crates/vizzle-core/src/import.rs` reads a component graph back from JSON;
`overlay.rs` merges an overlay into it; `lib.rs` exposes `overlay_json` and
`component_diagram_from_json`. The merge works on the typed graph and writes through the same
`component::to_json` (so `export::change_str` and `export::class_json` stay the only writers), not on
raw JSON. The CLI's `_draw` is one function behind both `vizzle draw` and `component --overlay`.

1. **An imported graph carries its class relations.** The JSON has no member type references or
   raw bases, so relations cannot be resolved again. `component::json_with_relations` takes them
   as facts. Round-trip `export → import → export` is byte-exact (tested on a plain graph, a lensed
   graph and a diff). The wire structs refuse unknown fields: a field added to the export without
   being taught to the import fails that test instead of being dropped silently.
2. **Wire strings are read through the inverse of their writer.** These are `export::parse_change`,
   `Language::from_name`, `Visibility::from_sigil` and `RelationKind::from_name`, each defined by
   searching the writer's output, so no string is spelled twice.
3. **Component graphs only, for overlay and mermaid drawing.** A class graph (`class -f json`) draws
   as HTML, because the page is a pure function of the JSON. It does not overlay or draw as mermaid:
   `draw` says so and points at `vizzle class`. Change this when a design needs planned classes with
   no component around them.
4. **Targets are exact.** A class is named by its `qualified` name and a component by its `path`.
   There is no bare-name or glob matching, because a renamed class must go stale rather than quietly
   match something else.
5. **What counts as stale (exit 1):** a `modify` or `remove` class, a `modify` member, or a relation
   end that the code lacks. Also an `add.classes` component or `add.edges` end that is in neither the
   code nor `add.components`, and any `add` the code already has (component path, class, edge
   `from→to`, or relation `from→to`). Every finding is listed, not only the first.
6. **What counts as "could not run" (exit 2):** an unreadable file, input that is not JSON, a class
   graph given as GRAPH, an unknown field anywhere in the overlay (so a misspelt `ad` is not
   silently empty), an unknown relation kind, `lang` or visibility, and (decision 13) a `change`
   written on a planned class or member.
7. **A stale overlay draws nothing.** `overlay` without `--check`, and `component --overlay`, exit 1
   with the same findings and write nothing. A planned part naming what the code lacks would draw a
   picture that lies. `--check` differs only in that, when clean, it writes nothing.
8. **`modify.members`:** a string names an existing member, which is marked `modified`. An object
   plans a new member, marked `added`; if one of that name exists, the entry is stale. A
   `modify`/`remove` on a class marks its component `modified` and adds the file to `changedFiles`
   (relative to the component, as the extractor writes it).
9. **Relation kinds:** the export's four names (`inherits`, `implements`, `association`,
   `dependency`), plus `realization` → `implements` and `generalization` → `inherits`. A design
   document is where an author writes the UML word. Output always uses the export's names.
10. **A planned edge does not mark its importer modified.** The overlay does not say which file
    will import, so it does not guess. If the importer should read amber, add a `modify` for it,
    which is what real code of that shape would show. The core test
    `an_overlay_draws_the_mermaid_the_real_change_would` proves the mermaid matches a real diff
    byte for byte when the overlay says so.
11. **Counts:** a planned component's `files` is its file list plus any planned class files outside
    it. A real component's `files` is left as extracted, because the JSON does not list a
    component's files, so the extractor cannot tell whether a planned file is new. The file does
    still appear in `changedFiles`. `classes` and `langs` grow with each planned class. `stats` is
    recomputed by the export.
12. **`remove`** strikes the class and every member, as a diff draws a deleted class. Removing a
    component or an edge is out of scope until a design needs it.
13. **`add.classes` share the extract's class wire shape** (`import::WireClass`), so the defaults
    (`name` and `module` from `qualified`, `lang` from the file suffix, visibility `+`) are worked
    out in one place. ~~A `change` given on a planned class is overridden to `added`.~~ **Changed 2026-11-18:** a `change`
    field on a planned class is refused (exit 2), as decision 6 refuses any other input it would have to ignore.
    Silently overriding input is the same failure as silently dropping an unknown field. Implemented
    2026-11-18 for planned classes and planned members alike (`WireClass::into_placed`, `WireMember::into_member`).
    Two edges, accepted: the check runs when an entry is applied, so an entry that is also stale (its class
    or component missing, or colliding with a real one) reports as stale, exit 1, before its `change` is
    seen; and `"change": null` reads as absent.
14. **`-f json` on `class`, `component` and `diff`** (both types), and an `-o *.json` suffix picks
    it. `component --overlay` implies `--classes`, because class targets must be in the graph to be
    checked.
15. **The lens survives the round trip.** `stats.highlight` is carried, and `draw -f mermaid` applies
    it unless given its own.
16. **Same page, given the same title.** `component --overlay` and the piped atoms are byte-identical
    for HTML and mermaid when given the same `--title` (tested). Without a title, `draw` names the
    graph file (`<stem> — component diagram`, or `graph` for stdin), because it cannot know the
    repository's name. Input JSON is stripped of surrounding whitespace, so a pipe's trailing
    newline does not ride into the page.
