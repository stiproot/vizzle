# vizzle

UML visualization for git: parse a codebase (or a git diff) and render UML
diagrams — as Mermaid text or as an interactive d3-powered HTML page — with
changes highlighted so a reader immediately sees what a change did to the
shape of the code. Two diagram types so far: **class** (the shape of the
code) and **component** (the shape of the application). Each type has a spec
in `docs/diagram-types/` — what an element is, how it is detected, what its
diff means, and the modelling decisions behind it (e.g. why aggregation and
composition diamonds are a convention rather than an inference:
`docs/diagram-types/class.md` §5.4).

- **Rust core** (`crates/vizzle-core`): tree-sitter parsing (Python +
  TypeScript), a language-neutral class graph + import graph, a component
  detector (manifest-driven), graph diff engines, and Mermaid renderers.
  Parses ~200 classes in well under a second.
- **PyO3 bindings** (`crates/vizzle-py`): built with maturin into the private
  extension module `vizzle_cli._core`.
- **Click CLI** (`packages/vizzle-cli`): the `vizzle` command. Python owns the
  git orchestration; Rust owns everything hot.

Three source layers ship as **one wheel** — `pip install vizzle` gets the CLI
and the compiled core together, so their versions cannot drift apart.

## Install

Nothing to install — run it straight from PyPI, with nothing added to the repo
you are looking at:

```sh
uvx vizzle component ~/code/some-repo
```

Or keep it around: `uv tool install vizzle` (or `pipx install vizzle`). Needs
Python ≥ 3.10 and `git` on PATH; on Linux, glibc ≥ 2.28. Why it is distributed
this way is in [`docs/distribution.md`](docs/distribution.md).

### For coding agents

vizzle ships a Claude Code plugin, so an agent knows to reach for a diagram
instead of reading the repo file by file — a component diagram of a 358-file
codebase costs about **1.5k tokens**:

```sh
claude plugin marketplace add stiproot/vizzle
claude plugin install vizzle@vizzle-marketplace
```

### From a clone

