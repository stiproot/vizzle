# One skill set: vizzle's plugin becomes the home of code comprehension

**Status:** built, 2026-11-18. The operator merged this spec with its recommendations, so D1–D10 stand as written in
§9; §10 records how each acceptance item was met.
**Established:** 2026-11-18
**Follows:** `docs/plans/impl/hybrid.md` (§7 named this as separate work).

## 1. Why

The operator's rule is that vizzle is the source of truth for diagramming tooling and skills. Two of the three
pieces that broke that rule have moved: the atoms (`extract`, `overlay`, `draw`; 0.14.0) and the hybrid architecture
style (`vizzle hybrid`; 0.15.0). The third is the **code-comprehension** plugin (`stiproot/code-comprehension`,
0.2.2), which h and its runs still load beside vizzle's.

The operator decided on 2026-11-18: **retire code-comprehension from h's use.** The repository stays as a standalone,
independent repo, but h stops loading its plugin, and **vizzle's plugin is the single source of truth for diagramming
and code comprehension.** This spec says what moves into vizzle's plugin, what does not, and what h changes after.

## 2. What code-comprehension carries today

| Piece | Lines | What it is | Already in vizzle? |
|---|---|---|---|
| `skills/comprehend-code/SKILL.md` + `references/level-selection.md` | 37 + 21 | How to **answer a question about code**: lead with one diagram at the right C4 level, ground every box in a file path, 5–9 elements, a four-part brief (answer · diagram · walkthrough · where to look next), iterate on the same diagram | No. vizzle's skill is about *getting* a diagram, not about answering with one |
| `commands/explain.md` | 10 | `/explain <question>`: runs comprehend-code end to end | No |
| `commands/map-code.md` | 8 | `/map-code [scope]`: a C4 container or component map of a repo | Half: `vizzle component` is that map, derived instead of drawn |
| `skills/generated-diagrams/SKILL.md` | 187 | The `gen:c4-code` manifest format, the extractor contract, how to invoke the toolkit | **Yes, superseded.** `vizzle doc` reads the same manifests (and a `scope.path` form the toolkit lacks); `vizzle render` replaces `render.sh` |
| … same skill: mermaid syntax traps, C4 layout traps, the delta-colour convention, UML component encoding | ~45 of the 187 | Hand-drawing advice, each trap from a real broken diagram | Partly: `vizzle palette` and the skill's "Drawing a diagram yourself" cover colours, with **different** values (§9 D3) |
| `skills/generated-diagrams/scripts/` | ~1,000 | The JS/Python AST extractors, mermaid emitter, sanitiser, `render.sh` | **Yes, superseded** by vizzle-core's extractors, `doc` and `render`, with one gap: Python `consts` (§5, §9 D9) |
| `skills/generated-diagrams/references/toolkit-architecture.md` | 50 | How the toolkit's extractors are laid out, for extending it | Not needed: the toolkit is not ported; vizzle-core's parsers are documented in its own code |
| `package.json` (`gen-code-diagram` / `render-diagram` bins, library exports) | — | The toolkit published as an npm package | Not needed: `vizzle doc` / `vizzle render` are the commands |
| `scripts/validate.sh` | — | The repo's plugin validation (manifests, frontmatter) | The idea moves: §8 builds vizzle's own skill check |
| `.claude-plugin/marketplace.json` (root) | — | The marketplace entry that installs the plugin | vizzle has its own; h's settings stop naming this one (§7) |
| `evals/` | — | A comprehend-code eval set: 4 cases, 3 runs per configuration, `grade.py` (Python) | No (§9 D5) |

`comprehend-code` names a companion plugin, `c4-mermaid-plugin`, for C4 syntax (§9 D2).

## 3. The skill set after the move

vizzle's plugin carries one skill today, `vizzle-diagrams` (379 lines): reading code, then sections on overlay, hybrid
and the palette that grew into it. It becomes **three skills**, one per job, so each triggers on its own question and
stays readable:

