# Scoping the component diff

Status: Proposed — not started. Raised by a consumer putting `vizzle diff --type component` in front
of reviewers on every pull request.
Established: 2026-09-14

## 1. The premise

`vizzle diff --type component` produces an artifact whose size is set by the **repository**, not by
the change. In a large multi-package repository that makes the interactive HTML unusable as a
per-pull-request artifact, and it makes the mermaid describe the whole tree rather than the part
under review.

Measured with `vizzle==0.2.0` on macOS, against a repository of ~93 parseable components spanning
Python and TypeScript, diffing a **one-commit** change and a **237-commit** merge — both scoped on
the command line to the same ~33-component subtree:

| | components | classes | mermaid | HTML raw | HTML gz | wall |
|---|---|---|---|---|---|---|
| `component <subtree>` (no diff) | 33 | 2 367 | — | 3.2 MB | 0.30 MB | 0.58s |
| `diff --type component` — 1 commit | 93 | ~20 000 | 17 685 chars | 24 MB | 1.86 MB | 6.0s |
| `diff --type component` — 237 commits | 93 | ~20 000 | — | 24 MB | 1.86 MB | 6.3s |

The two diff rows are the finding: **identical output for a one-file change and a 237-commit
merge**, and 93 components where the same subtree scoped without `--diff` yields 33.

## 2. Why the path argument does not scope it

Two causes, both in `packages/vizzle-cli/src/vizzle_cli/cli.py`:

1. `_collect_component_diff(path, base, head)` uses `path` only to find the repository root —
   `root = _repo_root(path)` — and then calls `_collect_component_revision(root, …)` for each
   revision. The path is discarded after the root lookup, so every Python and TypeScript file in the
   repository is parsed at both revisions regardless of what the caller asked for.
2. At the HTML call site, `_core.component_json_diff(..., classes=True)` is hardcoded, so every
   class in the repository is embedded whether or not a reviewer will ever expand one.

Neither is true of the non-diff path: `vizzle component <subtree>` scopes correctly, which is why
the first row of the table is 33 components and 3.2 MB.

## 3. What to change

- **Honour `path` in the component diff.** Collect each revision under the given path rather than
  the repository root. The class diff already takes a path-scoped collection
  (`_collect_diff_files`), so the asymmetry is between the two diff types, not between diff and
  non-diff.
- **Make classes opt-in on the component diff**, or at least opt-out — a flag rather than a literal
  at the call site. A reviewer asking "what did this rewire" does not need 20 000 class bodies to
  answer it.

Open question for whoever picks this up, and the reason this is a plan rather than a patch: a
component diff is a comparison of two **graphs**, and scoping changes what "the graph" means. A
component outside the path that gains or loses an edge *into* the scoped set is a real structural
change the reviewer wants to see. Scoping naively to the path would hide it. Options worth weighing:
scope the nodes but keep edges that cross the boundary; scope strictly and say so in the rendered
legend; or make the boundary a rendering concern rather than a collection one.

## 4. Acceptance

- `vizzle diff --type component <path>` in a multi-package repository produces a diagram of
  `<path>`, not of the whole repository.
- A subtree-scoped component diff lands near the **3.2 MB / 0.30 MB gz** the equivalent non-diff
  view already achieves, rather than 24 MB.
- A one-commit diff and a many-commit diff of the same scope produce *different* output — today they
  do not.
- Whatever is decided about boundary-crossing edges is written into this plan and reflected in the
  rendered legend, so a reader knows what the diagram is claiming.

## 5. Why now

A consumer has begun posting this diagram as a comment on every pull request. It works — the mermaid
is 17 685 characters, well inside GitHub's 65 536 limit — but it describes 93 repository-wide
components for a change confined to one subtree. The next step serves the interactive HTML from a
hosted surface, where 24 MB per pull request becomes a storage and cleanup problem that mostly
disappears once this lands.

## Tracking: Initial implementation — 2026-09-15

**Implemented:** Scoping strategy chosen (keep in-scope + boundary neighbours), boundary nodes
styled distinctly as `«boundary»`, classes made opt-in with `--classes/--no-classes` flag
defaulting to `--no-classes` for component type.

