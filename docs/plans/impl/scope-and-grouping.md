# Scoped diagrams, and grouping by component

Status: **Implemented** — Decisions A, B and C on `feature/scope-and-grouping`. Not yet
released; the newest tag is `v0.3.0`.
Established: 2026-09-20

Raised by a consumer wiring `vizzle` into a lint chain as a drift gate. Two gaps surfaced,
and they are separable: one is a missing renderer, the other is a missing manifest kind.

## 1. The premise

A consumer wants the detail the interactive component page shows — every class inside a
component, with members — as **mermaid**, so it can live in a repository, be reviewed in a
diff, and be checked by a lint step.

Today they cannot have it, for two independent reasons.

### 1.1 No command groups classes by component

| command | groups by | class detail | format |
|---|---|---|---|
| `vizzle class --group` | **module** (one file) | full | mermaid |
| `vizzle component -o x.html` | **component** | full | HTML only |
| `vizzle component` | component | none | mermaid |

The data is not the problem. `component_json_from_dir` already returns `ComponentClass {
component, class }` for every class — that payload is what drives the HTML drill-down. What is
missing is a renderer that writes `namespace <component>` where `mermaid.rs` currently writes
`namespace <module>`.

Measured on a 33-package Python tree (2,479 classes, 3,190 class relations): the component
payload carries every class tagged with its owner, and `mermaid.rs` exposes exactly one grouping,
`group_by_module: bool`.

### 1.2 A managed document cannot declare a scope

`vizzle doc --check` is the right shape for a lint gate and already exists. But a managed
document's manifest can only list **symbols by hand**:

```json
{"classes":[{"id":"Watcher","kind":"class","file":"…/base.py","symbol":"Watcher", …}, …]}
```

That is deliberate for curated diagrams — `curated-diagrams.md` §8 lists inferred scope as out of
scope, because choosing "the 8 that matter" is the judgement the mode preserves. But it makes the
manifest unable to express "every class under this path", and a consumer who wants that has to
leave `vizzle doc` behind and write their own regenerate-and-diff wrapper.

One did. It is 227 lines of Python, and every consumer who wants a path-scoped gate will write
the same thing.

**The tell:** a hand-listed scope cannot catch an *addition*. A new class is absent from the
manifest, therefore absent from the diagram, therefore the check reports current. The consumer
documented this as a known hole in their own gate.

## 2. Decision A: `--group-by {none,module,component}`

Replace `RenderOptions.group_by_module: bool` with a `Grouping` enum. `--group` becomes a
deprecated alias for `--group-by module`, so existing invocations and manifests keep working.

Component grouping reuses the detector in `component.rs` rather than re-deriving ownership: the
module→component map it already builds for class counts is exactly the lookup the renderer needs.

**Why not make component the default:** at a single-package scope every class lands in one
namespace, which is strictly worse than module grouping. The right default depends on the shape
of the scope, which only the caller knows.

Measured after implementing, on a 33-package Python tree:

| scope | `--group-by module` | `--group-by component` |
|---|--:|--:|
| whole tree (33 packages) | 533 namespaces | **34 namespaces** |
| one package (29 files) | **29 namespaces** | 1 namespace |

So the flag earns its keep in both directions, and neither value is the right
default.

## 3. Decision B: a `scope` key in the `gen:c4-code` manifest

```json
{"scope":{"path":"src/app/watchers","lang":"python","group":"module"},"direction":"LR"}
```

`scope` and `classes` are mutually exclusive: a manifest carrying both is an error, because it
would silently mean two different diagrams. Everything else about a managed document is
unchanged — same marker, same single fence, same "regeneration replaces the fence and nothing
else" rule, same `--check`.

This is deliberately **not** a new command. `vizzle doc` and `vizzle doc --check` are already the
check/regenerate surface and already have the ergonomics a linter needs; the change is to widen
what they can cover.

### 3.1 Why this does not reopen §8

`curated-diagrams.md` §8 rules out *inferring* scope — vizzle guessing which classes matter. A
`scope` key infers nothing: the author states the path, exactly as they state a symbol list
today. The two modes answer different questions, and both are authored:

- **curated** — "these eight classes are the story", for a design document.
- **scoped** — "everything here, kept honest", for a gate.

## 4. Decision C: a size ceiling belongs in vizzle

Mermaid stops laying out past **50,000 characters** and renders an error graphic instead of the
diagram. A generated document that crosses it fails in the one way nobody reports: it looks
rendered, and it is wrong.

`vizzle doc` therefore fails a document whose generated fence exceeds the ceiling, and warns
within a margin of it. This is a property of mermaid, not of any consumer, so it belongs here —
the consumer that discovered it had implemented the guard itself, which is the signal that it was
in the wrong place.

Measured on the tree above: a full-tree class diagram is **987,503 characters**, 20x the ceiling;
a single-package scope is 44,047. So a scoped diagram is not a nicety, it is the only thing that
renders, and a consumer needs to be told *before* they commit one that does not.

## 5. CLI surface

```sh
# unchanged
vizzle doc <doc.md>...
vizzle doc --dir <path>
vizzle doc --dir <path> --check

# new
vizzle class <path> --group-by component
vizzle class <path> --group-by module     # == the old --group
vizzle class <path> --group-by none       # == the default
```

Exit codes for `doc`, which is what a lint chain classifies on:

| | |
|---|---|
| 0 | every managed document is current |
| 1 | at least one is stale, malformed, or past the mermaid ceiling |
| 2 | vizzle could not produce an answer (Click usage error, unreadable tree) |

That split is what lets a caller fail open on an environment failure while still blocking on real
drift. It is already the de-facto behaviour; §5 states it so a consumer can rely on it.

## 6. Out of scope

- **Choosing a scope for the caller.** Unchanged from §8: a heuristic that guessed which subtree
  to diagram would remove the reason to use the mode.
- **Splitting an oversized diagram automatically.** vizzle reports the ceiling; where to cut is a
  modelling decision.
- **Component grouping for `vizzle component`'s own mermaid.** That renderer draws component
  boxes, not classes. Decision A covers the class diagram; a component diagram that also drew
  every class would be a third diagram type, and needs its own spec.