| Skill | Job | Comes from |
|---|---|---|
| `vizzle-diagrams` (name kept: installs and triggers already point at it) | **Reading** code: orient in a repo, scope a class diagram, keep a committed diagram true (`doc`), see what a change did (`diff`) | vizzle-diagrams §1–3, "Reading the output", "Do not" |
| `explain-code` (new) | **Answering** a question about code with a diagram: pick the level, ground every box, the four-part brief, iterate on one diagram | comprehend-code + level-selection, rewritten so vizzle supplies the structural levels (§4) |
| `design-diagrams` (new) | **Designing**: planned parts over real code (`overlay`), architecture pictures (`hybrid`), sequence and state diagrams drawn by hand in vizzle's palette, the mermaid traps, the UML component encoding, and **publishing** (PNG/SVG for Notion and Slack, HTML for people) | vizzle-diagrams' overlay, hybrid, palette and "When the human wants the diagram" sections + generated-diagrams' hand-drawing advice |

Two commands move with them, renamed into vizzle's namespace: `/vizzle:explain <question>` (runs `explain-code`) and
`/vizzle:map [scope]` (runs `vizzle component` at the right scope and writes the walkthrough).

## 4. How `explain-code` differs from `comprehend-code`

The method stays: one primary diagram at the highest level that answers the question, every element grounded in a
`path:line`, small diagrams, the four-part brief, iterate on the same diagram. What changes is **where the diagram
comes from**:

| Level | comprehend-code drew it with | explain-code gets it from |
|---|---|---|
| Container / component (the shape of a repo or package) | the `c4-container` / `c4-component` skills, by hand | `vizzle component` (`--around` with `--depth`, `--highlight`): derived, so it cannot invent a box |
| Code (classes, contracts) | `c4-code`, by hand | `vizzle class` scoped to the question |
| A change | — | `vizzle diff` (`--focus` keeps only the changed components and their edges) |
| A planned change | — | `vizzle overlay` + `draw` |
| System context, deployment (not in the code) | `c4-system-context`, `c4-deployment` | hand-drawn mermaid, short syntax notes in `design-diagrams`; or a `hybrid` concept picture when it will be kept |
| Behaviour (sequence, state) | mermaid by hand | mermaid by hand, in `vizzle palette` colours (`design-diagrams`) |

"Ground every element" gets cheaper: a derived diagram is grounded by construction, so the walkthrough only anchors
the boxes the answer talks about.

## 5. What does not move

- **The toolkit scripts** (`generated-diagrams/scripts/`): vizzle-core does the same job in Rust, behind `doc` and
  `render`. Nothing is ported.
- **The manifest format documentation**: `docs/diagram-types/class.md` and the `vizzle doc` section already document
  `gen:c4-code`, including the `scope.path` form. The skill points there.
- **One toolkit behaviour vizzle lacks, decided in §9 D9:** the toolkit's Python extractor resolves any module-level
  name listed in a manifest's `consts` and prints a string constant's value; vizzle resolves only UPPER_SNAKE names
  (`parse/python.rs`, `extract_module_const`), so a manifest naming a lowercase constant fails. Everything else the
  toolkit's manifest does, vizzle's `curated.rs` and parsers already do (fields, the `external` kind, realization
  edges from a const's type, Effect `Schema.Struct`, unions, interfaces, dataclasses).
- **The code-comprehension repository itself.** It stays as it is: standalone, independent, unchanged by this work.

## 6. Acceptance

1. vizzle's plugin carries `vizzle-diagrams`, `explain-code` and `design-diagrams`, and the two commands. Each skill's
   `description` names the questions that trigger it, without overlapping another's.
2. Every command and flag the three skills and two commands name exists in the vizzle they ship with. vizzle has no
   such check today; §8 builds one (a test that reads each `SKILL.md` and command file and checks every `vizzle <cmd>`
   and its flags against the CLI), and CI runs it.
3. Nothing in vizzle's plugin names `code-comprehension`, `comprehend-code`, `generated-diagrams` or
   `c4-mermaid-plugin` as something to load.
4. One palette: every colour a skill gives is `vizzle palette`'s (§9 D3).
5. An eval set for `explain-code` exists and has been run once to set its baseline (§9 D5). It cannot reuse
   comprehend-code's score as the bar: that grader asserts hand-drawn C4 (`C4Container`, `C4Component`, an explicit
   boundary, 5–9 elements) and points at one machine's checkout, so its assertions are rewritten for derived diagrams.
