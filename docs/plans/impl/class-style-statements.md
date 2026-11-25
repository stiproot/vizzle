# Class diagrams colour with `style` lines, so they render on Mermaid 10.7+

**Status:** built, 2026-11-25, in the same PR as the plan (the operator asked for the build on the plan's branch rather
than a merge first). D1–D5 as recommended; §9 records how each acceptance item was met.
**Archived:** 2026-11-25

Lifted to:
- `docs/diagram-types/class.md` §6 — the `style` rule, the version table, the 10.7.0 floor.
- `crates/vizzle-core/src/mermaid.rs` — the emitter comment and `assert_class_diagram_floor`.
- `scripts/check-mermaid-floor.sh` + `tests/fixtures/mermaid-floor/` — the manual render check.
**Established:** 2026-11-20
**Evidence:** an investigation outside this repo (a consumer reported "vizzle class zoom fails in an older Mermaid").
Its fixture, a 3-namespace class zoom from a real diff, came from a private codebase, so the tests use a zoom of this
repo's own history instead (same shape: namespaces, generics, every change kind).

## 1. Why

vizzle colours a class diagram in two ways:

- **The diff.** `cssClass "id1,id2" diffAdded` lines, then a `classDef diffAdded fill:…` block, all at the end
  (`crates/vizzle-core/src/mermaid.rs:236-252`, `palette::mermaid_classdefs`).
- **The lens** (`--highlight`, `--around`). `class id["…"]:::highlight` on the class line, then `classDef highlight` and
  `classDef context` (`mermaid.rs:295-300`, `palette::mermaid_lens_classdefs`).

Both depend on `classDef` inside a `classDiagram`, and **Mermaid only accepts that from 11.4.1**. Measured on every
release from 10.2 to 12.2 with plain Mermaid from jsdelivr in headless Chrome:

| Mermaid | The fixture as vizzle prints it today | The same fixture with one `style <id> <props>` line per class |
|---|---|---|
| ≤ 10.1 | fails at `namespace` | fails at `namespace` |
| 10.2 – 10.6 | fails at the first `classDef` | fails (`style` not yet allowed in a classDiagram) |
| **10.7 – 11.3** | **fails at the first `classDef`** | **renders, coloured** |
| 11.4.1 – 12.2 | renders | renders, the same colours (SVG fill counts equal: 25 added, 119 modified) |

A Mermaid parse error loses the whole diagram, not only its colours. So a class diagram vizzle prints is blank on
any viewer whose Mermaid is 10.7–11.3. Older Mermaid ships in older editor plugins, wiki and docs renderers, and in
viewers that lag the release. The skills tell agents to paste vizzle's output as printed, so the output itself must
render; a flag that people have to know about does not help.

Out of the consumer report: the failure that consumer saw had a different cause (three Mermaid preview renderers
in one editor drawing over each other). This plan fixes the real floor the investigation found along the way.

Flowcharts are not affected. `classDef` in a `flowchart` has worked for years, so `vizzle component` and its diff are
unchanged.

## 2. Design

**Class diagrams emit no `classDef`, no `cssClass` and no `:::`.** Every coloured class gets one line at the end:

```text
    style <id> fill:#dafbe1,stroke:#1a7f37,stroke-width:2px,color:#1a7f37
```

The properties are the ones the `classDef` lines carry today, from the same `palette.rs` table, so the colours do not
change and the HTML view still agrees with the Mermaid view (`palette.rs` module comment).

- **Which colour a class gets.** With the lens on, every class gets the lens colour (`highlight` or `context`). With
  the lens off, a changed class gets its change colour and an unchanged class gets no line. In a diff with a lens, a
  changed class still shows its change glyph in the label (`✚ ✖ ✱`), as today. Today's output draws the same:
  measured with the drawn (computed) fill of each box, a class carrying both `:::highlight` and `cssClass … diffAdded`
  is filled with the lens colour on Mermaid 11.4.1 and 11.17.2, and a context class that changed is filled grey (D2).
  The diff with a lens is reachable only through an imported JSON graph that carries both (`lib.rs:590-598`); the CLI's
  `diff` has no lens.
- **One line per id, never two.** A `style` line applies to one node, and two lines for one node would leave the result
  to Mermaid's merge order. The rule above picks one property list per class, so each id appears in at most one
  `style` line (A1 asserts it).
