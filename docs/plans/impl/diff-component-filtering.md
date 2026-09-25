# Filtering components in a diff

Status: **Complete** — 2026-09-15, released in v0.4.0 (the path-base fix in §6 in v0.5.0):
`-I/-E/-l` on `diff` (and honoured by `serve --diff`), applied to both revisions before build.
Raised by a consumer whose per-pull-request component diff renders test fixtures as architectural
components, with no flag available to exclude them.
Established: 2026-09-15
Archived: 2026-09-24

Lifted to:
- `docs/diagram-types/component.md` §6.2 — selection before scope, exclusion is total, the
  selection stated in every renderer, with the measurements.
- `crates/vizzle-core/src/walk.rs` — `Selector`, the one matcher, and the reason it matches a
  path by both spellings (§6 here).
- `packages/vizzle-cli/src/vizzle_cli/cli.py` — the core's `ValueError` becomes a one-line
  `Error:` at the click group.

## 1. The premise

A component root is "a directory with a manifest file sitting directly in it"
(`docs/diagram-types/component.md` §6). That rule is simple and right, and it means **anything
carrying a manifest is drawn as architecture** — including directories that are not:

- test fixtures that are themselves small repos, used to exercise tooling against a real manifest
- example or starter projects shipped alongside the library they demonstrate
- scaffolding templates
- vendored or sample code kept in-tree

A reviewer looking at a component diagram cannot tell these from real components, and a diagram that
shows a test fixture as part of the architecture spends its credibility on its first impression.

## 2. The capability already exists — it is just not on `diff`

`vizzle` already has a filtering vocabulary, exposed on two commands and absent on the third:

| command | `-I/--include` | `-E/--exclude` | `-l/--lang` |
|---|---|---|---|
| `component` | ✅ | ✅ | ✅ |
| `doc` | ✅ | ✅ | ✅ |
| **`diff`** | ❌ | ❌ | ❌ |

So this is an **inconsistency rather than a new feature**: the same glob vocabulary a user learns on
`vizzle component` stops working the moment they ask what a change did.

That the gap sits on `diff` specifically is what makes it worth fixing. `component` is run
deliberately by someone exploring; `diff` is the one wired into pull-request automation, seen by
every reviewer, and the one where a nonsense box is most expensive.

## 3. What to change

Put `select_options` on `diff` — the same `-I/--include`, `-E/--exclude`, `-l/--lang` the other two
commands take — and thread them through both revisions of the collection, not just the head.
Filtering only one side would make a fixture "removed" or "added" purely because the filter applied
asymmetrically.

Two questions for whoever picks it up:

1. **Does exclusion compose with path scoping, and in which order?** Scoping keeps in-scope
   components plus their boundary neighbours. If an excluded component is a boundary neighbour of an
   in-scope one, is it dropped, or kept as `«boundary»`? Dropping it is probably right — the user
   said they do not want to see it — but the edge into it then disappears too, and that edge is real
   structure. State the answer in the rendered legend either way.
2. **Should `--exclude` affect the change markers?** A PR that only touches excluded paths would
   render as "no structural change". That is arguably correct and arguably a lie; pick one and say
   which in `docs/diagram-types/component.md`.

## 4. Acceptance

- `vizzle diff --type component -E '<glob>'` omits matching components from the diagram.
- The same flags work identically on `component` and `diff` — a user learns the vocabulary once.
- Exclusion applies to **both** revisions, so nothing appears added or removed because of it.
- The composition with path scoping (§3.1) is decided, documented in
  `docs/diagram-types/component.md`, and covered by a test.

## 5. Why now

A consumer wired the component diff into pull-request review on 2026-09-15 and it works: scoped to
one package root the diagram is 6 components rather than 93, and the mermaid fits a comment
comfortably. One of those six is a test fixture. At six boxes a spurious one is a sixth of the
diagram, which is the difference between a reviewer trusting it and learning to skim past it.

## Tracking: Implementation — 2026-09-15