For development — requires [uv](https://docs.astral.sh/uv/) and a Rust toolchain:

```sh
uv sync   # builds the Rust extension via maturin and installs the CLI
```

## Usage

Full class diagram of a codebase:

```sh
uv run vizzle class ~/code/repo/h -o h-classes.mmd
```

Component diagram — one box per build-level module (workspace package, app,
service; any directory with a `package.json`/`pyproject.toml`/`Cargo.toml`/
`go.mod`), one dashed arrow per dependency derived from imports
(spec: `docs/diagram-types/component.md`):

```sh
uv run vizzle component ~/code/repo/h -o h-components.mmd --weights
uv run vizzle component ~/code/repo/h -o h-components.html   # interactive
```

In the HTML view each app/package gets a labelled box you can **drag as a
unit**, and every component carries a `+` toggle that **explodes it into a full
UML class diagram** — boxes with stereotypes, fields and methods, data types on
every member, and the association, dependency and inheritance edges between
them (or use *Show classes* to open them all at once). Each explosion is laid
out once at load and then only shown or hidden, so toggling stays instant.
`--no-classes` omits that detail for a leaner file.

What changed, as a diagram (added = green ✚, removed = red ✖, modified =
yellow ✱; unchanged classes in touched files render as context):

```sh
# working tree vs HEAD
uv run vizzle diff ~/code/repo/h

# between two revisions
uv run vizzle diff ~/code/repo/h --base HEAD~20 --head HEAD -o changes.mmd

# did the change rewire the application? (parses both full revisions;
# added/removed dependency edges render loudest)
uv run vizzle diff ~/code/repo/h --type component
```

### Interactive HTML (d3)

Write to a `.html` file (or pass `--format html`) and vizzle emits a fully
self-contained page — d3 v7 is inlined, no network needed — that renders the
class graph as SVG with **zoom** (scroll), **pan** (drag the background),
draggable class boxes, a fit-to-view button, and a live filter box. Diff
pages color whole classes *and* individual member rows (removed members are
struck through):

```sh
uv run vizzle class ~/code/repo/h -o h-classes.html
uv run vizzle diff ~/code/repo/h --base HEAD~20 --head HEAD -o changes.html
open changes.html
```

Layout is a d3 force simulation (link + charge + rectangle collision) with a
weak per-module gravity, ticked synchronously before first paint so the page
opens settled. The component view adds a two-level relaxation pass —
components separate within their group, then groups separate as blocks — so
group boxes never overlap, including after a component is expanded.

Both views are built from one shared core (`assets/viz-core.{css,js}`, inlined
into every page): the palette, geometry, viewport, filter, and header. A
template only describes what its own nodes look like.

**Comprehension first.** Nothing here needs a git diff: `vizzle class` and
`vizzle component` are for reading unfamiliar code. The diff is a *lens* laid
over the same diagram — under it, unchanged elements fade to neutral context
and only what changed keeps saturated color, so the change reads instantly
without losing its surroundings.

### On a pull request

`.github/workflows/pr-diagram.yml` posts a component diff as a PR comment, so
reviewers see what a change did to the shape of the application without
installing anything. GitHub renders ` ```mermaid ` blocks natively, so there is
no image hosting involved. Copy the file into any repo with Python or
TypeScript in it — the only requirement is `uvx` on the runner.

It edits one comment rather than appending, says so in a single line when
nothing structural moved, and skips fork PRs (which get a read-only token)
rather than failing them. It deliberately does not use `pull_request_target`;
the reasoning is in `docs/distribution.md` §2.3.

### Live server with hot reload

`vizzle serve` hosts the diagram and watches the source tree; every save
re-parses and pushes a reload to connected browsers (stdlib http.server +
server-sent events + watchfiles — no web framework). Your zoom/pan position
survives reloads.

```sh
uv run vizzle serve ~/code/repo/h                  # live class diagram
uv run vizzle serve ~/code/repo/h --diff --open    # watch your working-tree
                                                  # changes vs HEAD reshape
                                                  # the UML as you edit
uv run vizzle serve ~/code/repo/h --type component --diff   # live rewiring view
```

Defaults to http://127.0.0.1:8499/; see `--port`, `--host`, `--base`.

Useful flags (both commands): `--no-members`, `--group` (mermaid namespace
blocks per module), `--externals` (edges to types outside the parsed set),
`--direction LR` (mermaid), `-I/-E` include/exclude globs (`vizzle class`),
`--title`, `-f/--format mermaid|html`.

Render the `.mmd` output with mermaid-cli:

```sh
npx -y @mermaid-js/mermaid-cli -i changes.mmd -o changes.svg
```

## How the diff works

`vizzle diff` asks git for the changed files (`git diff --name-status -M -z`),
pulls the base-revision contents via `git show`, and hands both revisions to
the Rust core. The core parses each side into a class graph, diffs the graphs
(classes keyed by qualified name, members fingerprinted by signature), and
renders a single diagram where every class and member carries its change
status.

> Mermaid 11 quirk, learned the hard way: in `classDiagram`, `classDef`
> statements only take effect when they appear *after* the `cssClass`
> attachments. The renderer emits them last.

## Development

```sh
uv sync                                  # also installs dev tools (ruff, pre-commit)
uv run pre-commit install                # one-time: enable the git hook

cargo test -p vizzle-core                 # core unit tests
uv run pytest packages/vizzle-cli/tests   # CLI end-to-end tests
uv sync --reinstall-package vizzle       # rebuild after Rust changes
```

Formatting and linting run on every commit via pre-commit: `ruff format` +
`ruff check --fix` (Python), `cargo fmt` + `cargo clippy -D warnings` (Rust),
plus whitespace/YAML/TOML hygiene checks. Run everything manually with
`uv run pre-commit run --all-files`. Vendored (`assets/d3.*`) and generated
(`examples/`) files are excluded.

### JS dependencies (bun)

The d3 bundle inlined into HTML output is a real dependency managed with
[bun](https://bun.sh) in `web/`, then vendored into the Python package so
wheels stay self-contained:

```sh
cd web
bun install            # or: bun update d3
bun run sync-assets    # copies node_modules/d3/dist/d3.min.js into assets/
```

The vendored copy is committed; re-run `sync-assets` after bumping d3.

Example outputs generated from the `h` codebase live in `examples/`.
