# `vizzle hybrid` — architecture pictures laid out by a person, held to the code by vizzle

**Status:** implemented 2026-11-18 on `feat/hybrid` (acceptance §5 met; see §10 for what the build decided). The
diagram-type spec is `docs/diagram-types/hybrid.md`; this plan is kept for its decisions and h's follow-up (§6).
**Established:** 2026-11-18
**Follows:** `docs/plans/impl/atoms-and-overlay.md` §7.

## 1. Why

vizzle draws what the code IS: modules, classes, imports, diffs. It does not draw what RUNS — processes,
containers, stores, the runtime edges between them (spawn, POST, publish) — because none of that is in the source
tree. A design document or an onboarding page needs both on one picture.

h solved this in October 2026 with a third style beside "vizzle output as-is" (true to the code, but no runtime and
no control over layout) and "hand-written SVG" (looks right, nothing holds it to the code; an independent check
found defects in one the same day). The **hybrid** style: a person writes the runtime layer and a coarse layout as
JSON; vizzle supplies the module layer fresh on every build; the build refuses any disagreement and lints its own
picture. It has drawn h's nine canonical architecture pictures since 2026-10-24 and has caught real drift (a new
package that no picture placed failed the build until the spec placed or omitted it).

It lives in the wrong place. The generator is ~1,000 lines of plain Node in h (`scripts/hybrid-diagrams.mjs` +
`scripts/hybrid/{vizzle,reconcile,layout,route,lint,svg,metrics,paths}.mjs`, guide
`docs/diagrams/hybrid/README.md`). Any other repository can only reach it through h's checkout
(`HYBRID_REPO=<root>`). It also reads vizzle by parsing its **mermaid text with regexes**
(`scripts/hybrid/vizzle.mjs:parseMermaid`), which breaks the day the mermaid label format moves.

The rule (operator, 2026-11-17): vizzle is the source of truth for diagramming tooling; h consumes it and does not
grow its own. This spec moves the hybrid style into vizzle as one command, `vizzle hybrid`, and h keeps only its
specs.

## 2. What a hybrid picture is (unchanged by the move)

Two kinds, both generated from one hand-written JSON spec into one SVG:

- **runtime** — processes, containers and stores, each opened into the code modules it runs. The module layer comes
  from one or more vizzle **passes** (a `vizzle component` invocation over a scope). A module the spec names that the
  pass no longer reports, or a module the pass reports that the spec places nowhere (and does not `omit` with a
  reason), fails the build.
- **concept** — what a primitive or topology IS. No pass; every box that stands for a real thing cites a source
  `path`, and a missing path fails the build.

Shapes: `component` (title, stack line, a grid of module boxes, a "links band" of the shared modules it imports),
`store`, `box`, `band`, `boundary`, `tile`. Orthogonal edges, `runtime` (solid, labelled) or `weak` (dashed). A
title, subtitle, legend and footnote on every picture. Placement is coarse (`x: [x0, x1]`, `y` as a number,
`{below, gap}` or `{top, dy}`); heights come from content. Text widths are estimated from a fixed metric table, so
output is deterministic. h's README §"The spec" is the format reference and moves into vizzle verbatim (§5).

## 3. The command

```
vizzle hybrid [NAME ...] [--dir DIR] [--root ROOT] [--check] [--png]
```

| | |
|---|---|
| `NAME` | diagrams to build (spec stems); none = every `*.json` in `DIR` except the pass table |
| `--dir` | where specs live; default `docs/diagrams/hybrid`. Outputs go to its parent (`<name>.svg`), PNGs to `<parent>/rendered/` |
| `--root` | the repository the passes run over and cited paths resolve against; default `.` (replaces `HYBRID_REPO`) |
| `--check` | build in memory, compare with the committed SVG, write nothing |
| `--png` | also write a 2x PNG of each SVG (§9 D4 decides how) |

Exit codes, the same split as `doc --check` and `overlay --check`:
- `0` built (or, with `--check`, every SVG matches a fresh build);
- `1` the spec disagrees with the code (drift), the picture fails its layout lint, or a committed SVG is stale.
  **Nothing is written** for that diagram or any after it, and every finding is listed with the diagram and box;
- `2` could not run: unreadable or non-JSON spec, an unknown field, an unknown pass, `--png` with no renderer.

The pass table (`DIR/vizzle-passes.json`) keeps its shape, minus the version pin: `{ "passes": { "<name>": {
"args": [...] } } }`. A pass is run **in-process** as `component` with those arguments and read as the graph JSON
from the atoms (`component::to_json`), not mermaid. The old `"vizzle": "<version>"` key is accepted and ignored
with a note for one minor release (the version is now the vizzle running the command).

## 4. Where it lives

Per `CLAUDE.md`'s layers:

