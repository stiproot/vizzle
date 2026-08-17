# vizzle — working agreement

Make visualizing code as fast and as easy as possible. Two audiences for every
diagram: someone trying to **understand** unfamiliar code, and someone trying to
see **what a change did**. Comprehension is the primary one.

## The shape of the system

```
crates/vizzle-core/     Rust. Parse → graph → diff → render. Everything hot.
crates/vizzle-py/       PyO3 bindings. A thin, dumb translation layer.
packages/vizzle-cli/    Click CLI. Git orchestration, file I/O, page assembly.
  └── assets/          viz-core.{css,js} (shared) + one template per diagram type.
docs/diagram-types/    One spec per diagram type. Written before the code.
docs/distribution.md   How vizzle reaches other repos, and why.
plugins/vizzle/        Claude Code plugin: one skill teaching agents to use it.
.github/workflows/     ci, codeql, pr-diagram, and the tag-driven release.
```

Three source layers, **one shipped distribution**: `packages/vizzle-cli/` is the
maturin project root, and the bindings compile into it as the private extension
`vizzle_cli._core`. There is no importable `vizzle_core`, and no second package
whose version could drift from this one (`docs/distribution.md` §4).

Each layer has one job. The Rust core never shells out to git and never knows
about HTML; the CLI never parses source; the bindings contain no logic beyond
argument translation. If you find yourself reaching across a layer, the design
is wrong, not the layer.

Two properties exist to serve distribution, and are easy to break by accident:
a generated page is **fully self-contained** (no runtime fetches — d3 is inlined,
so a page is shareable with no infrastructure), and the tool's only external
requirement is **`git` on PATH**. See `docs/distribution.md` §7 and §9.

## Adding a diagram type

1. **Write the spec first** (`docs/diagram-types/<type>.md`): what an element
   is, how it is detected, how it renders, what its diff means, its CLI surface,
   and acceptance criteria against a real repo. Agree on the model before code.
   **Modelling decisions go in the spec, with their evidence** — what was
   measured, what was chosen, what would change it. A decision recorded only in
   a commit message is a decision the next reader will silently reverse.
2. Model + detection + render + JSON export in `vizzle-core`, as its own module.
3. Bindings, then CLI command, then a template.
4. A new diagram type means **a new template, never a copied page**. If you are
   about to copy from an existing template, that code belongs in `viz-core.js`.
5. **Teach the agent skill about it** —
   `plugins/vizzle/skills/vizzle-diagrams/SKILL.md`. It documents the CLI
   surface, so a new command it does not mention is a command no agent will
   ever reach for, and a flag it describes wrongly is the lying spec again.
   Run every command you add to it before you commit it.

## DRY, concretely

Duplication is not "similar-looking code" — it is *one fact expressed twice*,
where changing it in one place and not the other creates a bug. Those are the
ones to hunt.

Facts that must live in exactly one place:

- **Change-status strings** (`added`/`removed`/`modified`/`unchanged`):
  `export::change_str`. Every renderer keys its palette on these.
- **Change glyphs** (`✚ ✖ ✱`): `ChangeKind::glyph`.
- **The diff palette**: `palette.rs`. It generates the Mermaid `classDef`
  block *and* the CSS custom properties the HTML views read, because when
  those were maintained separately they drifted and the same diagram rendered
  in two different greens. Colors only one renderer cares about (the HTML
  context grey) stay in `viz-core.css`.
- **How a class becomes JSON**: `export::class_json`, shared by the class and
  component exports so both describe a class identically.
- **How two revisions are compared**: `diff::diff_graphs`. The component diff
  reuses it rather than implementing a second class-comparison.
- **Box/edge geometry, node transforms, drag, arrow markers, viewport, filter,
  header and legend**: `assets/viz-core.{css,js}`. Templates describe only
  their own nodes.
- **Geometry used for both layout and drawing** (e.g. group-box margins): one
  function, called by both. When the layout's idea of a box differs from the
  renderer's, you get overlaps that look like layout bugs and are not.