- **Externals stay uncoloured.** `ext_*` classes (`mermaid.rs:230-235`) are drawn outside `write_class` and get no
  `:::` today; they get no `style` line either.
- **The id is the sanitized one.** `style` names the same `ids[...]` value (`sanitize_id`) that `cssClass` names today,
  so namespaced and generic classes are addressed the same way. The removed class's dashed outline
  (`stroke-dasharray:6 4`) survives as a `style` property: measured `6px, 4px` on Mermaid 10.7.0.
- **Order.** The `style` lines go after every class and relation, where the `classDef` block is today. A `style` line
  names an id, so it must come after the class it styles.
- **One function owns the properties.** `palette.rs` gains `class_style(change) -> Option<String>` and
  `lens_style(lit: bool) -> String`, which return the property list only (`fill:…,stroke:…`). The existing `classDef`
  builders are rewritten on top of them, so flowcharts and class diagrams cannot drift apart. `mermaid_classdefs` is
  shared with flowcharts (`vizzle component`, `vizzle draw`), so its output must stay byte-identical (A8).
- **Comment lines stay.** `%% vizzle: highlight: N of M classes` and `%% vizzle: N classes, M relations` are
  unchanged.

**Hand-drawn class diagrams follow the same rule.** `vizzle palette` and the `design-diagrams` skill tell agents to
write `class X:::diffAdded` plus the `classDef` block. That has the same floor, so they change too (D3).

**Cost.** One line per coloured class instead of one per change kind. A 300-class `--highlight` view gains about 300
short lines. Mermaid's size limit is in characters (50,000 by default), and a `style` line is about 90 characters, so a
diagram near the limit can now cross it. The CLI's `oversized` flag measures the whole text against that limit
(`cli.py:213`, `managed.MERMAID_LIMIT`), so it counts the new lines with no change; A6 checks it on a real case.

## 3. Behaviour changes

| Who | Before | After |
|---|---|---|
| A viewer with Mermaid 10.7–11.3 | every coloured class diagram fails to parse | renders, coloured |
| A viewer with Mermaid 11.4.1+ (GitHub, mermaid-cli 12, the VS Code preview) | renders | renders the same picture |
| A repo with a committed class diagram under `vizzle doc --check` | clean | **drift** until `vizzle doc` regenerates it: the text changed |
| A script that greps the Mermaid for `cssClass` or `classDef diffAdded` in a class diagram | finds them | finds `style` lines instead. The JSON export (`--format json`) is unchanged and is the stable interface |
| `vizzle palette` | prints the `classDef` block, says to attach with `:::` | prints the `style` properties per meaning for class diagrams, and the `classDef` block for flowcharts. JSON gains `classStyles`, keeps `classDefs` |
| Flowcharts (`vizzle component`, its diff and `--highlight`) | unchanged | unchanged |

## 4. Decisions

| # | Question | Recommendation | Alternative |
|---|---|---|---|
| D1 | How do class diagrams get coloured? | One `style` line per coloured class, always (§2) | A `--mermaid-compat` flag. Rejected in the handover: the output is pasted as printed, so a flag protects no one who does not know about it |
| D2 | A diff with a lens: whose colour wins? | The lens, as today. The glyph still shows the change | Diff wins on changed classes. That would change today's picture |
| D3 | Hand-drawn class diagrams | `vizzle palette` and the `design-diagrams` skill switch to `style` lines for class diagrams; `classDefs` stays in the JSON for flowcharts | Leave the skill alone. Then agents keep drawing diagrams with the old floor next to vizzle's |
| D4 | Regression test | (a) A core unit test: no class-diagram output contains `classDef`, `cssClass` or `:::`, across diff, lens and both. This runs in CI. (b) `scripts/check-mermaid-floor.sh`, opt-in: renders the fixtures with `@mermaid-js/mermaid-cli@10.9.1` (it bundles Mermaid 10.9) and with the pinned 12.0.0, and fails if either fails | Put (b) in CI too. It downloads Chrome through puppeteer on each run (minutes), and (a) already catches the change that broke it |
| D5 | Release | **0.19.0**, a minor: committed diagrams drift under `vizzle doc --check`, which is a visible change for consumers. The usual steps: `bump-version.sh bump minor` (it also moves the `examples/*.svg` hybrid headers), tag, approve PyPI, then `bump-version.sh pin` for `pr-diagram.yml` | 0.18.1. The picture is the same, but the text a check compares is not |