6. The plugin's version is the release's (`bump-version.sh` already moves it).
7. If §9 D8 is accepted, the plugin carries a Codex manifest at the release's version, and the skills load under both
   agents.

## 7. What h does afterwards (a separate h PR, after the vizzle release)

- `.claude/settings.json`: drop the `code-comprehension-marketplace` entry and its enable line.
- `cli/charts/workflows/values.yaml`: drop `stiproot/code-comprehension` from `plugins.marketplaces`; update the
  vizzle comment ("vizzle derives and renders the diagram the comprehension skills choose the level for"). Edit the
  list in `cli/h/tests/test_render.py` (it hard-codes the marketplace URLs), then re-freeze the compose fixtures
  (`parity.json` mirrors the list) and re-bless the render snapshot.
- `c4-mermaid-plugin` (§9 D10): `.claude/skills/diagrams/SKILL.md` tells agents to load its `c4-*` skills for C4
  structure, and `.claude/settings.json` enables it. The h PR decides whether it stays.
- A history comment in `scripts/check-plugins.mjs` mentions code-comprehension; reword it or leave it.
- `.claude/skills/diagrams/SKILL.md`, `CLAUDE.md`, `docs/diagrams/README.md` and the two `-class` docs: point at
  vizzle's skills where they say "the `generated-diagrams` plugin skill".
