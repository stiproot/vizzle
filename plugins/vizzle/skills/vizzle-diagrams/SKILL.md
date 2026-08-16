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
| `vizzle class` — every class, unscoped | ~16k tokens |
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

An unscoped `class` diagram on a large repo is ~16k tokens. **Always scope it
with `-I`** unless the repo is small or you have budgeted for it. Cheaper still:

- `--no-members` — classes and relations without fields and methods.
- `-l python` / `-l typescript` — one language only.

### 3. After making changes: what did they do?

```sh
uvx vizzle diff <path> --type component          # working tree vs HEAD
uvx vizzle diff <path> --base main --type component
```

`--type component` is the interesting one: it shows whether your change
*rewired* the application — an added or removed dependency edge between modules
is a much bigger deal than a changed method, and it renders loudest.

Drop `--type component` for a class-level diff of what you touched.

## Reading the output

**Component diagrams** are a Mermaid `flowchart`. `«component»` boxes are
modules; `subgraph` blocks group siblings by parent directory; dashed arrows are
dependencies pointing from importer to imported.

**Class diagrams** are a Mermaid `classDiagram` with stereotypes
(`<<interface>>`, `<<dataclass>>`), typed members, and inheritance,
association, and dependency edges.

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