## 5. Changes

| File | Change |
|---|---|
| `crates/vizzle-core/src/palette.rs` | `class_style`, `lens_style`; `mermaid_classdefs` / `mermaid_lens_classdefs` built from them, output unchanged; the doc comment at 243-246 (the ordering quirk) now says the classDefs are for flowcharts; a test that the two forms carry the same properties |
| `crates/vizzle-core/src/mermaid.rs` | `write_class` drops `:::`; the end block emits `style` lines (§2); the comments at 236-239 and 295-297 are rewritten; the tests at 628 and 637-638 assert `style m_B fill:#dafbe1…`, not `cssClass` |
| `crates/vizzle-core/src/highlight.rs` | The module comment (lines 6-12) that justifies `:::` says why class diagrams now use `style` |
| `crates/vizzle-core/src/curated.rs` | Checked: it emits a classDiagram but no colours today. No change expected |
| `crates/vizzle-py/src/lib.rs` | `diff_class_styles() -> Vec<(name, props)>` beside `diff_classdefs` |
| `packages/vizzle-cli/src/vizzle_cli/cli.py` | `vizzle palette`: text says `style <Name> <props>` for class diagrams, `classDef` for flowcharts; JSON adds `classStyles` |
| `packages/vizzle-cli/tests/test_cli.py` | Class-diagram assertions move to `style`: 70-73 (`cssClass`, "classDef must trail"), 1204-1209, 1224-1225. The component lens checks (1246-1247) and the flowchart palette check (1441) stay as they are. 543-546 also checks `classStyles` |
| `packages/vizzle-cli/tests/test_plugin.py` | 217 reads `palette["classDefs"]`: keeps working; add `classStyles` if it checks the skill's class-diagram advice |
| `plugins/vizzle/skills/design-diagrams/SKILL.md` | The `:::` placement rule (132-133), the palette lines (154, 176) and the colour table (182-184): class diagrams use `style <Name> …` from `vizzle palette`; one sentence on why (renders on Mermaid 10.7+) |
| `docs/diagram-types/class.md` | 313 (the "classDef after cssClass" quirk), 371 and 389-400 (the lens and why `:::`): the `style` rule, the version table from §1, the floor |
| `README.md`, `CLAUDE.md` | README 265-266 and CLAUDE.md 70 repeat the ordering quirk: rewritten for class diagrams. `component.md` 204 and 604 are flowcharts and stay |
| `examples/h-diff.mmd` | The one committed coloured class diagram (used by `site/`). It was drawn from another repo's history and cannot be regenerated here, so its two `cssClass` lines and `classDef` block are converted to `style` lines by hand, colours unchanged. `docs/diagrams/` has no coloured class diagram |
| `scripts/check-mermaid-floor.sh` + `tests/fixtures/mermaid-floor/*.mmd` | D4 (b): the zoom fixture, a lens view, a diff with a lens |

## 6. Acceptance

| # | Check |
|---|---|
| A1 | `cargo test -p vizzle-core`: the new shape test passes across diff only, lens only, and diff with lens. It asserts no `classDef`, `cssClass` or `:::`, at most one `style` line per id, and none for an `ext_*` id |
| A2 | `grep -c 'classDef\|cssClass\|:::'` on `vizzle class` / `vizzle diff --type class` / `--zoom` output from this repo is 0 |
| A3 | `scripts/check-mermaid-floor.sh` passes: each fixture in `tests/fixtures/mermaid-floor/` (the zoom from the investigation, a lens view, a diff with a lens; between them a namespaced class, a generic class and an external) renders on mermaid-cli 10.9.1 and on 12.0.0, the version `render.py` pins. Before the change, the zoom fixture fails on 10.9.1 (proves the check bites) |
| A4 | Drawn colours match: the computed fill of each box, read in headless Chrome, is the same for the old and new output on Mermaid 11.4.1 and 11.17.2, and the new output gives the same fills on 10.7.0. The probe that reads them is kept beside the fixtures as `tests/fixtures/mermaid-floor/fill-probe.py` (manual, like the floor check) |
| A5 | `vizzle palette` and `-f json` show the `style` form; `test_cli.py` covers both |
| A6 | The size of a large `--highlight` view before and after is recorded in §9, and one case whose new text lands within 10% of 50,000 characters shows `oversized` agreeing with `len(text) > 50_000` |
| A7 | Every CI gate: fmt, clippy, core tests, pytest, ruff format and check, `vizzle doc --check`, `vizzle hybrid --check`, `bump-version.sh check` |
| A8 | Component (flowchart) output is byte-identical before and after on this repo |