- **Manifest names, source suffixes**: defined once; if a second language needs
  the list, it gets it from the first, or a comment names its twin.

Prefer sharing a *function* over sharing a *constant*, and a constant over a
copied literal. Three similar blocks with different parameters is a function
with a parameter; three blocks that merely rhyme are three blocks — do not
contort them into one.

## Modularity

- A module owns a concept, not a step in a pipeline.
- Public surface is the smallest thing that works: `pub(crate)` before `pub`,
  a helper before an export.
- Options structs carry rendering choices; do not thread six booleans through
  five call sites.
- Frontend: shared behaviour is a function on `window.vizzle` taking the
  selections it operates on. It must not reach for a global the template owns.

## Clean code, as practiced here

- Names say what a thing *is*, not how it is built. `source_watch_dirs`, not
  `get_dirs2`.
- Comments explain **why**, or a constraint the code cannot show (a Mermaid
  ordering quirk, an inotify limit, a resolution trade-off). Never what the
  next line does.
- Prefer explicit over clever. A readable 10-line loop beats a dense
  one-liner — except where the dense form is the idiom of the language.
- Errors are values with context (`anyhow::Context`, typed CLI errors). A
  background thread never dies with a raw traceback; it reports and degrades.
- Best-effort resolution is a deliberate stance: **a wrong edge is worse than a
  missing one.** Ambiguity resolves to nothing, and the spec says so.
- Determinism: sort before emitting. Diagram output must be diffable.

## Verifying

Nothing is "done" on the strength of it compiling.

```sh
cargo test -p vizzle-core                 # core
uv run pytest packages/vizzle-cli/tests   # CLI + server
uv run pre-commit run --all-files        # ruff, cargo fmt, clippy -D warnings
uv sync --reinstall-package vizzle       # after Rust changes, before CLI tests
```

- **Run it against a real repo**, not just fixtures. `~/code/h` is the standing
  target: big enough to expose scale and layout problems that fixtures never do.
- **Look at HTML output.** Layout bugs (overlapping boxes, oversized arrows,
  unreadable density) are invisible to unit tests. Drive the page headless and
  screenshot it; check the console for errors while you are there.
- Validate Mermaid output actually renders (`mmdc`), rather than assuming.
- A test that cannot fail is worth less than no test: when you fix a bug, first
  confirm the new test reproduces it.

`ci.yml` runs the first three on every push, plus `codeql.yml` and
`cargo audit`. CI is a backstop, not the check — it cannot look at a page.

## Releasing

The version is stated **once**, in the root `Cargo.toml`; maturin derives the
Python one from it. So a release is: bump it, commit, tag `vX.Y.Z`, push the
tag. `release.yml` builds four wheels and an sdist, smoke-tests each on a
clean runner, and publishes to PyPI by Trusted Publishing.

Two ways to break it silently, both learned the hard way (`docs/distribution.md`
§5.1): renaming `release.yml` voids the PyPI publisher registration, which
names that file; and `manylinux: auto` builds a Linux wheel that imports with
`undefined symbol: le16toh`, so the pin to `2_28` is load-bearing.

## Repo hygiene

- Generated diagrams belong in `examples/` (committed, regenerated
  deliberately) or nowhere. Root-level outputs are gitignored — never
  `git add -A` a diagram into the tree.
- `*.so` is gitignored: maturin writes the 5MB compiled extension into
  `packages/vizzle-cli/src/vizzle_cli/` on every editable build.
- Vendored assets (`assets/d3.*`) are managed via `web/` + bun, then synced.
- Actions are pinned to commit SHAs, with the tag in a comment beside them.
  Dependabot moves the pins; do not "tidy" them back to tags.
- Keep the spec current with the code. A spec that lies is worse than no spec:
  mark shipped sections, and move deferred ideas to the out-of-scope list.
  This applies to `SKILL.md` too — it is a spec that an agent executes.
