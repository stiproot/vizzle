# Curated diagrams

**Status:** specified, not yet built.
**Command:** `vizzle doc <paths|--dir>` and `vizzle doc --check` (proposed).

Every other spec here describes an **exhaustive** diagram: vizzle parses what
is there and draws all of it. This one describes the opposite mode — a diagram
whose *scope is chosen by a person* and whose *members are filled in by vizzle*
— and why both belong in the same tool.

## 1. Why a second mode exists

An exhaustive class diagram of h is 330 boxes. It is the right artifact for
orientation and a bad one for explanation: nobody puts 330 boxes in a design
doc. The diagram that goes in a document has 8 boxes, chosen because they carry
the story, with labelled edges a parser could never infer.

But a hand-drawn diagram rots. Member lists date fastest, so hand-writing them
is authoring instant drift.

**The split that resolves it**, and the whole idea of this mode:

| | Owned by | Because |
| --- | --- | --- |
| **Scope** — which symbols appear | a person | judgment; no parser can pick the 8 that matter |
| **Relations and their labels** | a person | `"merges env into"` is meaning, not syntax |
| **Notes, stereotype overrides** | a person | `<<Effect service>>` is not in the source |
| **Members** — fields, methods, signatures | **vizzle** | the part that goes stale |

Exhaustive mode answers *"what is this made of?"*. Curated mode answers *"here
is how this works"* and keeps answering it after a refactor.

## 2. Decision: adopt the existing manifest format

**Decided 2026-08-17.**

This mode is not new work invented here — it exists, in the
`code-comprehension` plugin, and it is already in use. `~/code/h` carries
**three managed class diagrams** (`docs/diagrams/{agent-cli,workflow-svc,h-cli}-class.md`),
each an HTML-comment manifest plus one generated mermaid fence.

**What was measured**, on `docs/diagrams/agent-cli-class.md`: 14 curated
entries against the ~40 symbols vizzle would draw for that package unprompted,
9 curated relations, 4 curated notes, 1 stereotype override.

**What was chosen.** vizzle reads the `gen:c4-code` manifest **as written**,
so those three documents work unedited. A new format would strand real
documents to gain nothing a reader would notice.

**What would change it.** A capability the format cannot express. Extend it
then, additively — the format tolerates unknown keys badly (it is JSON in an
HTML comment, so `--` is illegal anywhere inside), which is itself a
constraint worth respecting rather than fighting.

## 3. How an entry addresses a vizzle element

The manifest names a symbol by `file` + `symbol`; vizzle's graph keys on
`qualified` (`<module>.<Name>`). The mapping is mechanical and already exists:
`parse::module_path` turns `packages/js/agent-cli/src/invoker.ts` into
`packages.js.agent-cli.src.invoker`, so the entry resolves to
`packages.js.agent-cli.src.invoker.AgentInvokerService`.

**An entry that resolves to nothing is an error, not an omission.** Silently
dropping it would let a rename quietly empty a diagram — precisely the drift
this mode exists to prevent. `--check` fails; a regenerate reports the entry
and the file it looked in.

## 4. What vizzle supplies, per kind

| `kind` | Members come from | Notes |
| --- | --- | --- |
| `interface`, `class` | vizzle's parsed members | §2.1 of class.md |
| `union` | the arms | needs class.md §2.3, shipped 2026-08-17 |
| `module` | the entry's `functions` list, filtered from vizzle's `«module»` box | class.md §2.4 |
| `const` | **nothing** — the curated `note` is the whole body | no parser involvement |
| `external` | **nothing** — fully curated | a peer service, a binary |

Two of those five need no parsing at all, which is why this mode was never
blocked on parser fidelity.

### 4.1 Decision: curated mode ignores the density heuristics

**Decided 2026-08-17, from a real miss.** Resolving h's
`agent-cli-class.md` against vizzle's graph, 9 of 10 parseable entries matched
with correct member counts. The one that missed was
`StopReason = "completed" | "usage-limited" | "timeout" | "failed"` — a
string-literal union, which class.md §2.3 deliberately **skips**, because
drawing every literal union would drown a 330-box diagram.

In a diagram about classifying stops, those four outcomes *are* the story. The
manifest asked for it by name.

**So the §2.3 rule is a rendering decision, not a parsing one.** The parser
keeps every structural type alias; exhaustive mode applies the density rule
when rendering, and curated mode ignores it — because scope has already been
chosen by a person, and a heuristic protecting a reader from 330 boxes has
nothing to protect in a diagram of 14.

The general form, worth holding onto: **vizzle's judgment about what to *show*
must not become a limit on what it can *find*.** Anything the manifest can
name, vizzle must be able to extract.

`module` is the interesting one: vizzle's `«module»` box lists *every* public
function, and a curated entry lists the three that matter. The manifest
selects; vizzle supplies signatures for the selection. A named function that
vizzle cannot find is the §3 error.

## 5. The managed document

A markdown file containing an HTML-comment manifest and exactly one mermaid
fence. Regeneration replaces **the fence and nothing else** — prose around it
is the author's and is never touched.

**Never hand-edit a generated fence.** Edit the manifest or the code, then
regenerate. This is the same rule `code-comprehension` states, and it is the
only rule that makes `--check` meaningful.

## 6. `--check` is the point

```sh
vizzle doc --dir docs/diagrams          # regenerate every managed doc
vizzle doc --dir docs/diagrams --check  # exit non-zero if any would change
```

`--check` regenerates in memory and compares. Wire it into a repo's lint and a
refactor that changes a diagrammed contract fails the build until the diagram
catches up. Without it this mode is just a slower way to hand-draw.

This also closes `distribution.md` §2.4, which has been waiting on exactly
this and named it as the blocker.

## 7. CLI surface

```sh
vizzle doc <doc.md>...                  # regenerate the named docs
vizzle doc --dir <path>                 # every *.md under path carrying a manifest
vizzle doc --dir <path> --check         # verify, do not write
```

Docs without a `gen:c4-code` marker are ignored, so a directory mixing
generated and hand-authored diagrams is fine — h's `docs/diagrams/` is exactly
that: 3 managed class diagrams among 14 hand-authored sequence, state and C4
documents.

## 8. Out of scope

- **Curated component diagrams.** The same manifest idea applies one level up,
  but component detection is already manifest-driven and rarely needs
  narrowing. Revisit if a real repo wants it.
- **Rendering to PNG.** `mmdc` does this; vizzle should not grow a browser.
  Note the trap recorded in README: a whole-repo diagram exceeds mermaid's
  default `maxTextSize`. A curated diagram never will — being small is the
  point.
- **Inferring scope.** "Pick the 8 that matter" is the judgment this mode
  exists to preserve. A heuristic that guessed would remove the reason to use
  it.
- **Diff/change annotation on a curated diagram.** `ChangeKind` flows through
  the model and would work, but a design doc showing a change is usually
  showing a *proposed* one, which git cannot see.
