# Filtering components in a diff

Status: Proposed — not started. Raised by a consumer whose per-pull-request component diff renders
test fixtures as architectural components, with no flag available to exclude them.
Established: 2026-09-15

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