| layer | gets |
|---|---|
| `vizzle-core` | `hybrid/` module: `spec.rs` (typed spec, unknown fields refused), `reconcile.rs` (module layer vs spec, cited paths through a `exists` callback so the core never touches the filesystem directly), `layout.rs`, `route.rs`, `lint.rs`, `metrics.rs`, `svg.rs`. Input: spec JSON + `{pass → graph}`; output: SVG text or a findings list |
| `vizzle-core/palette.rs` | the five hybrid families (fill, stroke, text) beside the diff palette, so one file owns every colour vizzle draws |
| `vizzle-py` | one function, `hybrid_svg(spec_json, graphs, exists)`, argument translation only |
| `vizzle-cli` | `hybrid` command: find specs, run passes through the existing component path, call the core, write or compare, `--png` |
| `docs/diagram-types/hybrid.md` | the diagram-type spec (CLAUDE.md "Adding a diagram type" step 1): h's README sections "The two kinds", "The visual language", "How drift is caught", "What no check covers", "The spec", "How this style was chosen" |
| skill | `vizzle-diagrams/SKILL.md` gains "Architecture pictures: hybrid" — when to choose it over `component`, the add-a-diagram loop, `--check` in CI; every command in it run before commit |

Port, not rewrite: each JS module maps to one Rust module with the same functions, so a reviewer can read them side
by side. h's `scripts/hybrid/picture.test.mjs` cases (routing, lint, svg of single nodes) port as core unit tests.

## 5. Acceptance

1. **Byte-identical on h's nine pictures.** With h's specs and pass table, `vizzle hybrid --root <h checkout> --dir
   <h>/docs/diagrams/hybrid` writes SVGs identical to h's committed ones except the one `GENERATED by` comment line
   (which now names vizzle and its version). Proved by a script in the PR that diffs all nine.
2. **Drift fails, naming the box.** In a throwaway copy of h: rename a module a runtime spec names → exit 1 naming
   the component and the ref; add a package inside a pass scope → exit 1 naming it as unplaced; delete a cited path in
   a concept spec → exit 1 naming the node. Each restored → 0. One test each in the core, plus the live run.
3. **Lint fails, naming the rule.** A spec with two overlapping boxes, and one with text wider than its box → exit 1
   with the rule and both ids.
4. **`--check`** exits 0 on h's committed set, 1 after one SVG is hand-edited, 2 on a misspelt field (`"nodez"`).
5. **A second repository.** A small runtime spec over vizzle's own tree (CLI process → core extension; the pass is
   `crates/` + `packages/`) is committed as `examples/hybrid/` with its SVG, and `vizzle hybrid --check --dir
   examples/hybrid` runs in vizzle's CI. LOOK at its PNG.
6. **No new runtime requirement for SVG.** Building SVGs needs nothing but vizzle (and `git`, as today). Only `--png`
   may need more (§9 D4).
7. Gates as `CLAUDE.md`: `cargo fmt`, `cargo test`, `clippy -D warnings`, pytest, ruff, `vizzle doc --check`,
   `bump-version.sh check`, pre-commit.

## 6. What h does afterwards (a separate h PR, after the vizzle release)

- `package.json` lint: `node scripts/hybrid-diagrams.mjs --check` → `uvx vizzle@<pin> hybrid --check`; `Makefile`
  `diagrams` target likewise with `--png`. The pin is the same one `check-diagrams` already holds to
  `VIZZLE_VERSION`, so the pass table's own pin goes.
- Delete `scripts/hybrid-diagrams.mjs`, `scripts/hybrid/`, and the README's "How it works" section; the README
  becomes h's notes on its own pictures plus a link to vizzle's `docs/diagram-types/hybrid.md`.
- Regenerate the nine SVGs once (only the comment line changes) and commit.
- `.claude/skills/diagrams/SKILL.md` points at the vizzle command.

## 7. Out of scope

- An interactive HTML hybrid page (SVG is what decks, READMEs and Notion take).
- Automatic layout. The point of the style is that a person chooses it.
- Overlay inside hybrid (planned modules drawn in a runtime picture). Natural next step: a pass may name an overlay
  file, and planned modules draw green. Its own spec once a design needs it (§9 D7).
- Folding the code-comprehension plugin's skills in (HANDOVER item 5; separate).

## 8. Build plan

Fired through h like #104: one host run in a fresh worktree of `stiproot/vizzle`, task = this spec + the acceptance
as orders, claude opus, no push; the driver reviews, pushes `feat/hybrid`, opens the PR, and the h-reviewer app's
approval merges it. Then a release (minor), then the h PR of §6.

## 9. Decisions (settled 2026-11-18: every recommendation accepted)

