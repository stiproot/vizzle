---
name: vizzle-diagrams
description: Orient in an unfamiliar codebase, or check what a change did to its shape, by generating a diagram with vizzle instead of reading files. Use when you need to understand a repository or subsystem you have not seen, when asked what a codebase contains or how its parts depend on each other, when asked for an architecture or dependency overview, when asked to produce a diagram of code, and after making changes to see how they reshaped the module graph.
---

# Reading a codebase with vizzle

`vizzle` parses a repository and prints its shape as a Mermaid diagram. For an
agent it is an orientation tool: it answers *"what is this made of, and what
depends on what"* for a fraction of what reading the files costs.

## Why to reach for it

Measured on a real 358-file repository:

| What you do | Cost |
| --- | --- |
| `vizzle component` — the whole repo's module graph | **~1.5k tokens** |
| `vizzle class` — every named type, unscoped | ~30k tokens |
| Reading the repo to learn the same thing | tens of thousands |

A component diagram is cheap enough to run *before* you have a specific
question. Reach for it at the start of work in an unfamiliar repo, the way you
would `ls` — not as a last resort.

**Prefer Mermaid over any JSON export.** Mermaid is 3–6× cheaper for the same
graph and needs no parsing step. This is the opposite of the usual instinct.

## First: can vizzle see this repo?

**vizzle parses Python and TypeScript only.** This is the one thing that will
waste your time if you skip it.

Components are found from manifests (`package.json`, `pyproject.toml`,
`Cargo.toml`, `go.mod`), but **a component containing no `.py`/`.ts` files is
dropped**. So a Rust or Go repository produces an empty or near-empty diagram
even though its manifests were found — that is by design, not a failure.

Check before running:

```sh
git ls-files | grep -cE '\.(py|ts|tsx)$'
```

If that count is zero or tiny, read files instead. Do not report an empty
diagram as a finding about the architecture.

## The ladder — go in this order

### 1. Always start here: the component diagram

```sh
uvx vizzle component <path>
```

One box per build-level module, one dashed arrow per dependency derived from
imports. Prints to **stdout** — no output file needed, no cleanup.

Useful additions:

- `--weights` — label each edge with how many files import across it, so you
  can see which dependencies are load-bearing and which are incidental.
- `--externals` — add a node per external package (npm/PyPI).
- `-E 'tests/*'` — exclude paths *before* detection, removing those components.

### 2. Then, only if you need class-level detail: scope it

```sh
uvx vizzle class <path> -I 'src/the/part/you/care/about/**'
```

An unscoped `class` diagram on a large repo is ~30k tokens. **Always scope it
with `-I`** unless the repo is small or you have budgeted for it. Cheaper still:

- `--no-members` — classes and relations without fields and methods.
- `-l python` / `-l typescript` — one language only.
- `--group-by component` — one `namespace` per package, so the diagram reads as
  "what is each component made of". `--group-by module` is finer (one namespace
  per file) and reads better when the scope is already a single package.

### Keeping a diagram true: `vizzle doc`

A diagram committed to a repository goes stale. `vizzle doc` regenerates one
from a manifest, and `--check` verifies without writing, which is what belongs
in a lint chain:

```sh
uvx vizzle doc --dir docs/architecture            # regenerate
uvx vizzle doc --dir docs/architecture --check    # verify; non-zero if stale
```

A managed document is markdown carrying a manifest comment and one mermaid
fence. **Only the fence is rewritten** — prose around it is the author's. The
manifest either lists symbols by hand, or names a path:

```
<!-- gen:c4-code {"scope":{"path":"src/pkg","lang":"python","group":"module"}} -->
```

Prefer the path form for a gate: a hand-listed manifest cannot catch a *new*
class, because a class absent from the manifest is absent from the diagram and
the check reports current. A path catches it.

Exit codes: `0` current, `1` stale or too large for mermaid to render, `2` the
command could not run. A caller that wants to fail open on a broken environment
while still blocking on real drift keys off that split.

The class diagram covers classes, interfaces, enums, dataclasses, and the
TypeScript `type` aliases that carry structure (an object literal, or a union
of named types). It does **not** include module-level functions unless you ask:
`--modules` adds one `«module»` box per module listing its public functions,
which on a large repo is another ~90 KB, so ask for it only when the functions
are the question.

### 3. After making changes: what did they do?