**Where the filtering lives.** One matcher, `walk::Selector` (language → include → exclude,
over the repo-relative path), extracted from the walker's inline loop and reused by the four diff
entry points (`diff_diagram`, `json_diff`, `component_diff_diagram`, `component_json_diff`), which
now take a `SelectOptions`. The bindings gained `include`/`exclude`/`langs` keyword args with
empty defaults, so nothing that did not pass them changed behaviour. The CLI puts the shared
`select_options` decorator on `diff`, and `serve` now uses the same decorator instead of its own
three copies whose help text said "class mode" — a fourth call site the plan's table missed (the
flags were there, but ignored in component diff mode).

**§3.1 — composition with scoping: selection first, then diff, then scope.** An excluded
component is dropped even as a boundary neighbour, and its edge goes with it. Reasoning and the
measured effect are in the spec (§6.2): on h, scoping to `packages/js/engine-core` gives 6/5;
adding `-E 'apps/**'` drops the `workflow-svc` boundary node → 5/4.

**§3.2 — change markers: exclusion is total.** A change confined to excluded paths renders as no
structural change, and the pr-diagram grep therefore reads "unchanged" for a fixture-only PR.
Recorded in the spec with the reason it is the truth about the selection rather than a lie about
the repository.

**Legend.** HTML legend entry (`selection: exclude …`), `stats.selection` in the JSON, and a
`%% vizzle: selection:` trailer on the Mermaid, which has no legend of its own. Wording uses rule
names (`include`/`exclude`/`lang`), not flag spellings — the core does not know how the CLI
spells `-E`.

**Filtering everything is an error** on the class diff (`no changed files match the
include/exclude/lang selection`), not an empty diagram. While making that message reach the user
it turned out that EVERY core error surfaced as a Python traceback (an invalid glob on today's
`vizzle component` did too), so the click group now translates the core's `ValueError` into a
one-line `Error:`. Test: `test_core_errors_are_reported_not_raised`.

**Also fixed in passing:** pre-commit's `check-yaml` failed on main against
`.h/charts/**/*.tmpl.yaml` — helm templates are not YAML until rendered; excluded.

**Tests:** 3 Rust (`walk` selector rules + invalid glob; `lib` selection-drops-on-both-sides,
selection-before-scope, filter-to-nothing is an error) and 6 Python CLI tests, including the
acceptance items one for one: excluded component gone, nothing added/removed because of it,
boundary neighbour dropped, fixture-only change is no change, legend carries the selection,
class diff refuses an empty selection. 38 Rust + 37 Python pass; pre-commit clean.

**Acceptance (§4) — all met.** The fourth bullet's "covered by a test" is
`test_component_diff_exclude_wins_over_boundary` (CLI) and
`selection_runs_before_scope_so_an_excluded_neighbour_is_not_a_boundary` (core).

## 6. Implementation note — the path base

Discovered while verifying §4's first acceptance criterion against a real tree, after the
matcher was already shared.

`Selector` matched the repo-relative path only, on the reasoning that one matcher gives one
meaning. That is necessary but not sufficient: the commands do not hand it the same kind of path.
A walk is rooted at the path the user names (`component harness/kikimora` sees `tests/...`); a
component diff collects the whole repository by design, because an edge's existence depends on
files the change never touched (`diff harness/kikimora` sees `harness/kikimora/tests/...`).

Measured on the consumer's tree before the fix:

| command | `-E 'tests/fixtures/**'` | `-E 'harness/kikimora/tests/fixtures/**'` |
|---|---|---|
| `component` | filtered | — |
| `diff` | **no effect** | filtered |

So the flag was present on `diff` and quietly did nothing for the glob a user would actually
write, which is the failure mode the change set out to remove. `Selector::within_scope` matches
a path under the scope by both spellings; stripping is prefix-anchored, so a scope-relative glob
cannot reach outside the scope. Covered by five unit tests and two CLI tests, the latter verified
by removing the fix and watching them fail.
