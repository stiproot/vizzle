---
name: design-diagrams
description: Draw diagrams of code that does not exist yet or that the code cannot show, and publish diagrams for people. Use when designing a change and you need planned parts drawn over the real code (vizzle overlay), when a person wants an architecture picture of what runs that they lay out themselves (vizzle hybrid), when drawing a sequence, state, context, deployment or UML component diagram by hand, when choosing colours for any diagram, and when a diagram must become a PNG, SVG or HTML page for a document, a chat message, a slide or a pull request.
---

# Designing and publishing diagrams

`vizzle-diagrams` reads code that exists. This skill covers the rest: parts that
are only planned, pictures a person lays out, the diagrams you draw by hand, and
turning any of them into something a person opens. One rule runs through all of
it: **whatever the code can supply, vizzle supplies**, so the picture stays true
when the code moves. Draw by hand only what the code cannot know.

## Designing a change: planned parts over real code

A design needs a picture of code that does not exist yet, drawn beside the real
code it touches. Don't hand-draw the whole thing, because the real parts rot.
Write only the planned parts as an **overlay**, and let vizzle merge them into
the real graph. Planned parts draw as a diff would draw them: added parts green
`✚`, touched parts amber `✱`, removed parts struck.

```sh
uvx vizzle component <path> -f json -o graph.json             # extract: the real graph
jq -r '.classes[].qualified' graph.json                       # the names an overlay may target
uvx vizzle overlay graph.json proposal.json -o merged.json    # merge the planned parts
uvx vizzle draw merged.json -o design.html                    # the page (for the human)
uvx vizzle draw merged.json -f mermaid                        # for a doc or PR
uvx vizzle component <path> --overlay proposal.json -f mermaid   # all three in one go, same output
uvx vizzle overlay graph.json proposal.json --check           # CI: 0 current, 1 stale, 2 cannot run
```

```json
{
  "add": {
    "components": [{ "path": "cache", "files": ["cache/src/cache/store.py"] }],
    "classes": [{ "qualified": "cache.src.cache.store.Store", "component": "cache",
                  "file": "cache/src/cache/store.py", "annotation": "interface",
                  "members": [{ "name": "fetch", "detail": "key: str", "returns": "str", "isMethod": true }] }],
    "edges": [{ "from": "api", "to": "cache" }],
    "classRelations": [{ "from": "api.src.api.handlers.Handler", "to": "cache.src.cache.store.Store", "kind": "dependency" }]
  },
  "modify": [{ "class": "api.src.api.handlers.Handler",
               "members": ["get", { "name": "invalidate", "detail": "key: str", "isMethod": true }] }],
  "remove": []
}
```

- Components are named by path and classes by qualified name, exactly as they
  appear in `graph.json`. Anything you leave out of an `add` entry is derived:
  `name` and `module` from the qualified name, `lang` from the file suffix,
  visibility `+`, and the counts.
- In `modify`, a member given as a **string** is an existing member, which is
  marked `✱`. A member given as an **object** is a new one, marked `✚`. The
  class and its component turn amber.
- Never write `change` on a planned class or member: the overlay decides it, and
  a `change` field there exits 2, naming the class or member.
- Relation kinds are `inherits`, `implements`, `association` and `dependency`.
  `realization` and `generalization` are accepted as the UML words for the
  first two.
- **A stale overlay draws nothing.** `overlay` exits 1 and names every entry
  that is stale: a `modify`, `remove` or relation end the code no longer has,
  or an `add` the code already has. If something was renamed, update the
  overlay. If something was built, change its `add` to a `modify` or drop it.
- `--overlay` implies `--classes`, because the overlay's class targets have to
  be in the graph to be checked.
- `draw` also accepts a class graph (`vizzle class -f json`), as HTML only.

## Architecture pictures: hybrid

`component` shows what the code IS. When the human needs what RUNS (processes,
containers, stores, the spawn/POST/publish edges between them) opened up to the
modules each one runs, and wants a layout they chose (a deck slide, an
onboarding page), use **`vizzle hybrid`**. A person writes the runtime layer
and a coarse layout as JSON; vizzle supplies the modules fresh on every build
and refuses to write a picture that disagrees with the code or fails its own
layout lint. Do not hand-write SVG for this: nothing holds it to the code.

```sh
uvx vizzle hybrid --dir examples/hybrid --png     # build every spec, + 2x PNGs (needs Chrome)
uvx vizzle hybrid vizzle-runtime --dir examples/hybrid   # one picture
uvx vizzle hybrid --check --dir examples/hybrid   # CI: 0 current, 1 drift/lint/stale, 2 cannot run
```

- Specs live in `--dir` (default `docs/diagrams/hybrid` under `--root`); each
  `<name>.json` writes `<name>.svg` to the directory's **parent**, PNGs to
  `<parent>/rendered/`. `vizzle-passes.json` there names the passes:
  `{"passes": {"self": {"args": ["-I", "crates/**"]}}}` — `component`'s own
  selection flags, run in-process over `--root`.
- A module box carries `refs` like `self:crates/vizzle-core`: the pass name,
  then the module id `<group>/<leaf of its name>`. To see the ids a pass
  reports, run the same flags as `uvx vizzle component . <args> -f json` and
  read `components[].group` / `.name`.
- **The add-a-picture loop:** copy the nearest spec, run with `--png`, fix
  what each `✗ [<name>] drift: …` / `layout: …` line names, then LOOK at the
  PNG — passing the lint does not make it readable. Commit the spec and the
  SVG; never commit the PNG.
- A new package in a pass scope fails the build until the spec places it in a
  component or lists it under `omit` with a reason. A concept picture
  (`"vizzle": []`) is held by the `path` each box cites.