```sh
uvx vizzle diff <path> --type component                  # working tree vs HEAD, scoped to <path>
uvx vizzle diff <path> --base main --type component
uvx vizzle diff packages/my-module --type component      # focus on one module
uvx vizzle diff <path> --type component -E '**/tests/fixtures/**'   # drop fixture "components"
uvx vizzle diff <path> --type component --classes        # include class detail in HTML
uvx vizzle diff <path> --type component -o d.mmd --stats verdict.json   # + machine-readable verdict
```

`-I`/`-E`/`-l` mean exactly what they mean on `component`: a component whose
files are all filtered out is never drawn — on either revision, so nothing
reads as added or removed because of the filter. An excluded component is
dropped even if it would have been a «boundary» neighbour (and the edge into it
goes with it), and a change confined to excluded paths renders as no structural
change. The active selection is stated in the HTML legend and in a
`%% vizzle: selection:` trailer on the Mermaid.

`--type component` is the interesting one: it shows whether your change
*rewired* the application — an added or removed dependency edge between modules
is a much bigger deal than a changed method, and it renders loudest. The diagram
is scoped to the path you give it, keeping only components in that subtree plus
any out-of-scope neighbours that share edges (marked distinctly as «boundary»
nodes to show structural coupling at the scope boundary).

For HTML output, `--no-classes` is the default for **component diffs** (`vizzle
diff --type component`), because class bodies are heavy and omitted to keep the
artifact lean. `vizzle component` (non-diff) defaults to `--classes`. Use
`--classes` in a diff to embed drill-down detail, or `--no-classes` in the
non-diff command to produce a leaner page.

Drop `--type component` for a class-level diff of what you touched.

**If a script acts on the result, give it `--stats FILE` and read that.** It is
JSON: `changed` (bool), `changes` (`added`/`removed`/`modified` counts),
`chars` (rendered size) and, for mermaid, `mermaidLimit` and `oversized`. Never
decide "did anything change" or "will this fit in a comment" by searching the
diagram text for `vizzleAdded` or a `✚`: those are rendering choices and can
change in any release, and a script keyed on them fails silently as "no
change".

## Reading the output

**Component diagrams** are a Mermaid `flowchart`. `«component»` boxes are
modules; `subgraph` blocks group siblings by parent directory; dashed arrows are
dependencies pointing from importer to imported.

**Class diagrams** are a Mermaid `classDiagram` with stereotypes
(`<<interface>>`, `<<dataclass>>`, `<<type>>`, `<<union>>`, `<<module>>`),
typed members, and inheritance, association, and dependency edges. A
`<<union>>` box lists its arms as members and draws a dependency edge to each
one it could resolve.

**Diff output** marks every element: `✚` added, `✖` removed, `✱` modified.
Unchanged elements in touched files appear as context so the change keeps its
surroundings.

A caveat worth carrying into any conclusion you draw: vizzle resolves edges
best-effort and **prefers a missing edge to a wrong one**. Ambiguous references
resolve to nothing. Treat the graph as a reliable floor, not an exhaustive
inventory — an absent edge is weak evidence, a present one is strong.

## Do not

- **Do not use `--format html` or `-o page.html` for yourself.** The HTML view
  is ~500 KB and its entire value — zoom, drag, expand a component into its
  classes, filter — needs a human with a browser. It is inert to you.
- **Do not write a file when you only want to read.** Omit `-o` and read stdout.
  If you do write one, put it somewhere disposable, not in the user's repo.
- **Do not run an unscoped `class` diagram on a large repo** without deciding
  the tokens are worth it. Start with `component`, then scope.

## When the human wants the diagram, not you

That is exactly when `--format html` earns its size: the page is fully
self-contained — d3 is inlined, no network needed — so it opens from `file://`
and can be attached or shared as-is.

```sh
uvx vizzle component <path> -o architecture.html
uvx vizzle serve <path> --diff --open     # live, re-renders as they edit
```

For a diagram to paste into a PR comment, issue, or Markdown file, use the
Mermaid output instead — GitHub renders ` ```mermaid ` blocks natively, so it
needs no image hosting.

## Requirements

`uvx` (from [uv](https://docs.astral.sh/uv/)) and `git` on PATH. Nothing is
installed into the repository being examined. If `uvx` is unavailable, install
once with `uv tool install vizzle` or `pipx install vizzle`.