| | Question | Recommendation | Why |
|---|---|---|---|
| D1 | Port the generator to Rust, or ship the JS and run it with Node? | **Rust, in `vizzle-core`** | `CLAUDE.md`: the only external requirement is `git`, and one distribution. Shipping JS adds Node to every consumer and a second language to the repo for one command. The JS is small, pure and deterministic; it ports module for module |
| D2 | Read passes as graph JSON in-process, or keep running `uvx vizzle@pin component` + parsing mermaid? | **In-process graph JSON** | The atoms made graph JSON the contract (#104). The mermaid regex is the most fragile part of today's generator |
| D3 | Keep the spec format exactly, or clean it up while moving? | **Exactly, plus unknown fields refused** | Acceptance 1 (byte-identical) is the proof the port is right; any format change spoils it. Clean-ups go in a later minor, with h already on vizzle |
| D4 | How does `--png` render? | **Headless Chrome, found as h finds it (`$CHROME`, standard installs, `PATH`), in the CLI** | Same pixels as today's PNGs. A pure-Rust rasteriser (resvg) would drop the Chrome need but render fonts differently from the SVG people see; revisit if CI needs PNGs |
| D5 | Family names: keep h's (`sn`, `dist`, `shared`, `ext`, `entry`) or make them generic? | **Generic names, h's accepted as aliases**: `primary`, `secondary`, `shared`, `external`, `entry` | `sn`/`dist` mean single-node/distributed — h words in a general tool. Names never reach the SVG, so aliases cost nothing and keep acceptance 1 |
| D6 | Is a layout-lint failure exit 1 (stale) or 2 (could not run)? | **1** | It is the author's spec to fix, like drift; 2 is reserved for "the tool could not do its job" |
| D7 | Overlay in hybrid now or later? | **Later, own spec** | Keeps this a pure move with a byte-identical proof. It is the reason to do the move, though: the first design doc that wants planned modules in a runtime picture triggers it |
| D8 | Default `--dir` = `docs/diagrams/hybrid`? | **Yes** | h's layout; any repo can pass `--dir`. Avoids a config file |
| D9 | Where does the GENERATED comment point? | **`vizzle hybrid` + version + spec path** | Tells a reader what to run; the version makes a stale pin visible in a diff |

## 10. Decided while building (the spec left these open)

| | Question | Decision | Why |
|---|---|---|---|
| B1 | How is a module's id derived from graph JSON? | `<group>/<leaf of name>` (`hybrid::passes::module_id`) | Exactly what the mermaid parse produced, so every existing ref resolves (proved: the sn pass gives identical modules and edges both ways). Consequence: renaming a manifest renames the module; renaming the directory under one does not |
| B2 | Which `component` flags may a pass carry? | Parsed by `component`'s own parser; graph-shaping flags only; drawing flags (`--group`, `--weights`, `-o`, `-f`, `--overlay`, …) are exit 2 | A drawing flag would be silently ignored. `--group` included: ids are `<group>/<leaf>` regardless |
| B3 | What is `--dir` relative to? | A given `--dir` is a shell path; the default is `ROOT/docs/diagrams/hybrid` | Explicit paths behave as every other CLI path; the default follows `--root` so `--root ../h` works alone |
| B4 | How does the GENERATED comment name the spec? | Root-relative when the spec is under `--root`, else as given | Stable across machines, so `--check` is not path-dependent |
| B5 | `links` values | `"auto"`, `"none"` or a list; any other word exit 2 | h's specs use `"none"` (the old code treated any non-`auto` word as no band) |
| B6 | Validation beyond unknown fields | Exit 2 for: a duplicate node id, an id named by `in`/`contains`/`below`/an edge that names no node, a non-boundary node with no `x`/`y`, a tile with no `height`, a bad coordinate expression, an out-of-range `labelSeg`, the default legend with no spec `family` | The JS crashed or drew `NaN` on each; none occurs in h's specs |
| B7 | `$comment` | Allowed on the spec, nodes, modules and edges; ignored | Where authors record the facts a picture rests on |
| B8 | Legend kinds, label sides, edge kinds | Closed enums (`swatch`/`edge`/`weak`; `right`/`left`/`above`/`below`; `runtime`/`weak`) | The JS drew any unknown value as the last branch; refusing it is the "unknown fields refused" rule applied to values |
| B9 | The default legend's wording | Kept as h's ("single-node runtime component…", "infrastructure h does not build") | Byte-identical (§5.1). A spec outside h should give its own `rows`; a generic default is a later-minor clean-up |
| B10 | The version in the example's GENERATED line vs releases | `scripts/bump-version.sh` moves `examples/*.svg`'s `vizzle hybrid X.Y.Z` on `bump` and fails `check` on skew | Otherwise every bump commit fails CI's `hybrid --check` |
| B11 | Nine pictures? | h's `feat/h-lab` has **eight** specs (plus the pass table); all eight are byte-identical | The ninth in §1 is not on that branch |
| B12 | Number printing | `hybrid::metrics::num` prints as JavaScript does (no `.0`, `-0` → `0`), `r` rounds halves up like `Math.round` | Byte-identical output; differs from JS only below 1e-6 / from 1e21, which no coordinate reaches |