## 7. Out of scope

- Flowcharts' `classDef`s. They work on every Mermaid we measured.
- Mermaid older than 10.7. Class diagrams there also fail on `namespace` (≤ 10.1) or have no way to colour a class.
- The consumer's viewer. That was a local editor setup, fixed there, not in vizzle.

## 8. Review

An independent reviewer read the plan against the code (2026-11-20). Taken:
- D2 was argued from the order of the `classDef` lines, which is not how Mermaid decides. Now measured: the drawn fill
  of a lit, changed class is the lens colour on 11.4.1 and 11.17.2 (§2). 12.2.0-b.1 did not load in the probe; not
  claimed.
- Externals, the sanitized id and the one-line-per-id rule were unstated (§2, A1).
- §5 missed tests and docs that name the old form: `test_cli.py`, `test_plugin.py`, `highlight.rs`, `palette.rs`
  comments, README, CLAUDE.md, more of `class.md` and the skill. Added, with the flowchart ones marked unchanged.
- `mermaid_classdefs` is shared with flowcharts, so its output must not change (§2, A8).
- A3 names its fixtures, A4 names its probe, A6 has a threshold, D5 lists the release steps.

A second independent review read the built change (2026-11-25). Taken: the floor script's EXIT trap named a `local`
variable, so under `set -u` a green run exited 1 and left its temp dir (fixed: the path is expanded when the trap is
set); the fill probe matched ids as substrings and its docstring path only worked from the fixtures directory (fixed:
exact match, repo-relative example). Not taken: a class whose sanitized id is a Mermaid keyword (`style`, `class`)
would also break the existing `class` line, so it predates this change. Confirmed by the reviewer: the style loop uses
the same pruned graph and id map as the class lines, the flowchart `classDef` text is byte-identical to before, and no
class-diagram path still emits the old syntax.

## 9. Results

Built 2026-11-25 on the plan's branch.

| # | Result |
|---|---|
| A1 | `cargo test -p vizzle-core`: 194 passed. New: `renders_diff_styling` (rewritten), `the_lens_colours_every_class_with_style_lines`, `with_a_diff_and_a_lens_the_lens_colour_wins` (a removed class, an added lit class, an external), `style_lines_carry_the_classdef_properties`; all go through `assert_class_diagram_floor` |
| A2 | On this repo: the zoom (17 classes), `class --highlight` (40) and `diff --type class` (15): 0 lines matching `classDef\|cssClass\|:::`, one `style` line per coloured class |
| A3 | `scripts/check-mermaid-floor.sh`: on mermaid-cli 10.9.1 the zoom, lens and diff-lens fixtures render and the old-form control fails; on 12.0.0 all four render. 8/8 as expected (the first npx install of 12.0.0 took several minutes) |
| A4 | `fill-probe.py` on six zoom classes: the old (`classDef`) and new (`style`) output draw identical fills on 11.4.1 and 11.17.2 (`#fff1c2` modified, `#dafbe1` added); the new output draws the same on 10.7.0 |
| A5 | `vizzle palette` prints the class-diagram `style` properties and the flowchart `classDef`s; `-f json` adds `classStyles`. `test_cli.py` checks each `classStyles` entry is its `classDef` minus the prefix |
| A6 | Zoom of `e052d27~90..`: 48,076 characters, `oversized: false`. Of `e052d27~120..`: 51,019, `oversized: true`, matching `len > 50_000` both times. **Cost, measured:** that second zoom has 109 `style` lines (11,319 characters, 102 average); its old form would have been about 43,800, so the change alone pushed it over the limit. About 66 characters more per coloured class. `oversized` reports it, and pr-diagram already reads `zoom_oversized` |
| A7 | fmt, clippy (`-D warnings`), core tests, pytest (165 passed), ruff format and check, `vizzle doc --check`, `vizzle hybrid --check`, `bump-version.sh check`: all pass |
| A8 | `vizzle component .` and `--highlight vizzle-core`: byte-identical before and after. The component diff differs only in its title (the base was named differently) |
