# vizzy

UML visualization for git: parse a codebase (or a git diff) and render class
diagrams — as Mermaid text or as an interactive d3-powered HTML page — with
changes highlighted so a reader immediately sees what a change did to the
shape of the code.

- **Rust core** (`crates/vizzy-core`): tree-sitter parsing (Python +
  TypeScript), a language-neutral class graph, a graph diff engine, and a
  Mermaid `classDiagram` renderer. Parses ~200 classes in well under a second.
- **PyO3 bindings** (`crates/vizzy-py`): exposed to Python as `vizzy_core`,
  built with maturin.
- **Click CLI** (`packages/vizzy-cli`): the `vizzy` command. Python owns the
  git orchestration; Rust owns everything hot.

## Setup

Requires [uv](https://docs.astral.sh/uv/) and a Rust toolchain.

```sh
uv sync   # builds the Rust extension via maturin and installs the CLI
```

## Usage

Full class diagram of a codebase:

```sh
uv run vizzy class ~/code/repo/h -o h-classes.mmd
```

What changed, as a diagram (added = green ✚, removed = red ✖, modified =
yellow ✱; unchanged classes in touched files render as context):

```sh
# working tree vs HEAD
uv run vizzy diff ~/code/repo/h

# between two revisions
uv run vizzy diff ~/code/repo/h --base HEAD~20 --head HEAD -o changes.mmd
```

### Interactive HTML (d3)

Write to a `.html` file (or pass `--format html`) and vizzy emits a fully
self-contained page — d3 v7 is inlined, no network needed — that renders the
class graph as SVG with **zoom** (scroll), **pan** (drag the background),
draggable class boxes, a fit-to-view button, and a live filter box. Diff
pages color whole classes *and* individual member rows (removed members are
struck through):

```sh
uv run vizzy class ~/code/repo/h -o h-classes.html
uv run vizzy diff ~/code/repo/h --base HEAD~20 --head HEAD -o changes.html
open changes.html
```

Layout is a d3 force simulation (link + charge + rectangle collision) with a
weak per-module gravity, ticked synchronously before first paint so the page
opens settled.

Useful flags (both commands): `--no-members`, `--group` (mermaid namespace
blocks per module), `--externals` (edges to types outside the parsed set),
`--direction LR` (mermaid), `-I/-E` include/exclude globs (`vizzy class`),
`--title`, `-f/--format mermaid|html`.

Render the `.mmd` output with mermaid-cli:

```sh
npx -y @mermaid-js/mermaid-cli -i changes.mmd -o changes.svg
```

## How the diff works

`vizzy diff` asks git for the changed files (`git diff --name-status -M -z`),
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

cargo test -p vizzy-core                 # core unit tests
uv run pytest packages/vizzy-cli/tests   # CLI end-to-end tests
uv sync --reinstall-package vizzy-core   # rebuild after Rust changes
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