- Families: `primary`, `secondary`, `shared`, `external`, `entry`. The full
  format is `docs/diagram-types/hybrid.md` in the vizzle repository; an unknown
  field is refused (exit 2), so a misspelling cannot pass silently.

## Drawing by hand: what the code cannot show

The code knows its modules and types, so those come from vizzle. It does not
know who uses the system, where it runs, or what happens in what order. Those
you draw.

**System context and deployment.** A mermaid `flowchart`: the system as one
box, people and external systems around it (context), or the hosts, containers
and stores it runs on (deployment). Keep each label to a phrase and put the
detail in the prose. Ground each box in the file that proves it (the client
that calls the external API, the compose file that defines the container). If
the picture will be kept and shown again, make it a `hybrid` concept picture
instead, where each box cites a path and the build fails when a path goes away.

**Behaviour.** A `sequenceDiagram` for "what happens, in what order"; a
`stateDiagram-v2` for "what states, which transitions". Name participants after
the components in the structural diagram so the two views line up.

**What a component provides and requires** (UML component view, distinct from
vizzle's component diagram, which shows dependencies): mermaid has no native
type, so use a `classDiagram` with `<<component>>` and `<<interface>>`
stereotypes. Provides is a realization (`..|>`), requires is a dependency
(`..>`).

### Mermaid syntax traps (each one broke a real diagram)

- **classDiagram member lines keep parentheses balanced per line.** An unbalanced
  `(` crashes mermaid's member parser.
- **A `:::` class goes on the declaration** (`class Store:::diffAdded {`). A
  separate `class Store:::diffAdded` line after a declaration with a body is
  silently ignored.
- **No commas in `loop`, `alt` or `opt` labels, and no semicolons anywhere in
  message text.** A `;` ends the statement and the rest of the line parses as a
  new one.
- **Never `deactivate` the same activation inside both branches of an `alt`.**
  Mermaid counts statically and errors; deactivate once, after the `end`.
- **Mermaid C4 (`C4Context`, `C4Container`) collapses to one box per row when a
  description is long.** Keep descriptions under about 40 characters, put
  `UpdateLayoutConfig` right after the `title`, and look at the render: compiling
  is not enough. A `flowchart` avoids the problem.
- **Render before you commit.** `uvx vizzle render <doc.md> out/` is the syntax
  check; a diagram that does not render is not done.

## Colours: one palette, `vizzle palette`

vizzle's own diagrams draw in one palette, so a diagram you draw beside them
takes its colours from the same place. Never invent a green: two greens on one
page read as two meanings.

```sh
uvx vizzle palette            # the rect tints + the diff classDefs; -f json for scripts
```

```text
sequenceDiagram
    participant A as poller
    participant B as lease store
    rect rgba(207, 34, 46, 0.15)
        Note over A,B: BEFORE (main @ abc1234) — where it breaks
        A->>B: renew the lease
        B--xA: rejected
    end
    rect rgba(26, 127, 55, 0.15)
        Note over A,B: AFTER (PR @ def5678) — what the fix does
        A->>B: ✓ reclaim the timed-out ownership first
    end
```

The tints are translucent on purpose. A sequence diagram's message text sits
directly on the `rect` fill and `rect` cannot set a text colour, so an opaque
pale fill such as `rect rgb(255, 235, 233)` turns white dark-mode text
unreadable. Never use an opaque `rgb(...)` there. The class-diagram
`classDef`s set their own text colour, which is why they can be opaque.

What each colour means, in every diagram you draw:

| Meaning | classDiagram | sequenceDiagram |
| --- | --- | --- |
| New, or the fixed path | `:::diffAdded` | the `after` tint |
| Changed in shape or behaviour | `:::diffModified` | a `[CHANGED]` prefix on the message |
| Removed, failing, or the alternative the design rejects | `:::diffRemoved` | the `before` tint |
| The current state, neither old nor new | default (no class) | the `now` tint |

Untouched elements stay default, and a Note says the untinted machinery did
not change. A design document should show the path it refuses as well as the
one it picks, in the removed colour. State the legend in a sentence next to the
diagram.

## Publishing: when a person needs to see it

Pick the output by where it will be read:

| Where | Output |
| --- | --- |
| A pull request, an issue, a Markdown file in a repo | the mermaid itself, in a ` ```mermaid ` fence: GitHub renders it, no image hosting |
| A document, a chat message, a slide | a PNG (or SVG) from `vizzle render` |
| A person who will explore it (zoom, drag, expand a component into its classes) | a self-contained HTML page (`-o page.html`) |

The HTML page is fully self-contained (d3 is inlined, no network needed), so it
opens from `file://` and can be attached or shared as it is. It is useless to
you as an agent and the right thing for a person.

```sh
uvx vizzle component <path> -o architecture.html
uvx vizzle serve <path> --diff --open     # live, re-renders as they edit
uvx vizzle render architecture.mmd out/   # PNG (or -f svg) for chat or a slide
```

`render` drives mermaid-cli (`mmdc` from PATH, else a pinned version through
bunx or npx) and needs a browser on the machine; it uses one already in
puppeteer's cache. It raises Mermaid's 50,000-character cap so a whole-repo
diagram draws instead of failing silently.

**GitHub draws a mermaid fence in the reader's theme**, so before you post one
that sets its own colours, look at it both ways:

```sh
uvx vizzle render diagram.mmd out/ --theme both   # out/diagram.png + out/diagram.dark.png
```

## Requirements

`uvx` (from [uv](https://docs.astral.sh/uv/)) and `git` on PATH; `vizzle render`
and `vizzle hybrid --png` also need a browser (Chrome, or the one in
puppeteer's cache). Nothing is installed into the repository being drawn.