**Changes (initial):**
- Rust core: `component::scope()` filters to in-scope + boundary neighbours, marks boundary
  with `is_boundary: bool`, styles boundary nodes in Mermaid output
- Python CLI: `_component_scope_path()` computes scope; `--classes/--no-classes` flag on both
  `diff` and `serve` commands; all three call sites updated (diff HTML, diff mermaid, serve diff)
- All tests pass (31 Rust + 29 Python); pre-commit clean

**Corrections to plan §4:**

1. **Third bullet — artifact size insensitivity.** The claim was that "a one-commit diff and
   a many-commit diff of the same scope produce *different* output — today they do not." This is
   **incorrect for mermaid** (measured on h@17011fa: `HEAD~1` vs `HEAD~50` diffs produce different
   mermaid, ~5791 vs ~6370 bytes, 25 lines differ). The insensitivity is **HTML artifact size only**
   — ~20 000 embedded class bodies dominate the payload and swamp the delta markers, a `classes`
   hardcode not a scoping bug. This fix (§3.5 of feature spec) separates the two problems.

2. **Plan §2, call sites.** Two sites were listed (`diff` HTML and mermaid); **there is a third:**
   `serve`'s `build_page()` diff mode also had both defects. All three now fixed.

## Tracking: Revision — 2026-09-15

Review of PR #21 found issues in the initial implementation. All addressed in this revision.

**Decision A — HTML view MUST render boundary nodes:**
The initial implementation exported `"boundary"` in `to_json` but `template-component.html`
never read it. Now fixed: boundary nodes render with dashed grey stroke and `«boundary»`
stereotype; the legend gains a `«boundary»` entry. Colours live once in `palette.rs`
(`BOUNDARY` constant + `mermaid_boundary_classdef()` / `css_boundary_variables()`), both
Mermaid and HTML CSS read from there. The hardcoded `#f6f8fa/#57606a` literals in the Mermaid
renderer were replaced with `palette::mermaid_boundary_classdef()`.

**Decision B — path inside a component root is an ERROR:**
`vizzle diff --type component packages/vizzle-cli/src` previously returned an empty diagram
and exited 0. The CLI now raises a `ClickException` naming the nearest enclosing component root:
```
Error: no component is rooted at packages/vizzle-cli/src
The nearest enclosing component is packages/vizzle-cli
```

**F1 — boundary × changed styling collision:**
Boundary nodes were getting change fill/stroke classes even though a change outside scope is
irrelevant to a reviewer. Excluded boundary nodes from diff-class attachment and glyph in
both Mermaid (`component.rs:870`, `component.rs:780`) and HTML (`drawNode` in template).
Rule documented in `docs/diagram-types/component.md` §6.1.

**F2 — `_component_scope_path` uncaught ValueError:**
Added `.relative_to(root)` resolution for the root path and catches `ValueError` to emit
a `ClickException` for paths outside the repo. Returns `as_posix()` so separators match git.

**F3 — tests strengthened:**
Rewrote `scope_filters_to_path_and_boundary_neighbours` and
`scope_keeps_only_edges_where_at_least_one_endpoint_is_in_scope` to assert exact component
sets and exact edge sets. Added `scope_drops_boundary_to_boundary_edges` fixture covering
boundary-to-boundary edges (must be dropped). Added two Python CLI tests:
- `test_component_diff_scoped_to_package_root_has_fewer_components` — asserts scoped count
  differs from unscoped
- `test_component_diff_refuses_non_root_path` — asserts Decision B refusal and message

All three Rust scope tests demonstrated failing against identity `scope()` before trusting them.
Python scope test demonstrated failing when `scope=` was dropped.

**F4 — `serve --no-classes` was silently ignored outside diff mode:**
Fixed by passing `effective_classes` (with `True` default for non-diff) to
`component_json_from_dir`. Deduplicated the `else False` default via `_effective_classes_for_diff()`.

**F5 — SKILL.md overstated `--no-classes`:**
Fixed wording: `--no-classes` is the default for component *diffs*, not for `vizzle component`.

**F6 — dead code removed:**
`from_kept && to_kept && (from_kept || to_kept)` simplified to `from_kept && to_kept`.
Redundant `!is_in_scope()` guards removed.

**Final state:** 33 Rust tests + 31 Python tests pass; pre-commit clean.
