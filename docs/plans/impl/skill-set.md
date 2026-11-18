# One skill set: vizzle's plugin becomes the home of code comprehension

**Status:** draft for review, 2026-11-18. §9 holds the open decisions, each with a recommendation.
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
| `skills/generated-diagrams/scripts/` | ~1,000 | The JS/Python AST extractors, mermaid emitter, sanitiser, `render.sh` | **Yes, superseded** by vizzle-core's extractors, `doc` and `render` |
| `evals/` | — | A comprehend-code eval set (4 cases, one iteration, `grade.py`) | No |

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
| Container / component (the shape of a repo or package) | the `c4-container` / `c4-component` skills, by hand | `vizzle component` (`--focus`, `--around`, `--highlight`): derived, so it cannot invent a box |
| Code (classes, contracts) | `c4-code`, by hand | `vizzle class` scoped to the question |
| A change | — | `vizzle diff` |
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
- **The code-comprehension repository itself.** It stays as it is: standalone, independent, unchanged by this work.

## 6. Acceptance

1. vizzle's plugin carries `vizzle-diagrams`, `explain-code` and `design-diagrams`, and the two commands. Each skill's
   `description` names the questions that trigger it, without overlapping another's.
2. Every command and flag the three skills name exists in the vizzle they ship with (the repo's existing skill check,
   extended to all three, holds it).
3. Nothing in vizzle's plugin names `code-comprehension`, `comprehend-code`, `generated-diagrams` or
   `c4-mermaid-plugin` as something to load.
4. One palette: every colour a skill gives is `vizzle palette`'s (§9 D3).
5. The comprehend-code eval set runs against `explain-code` and scores at least what comprehend-code scored (§9 D5).
6. The plugin's version is the release's (`bump-version.sh` already moves it).

## 7. What h does afterwards (a separate h PR, after the vizzle release)

- `.claude/settings.json`: drop the `code-comprehension-marketplace` entry and its enable line.
- `cli/charts/workflows/values.yaml`: drop `stiproot/code-comprehension` from `plugins.marketplaces`; update the
  vizzle comment ("vizzle derives and renders the diagram the comprehension skills choose the level for"). Re-freeze
  the compose fixtures and re-bless the render snapshot, which mirror that list.
- `.claude/skills/diagrams/SKILL.md`, `CLAUDE.md`, `docs/diagrams/README.md` and the two `-class` docs: point at
  vizzle's skills where they say "the `generated-diagrams` plugin skill".
- Runs that pass `plugins=code-comprehension@…` (none found in h or the operator's workspace on 2026-11-18) switch to
  `vizzle@vizzle-marketplace`.
- The operator's workspace makes the same settings change, and its comprehension task names `explain-code`.

## 8. Build plan

The driver writes the three skills and the two commands by hand in one branch (they are prose, and the move is small);
an independent reviewer checks the branch against §6; the operator reviews and merges. Then a release (minor) and the
h PR of §7.

## 9. Decisions (open; each with a recommendation)

| | Question | Recommendation | Why |
|---|---|---|---|
| D1 | One skill or three? | **Three** (§3) | One 500-line skill triggers on everything and is read top to bottom for any question. Reading, answering and designing are different questions; the existing skill already has three halves |
| D2 | Keep the dependency on `c4-mermaid-plugin`? | **No.** vizzle derives the structural levels; context and deployment get short hand-drawing notes in `design-diagrams`. Whether h keeps loading `c4-mermaid-plugin` for other reasons is outside this spec | A skill that needs another plugin to work is not a single source of truth. The levels it was used for are the ones vizzle now derives |
| D3 | Two delta palettes disagree (code-comprehension: Tailwind green/amber/red; vizzle: Primer `diffAdded`/`diffModified`/`diffRemoved`). Which wins? | **vizzle's**, everywhere. The "rejected path" red maps to `diffRemoved` / the `before` tint, and the `[CHANGED]` message prefix is kept as a convention | One palette is the point of `vizzle palette`; vizzle's diagrams already draw in it, so hand-drawn ones beside them must match |
| D4 | Rename the commands or keep `/explain`, `/map-code`? | **`/vizzle:explain`, `/vizzle:map`** | Plugin commands are namespaced anyway; the old names would read as code-comprehension's |
| D5 | Port the eval set? | **Yes, as `plugins/vizzle/evals/`**, adapted to `explain-code`, run once to set a baseline | Without it, "explain-code is at least as good" is a claim; with it, a measurement. It is four cases and a grader |
| D6 | Does `explain-code` call vizzle itself, or tell the agent to? | **Tell the agent** (the commands in the skill prose, as `vizzle-diagrams` does) | Skills here are prose over a CLI; no new runtime |
| D7 | What happens to the code-comprehension repo? | **Nothing** (the operator's decision) | It stays standalone and independent. A README line pointing at vizzle is the repo owner's call, not this spec's |