- Runs that pass `plugins=code-comprehension@…` (none found in h or the operator's workspace on 2026-11-18) switch to
  `vizzle@vizzle-marketplace`.
- The operator's workspace makes the same settings change, and its comprehension task names `explain-code`.

## 8. Build plan

The driver writes, in one branch: the three skills and the two commands (prose), the skill check of acceptance 2 (a
test in `packages/vizzle-cli/tests`, run by CI), the rewritten eval set and its baseline run (D5), the Codex manifest
(D8) and the Python constants change (D9) if accepted. An independent reviewer checks the branch against §6; the
operator reviews and merges. Then a release (minor) and the h PR of §7.

## 9. Decisions (accepted as recommended, 2026-11-18)

| | Question | Recommendation | Why |
|---|---|---|---|
| D1 | One skill or three? | **Three** (§3) | One 500-line skill triggers on everything and is read top to bottom for any question. Reading, answering and designing are different questions; the existing skill already has three halves |
| D2 | Keep the dependency on `c4-mermaid-plugin`? | **No.** vizzle derives the structural levels; context and deployment get short hand-drawing notes in `design-diagrams`. Whether h keeps loading `c4-mermaid-plugin` for other reasons is outside this spec | A skill that needs another plugin to work is not a single source of truth. The levels it was used for are the ones vizzle now derives |
| D3 | Two delta palettes disagree (code-comprehension: Tailwind green/amber/red; vizzle: Primer `diffAdded`/`diffModified`/`diffRemoved`). Which wins? | **vizzle's**, everywhere. The "rejected path" red maps to `diffRemoved` / the `before` tint, and the `[CHANGED]` message prefix is kept as a convention | One palette is the point of `vizzle palette`; vizzle's diagrams already draw in it, so hand-drawn ones beside them must match |
| D4 | Rename the commands or keep `/explain`, `/map-code`? | **`/vizzle:explain`, `/vizzle:map`** | Plugin commands are namespaced anyway; the old names would read as code-comprehension's |
| D5 | Port the eval set? | **Yes, rewritten**, as `evals/explain-code/` at the repo root (not in the plugin, so installs don't carry it). The grader stays stdlib-only Python run with the repo's `uv` dev environment; the target is a checkout of this repo, not a path on one machine; the assertions check grounding, the brief's four parts and that the diagram came from vizzle | Without it, "explain-code works" is a claim. The old assertions encode the hand-drawn C4 method this spec replaces, so they are rewritten, and the first run sets the baseline |
| D6 | Does `explain-code` call vizzle itself, or tell the agent to? | **Tell the agent** (the commands in the skill prose, as `vizzle-diagrams` does) | Skills here are prose over a CLI; no new runtime |
| D7 | What happens to the code-comprehension repo? | **Nothing** (the operator's decision) | It stays standalone and independent. A README line pointing at vizzle is the repo owner's call, not this spec's |
| D8 | Must the skills work under Codex too? | **Yes: add a Codex manifest** (`plugins/vizzle/.codex-plugin/plugin.json`, skills at `./skills/`, moved by `bump-version.sh`) and keep skill frontmatter to the two keys both agents read. The commands stay Claude-only | h's production agent is Codex, and h's own plugin check already requires both manifests. Neither plugin has one today, so Codex runs get no comprehension skill at all |
| D9 | Python `consts`: vizzle resolves only UPPER_SNAKE names; the toolkit resolved any listed name and printed string values | **Resolve any module-level name a manifest lists explicitly**; keep UPPER_SNAKE for what a `scope.path` manifest discovers on its own. Printing values: not now | An explicit list is the author saying "this one"; refusing it is a regression for anyone moving a manifest over. Auto-discovery keeps its filter so ordinary assignments stay out |
| D10 | Does h keep `c4-mermaid-plugin`? | **The h PR of §7 decides.** Recommendation there: keep it for now (h's policy uses it for hand-drawn C4 structure), and revisit once `design-diagrams` has carried a C4 picture | This spec drops vizzle's dependency on it; whether h still wants it is a separate, smaller question with its own users |

## 10. How acceptance was met (2026-11-18)

| # | Met by |
|---|---|
| 1 | `plugins/vizzle/skills/{vizzle-diagrams,explain-code,design-diagrams}/` and `commands/{explain,map}.md`. Each description names its own questions and points at the other two for theirs |
| 2 | `packages/vizzle-cli/tests/test_plugin.py`, in CI's existing pytest step: every `vizzle <command>` and flag in a skill or command file is checked against the Click command tree, and a flag named on its own must exist on some command. Mutation-checked: a flag on the wrong command, a made-up flag and an off-palette colour each fail it |
| 3 | The same test fails on any line in the plugin naming code-comprehension, comprehend-code, generated-diagrams or c4-mermaid |
| 4 | The same test: every colour in the prose must be `vizzle palette -f json`'s, except the one counter-example `design-diagrams` shows on purpose (listed, with its reason). `design-diagrams` maps the old delta convention onto it (D3) |
| 5 | `evals/explain-code/` (D5): baseline 86% with the skill against 60% without, 24 runs; two skill defects found and fixed on the way; open items in its README |
| 6 | `bump-version.sh` moves both manifests; `check` fails when either drifts (shown by hand) |
| 7 | `plugins/vizzle/.codex-plugin/plugin.json` + `.agents/plugins/marketplace.json` (D8), held by the same test; skill frontmatter is exactly `name` + `description` |

D9 shipped as its own commit: a lowercase public Python module assignment is kept in `CodeGraph.module_values`,
reached only by a manifest's `consts`, never drawn or diffed (`docs/curated-diagrams.md` §4).

Found while building, outside this spec: `vizzle diff` ignores Rust files entirely (`git.py`'s source suffixes predate
Rust support), so a Rust-only change reads as "no changed Python/TypeScript files". A separate fix.
