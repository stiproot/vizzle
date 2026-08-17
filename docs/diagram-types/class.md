# Class diagram

**Status:** implemented, except where §5.4 marks a decision as pending.
**Command:** `vizzle class <repo>` (+ `vizzle diff`, `vizzle serve`), and the
per-component drill-down in the [component diagram](component.md#53-drill-down-the-class-diagram-inside-a-component).

The class diagram answers *"what is the shape of the code?"* — the types a
codebase defines, what they hold, and how they relate. It is the most
zoomed-in of vizzle's diagram types, and the one the component diagram renders
inside an exploded component.

This document was written after the implementation rather than before it (the
class diagram predates the spec-first practice in CLAUDE.md). It records the
model as built, and — in §5.4 — a decision about ownership notation that the
code has *not* yet acted on.

## 1. Two lenses, comprehension first

Same rule as every diagram type: **comprehension is primary**. `vizzle class`
needs no git, no base revision, no repository even — just source files.
`vizzle diff` layers a change annotation over the identical diagram, fading
unchanged elements to context so the change carries the eye. See
[component.md §1.1](component.md#11-two-lenses-comprehension-first).

## 2. Elements

### 2.1 Class

Every named type the parsers recognise: classes, interfaces, enums,
dataclasses, and the structural type aliases of §2.3.

| Attribute | Meaning |
|---|---|
| `name` | Bare name (`AgentRunner`); nested classes read `Outer.Inner` |
| `qualified` | Unique key, `<module>.<name>` |
| `module` | Dotted path derived from the file path |
| `annotation` | UML stereotype: `interface`, `abstract`, `enumeration`, `dataclass`, `type`, `union`, `schema`, `module` |
| `members` | Fields and methods (§2.2) |
| `change` | `ChangeKind`, shared with every other diagram type |

Stereotypes are inferred from the source, not declared: a Python `Protocol` or
`@runtime_checkable` is an interface, `ABC`/`@abstractmethod` is abstract, an
`Enum` subclass is an enumeration, `@dataclass` is a dataclass; a TypeScript
`interface` and `abstract class` map directly. On h: 74 interfaces, 15
dataclasses, 1 enumeration, 112 plain classes.

**One element type, more stereotypes.** §2.3, §2.4 and §2.5 add new *kinds* of box,
not new kinds of element: they are `Class` values carrying a different
`annotation`. That is deliberate — the relation extraction (§5), the diff
(§7), `export::class_json`, and both renderers then work on them unchanged,
and a diff still keys on `qualified`. A second element type would have meant
touching all of that twice.

### 2.2 Member

| Attribute | Meaning |
|---|---|
| `name` | Member name |
| `visibility` | `+` public, `#` protected, `-` private |
| `detail` | A field's type, or a method's parameter list (`cfg: Config, retries`) |
| `returns` | A method's return type |
| `type_refs` | Type expressions this member mentions — the raw material for §5 |
| `is_method` / `is_static` / `is_abstract` | Rendered as UML classifiers `$` and `*` |

Visibility comes from TypeScript accessibility modifiers and `#private`
fields, and from Python's `_protected` / `__private` naming convention.
Fields include TypeScript constructor parameter properties and Python
attributes assigned to `self` in `__init__`; `@property` reads as a field, not
a method. Dunder methods are skipped as noise.

### 2.3 Decision: which type aliases earn a box

**Decided and implemented 2026-08-17.** A TypeScript `type` declaration is a named type, so
§2.1 always implied it belonged here — but drawing all of them would drown
the diagram, so the question is which.

**What was measured** — the 183 exported `type` aliases in h:

| Shape | Count | Drawn? |
|---|---|---|
| `type X = { … }` — object literal with members | **61** | yes, `<<type>>` |
| `type X = A \| B \| C` — any union | **18** | yes, `<<union>>` |
| references a named type but has no drawable structure | ~100 | no |
| opens neither a literal nor a union | rest | no |

**What was chosen.** A type alias earns a box when it carries structure this
parser can actually see:

1. **An object literal** gets a `<<type>>` box with its fields as members. This
   is the load-bearing case: `type Config = { … }` is an `interface Config`
   in all but keyword, and h has 61 of them against 73 interfaces. Drawing one
   and not the other is a distinction no reader cares about, and omitting them
   was the single biggest hole in the class model.
2. **Every union** gets a `<<union>>` box with its arms as members — including
   `StopReason = "completed" | "timeout"`, which is an enumeration in all but
   keyword. **But only a *named* arm becomes an edge.** Measured on h: drawing
   every union costs 22 boxes, while pointing a dependency at every arm would
   add 83 edges to literals and primitives. A box is cheap; a wrong edge
   violates §4.
3. **Everything else is skipped.** `type Env = Effect<A, B, C>`, mapped types,
   conditionals, `keyof` — resolving those needs a type checker, and §9 says
   we do not have one. A box with a truncated type string in it is noise.

Case 3 is the only exclusion, and it is a limit of the parser rather than a
density judgment — which matters, because `curated-diagrams.md` §4.1 requires
that anything a manifest can *name*, vizzle can *find*.

**What it produced.** 139 new boxes on h — 100 `<<type>>` and 39 `<<union>>` —
taking the class diagram from 213 boxes to 352 and its relations from 156 to
277. More than the table predicts, because the table counted only `export`ed
aliases while the parser draws unexported ones too, exactly as it already does
for interfaces.

**What would change it.** A JSON input path fed by the TypeScript compiler
(§9) would resolve cases 3 exactly, at which point the selection rule can
loosen. Until then, guessing at them would violate §4's stance: a wrong
element is worse than a missing one.

**Python** has no equivalent worth drawing today: h has **zero** `TypeAlias`
or `type X = …` declarations across 124 files. The parser accepts them for
symmetry when they are object-shaped, and nothing more.

### 2.4 Effect Schema structs

**Implemented 2026-08-17.** `export const X = Schema.Struct({ … })` declares a
named type with fields, so it draws as one — stereotype `schema`, fields typed
by reading the combinator (`Schema.optional(Schema.String)` → `string?`).

This is the parser's **only framework-specific branch**, and the exception is
argued in `curated-diagrams.md` §4.2 rather than here: 73 declarations in h, and
without it a real curated document cannot be regenerated. On h it adds 75 boxes,
taking the class diagram to 427.

### 2.5 Module-level functions, and why they are opt-in

**Implemented 2026-08-17.**

Neither language puts everything in a class, and h is the proof: **143
exported TypeScript functions across 80 files, and 152 public Python
module-level functions across 44 files**, against 213 classes. A class diagram
of a functional codebase that shows none of them is describing a minority of
the code.

They are modelled as **one `<<module>>` box per module**, its public
module-level functions, exported typed consts (TypeScript) and
UPPER_SNAKE module constants (Python) as members — not one box per function, which would add
295 boxes rather than 177, and would say nothing about which file a function
lives in.

Measured on h once built: **177 module boxes**, taking the diagram from 330
classes to 507 and the Mermaid from 111 KB to 201 KB.

**Opt-in, behind `--modules`.** Unlike §2.3 and §2.4, these are not named types, and
177 extra boxes changes every existing diagram and nearly doubles what an
agent pays to read one (§2.6 of `docs/distribution.md`). The
default stays "named types"; `--modules` says "and the functions too".

## 3. Extraction

tree-sitter parses each file into an AST; one extractor per language walks it.
Nothing here evaluates a build system or a type checker — this is a
best-effort syntactic model, and §4 says what that costs.

Both extractors also record the file's imports, which the component diagram
consumes; a class graph and an import graph come out of one parse.

## 4. Resolution, and its deliberate limits

A type named in source is resolved to a class in the graph by looking in the
same module first, then for a globally unique name. **Ambiguous names resolve
to nothing.** vizzle has no symbol table and no imports-to-definitions map, so a
name matching three classes could point at any of them.

> The standing rule for every resolver in this codebase: **a wrong edge is
> worse than a missing one.** A missing edge understates the coupling; a wrong
> edge sends the reader to the wrong file.

Unresolved base types are rendered as `«external»` stubs under `--externals`;
unresolved member types produce no edge at all. On h, 92 of 151 relations
point at external types (`Error`, framework base classes) — visible only when
you ask for them.

## 5. Relations

### 5.1 What is drawn today

| Kind | Derived from | Mermaid | Line |
|---|---|---|---|
| Inheritance | `extends`, Python base classes | `--\|>` | solid, hollow head |
| Implements | `implements`, `Protocol` | `..\|>` | dashed, hollow head |
| Association | A **field's** type names another class | `-->` | solid, open head |
| Dependency | A **method signature's** parameter or return type names another class | `..>` | dashed, open head |

Association and dependency are derived from `Member.type_refs`: every type
expression a member mentions is tokenised (so `Map~string, Repo~` yields
`Repo`) and resolved by §4.

Two rules keep the graph honest rather than dense:

1. **One edge per pair**, keeping the strongest kind. A class holding three
   fields of one type gets one arrow, and a type both held and passed reads as
   an association.
2. **Inheritance wins.** A subclass that also holds a field of its base
   renders as inheritance only.

On h this yields 9 inheritance, 1 implements, 24 association, 25 dependency
internal edges across 202 classes.

### 5.2 Why the graph is sparser than you might expect

h defines 202 classes and only 10 inheritance relations between them. That is
not a parser failure — it is what a TypeScript/Python codebase built from
interfaces and small services actually looks like. Member-derived edges are
what make the diagram informative there, which is why they exist.

### 5.3 Not drawn

- **Multiplicity** (`1..*`, `0..1`). A field's cardinality is often
  syntactically visible (`Foo[]`, `List[Foo]`) but the interesting half —
  whether a collection is required, bounded, or keyed — is not.
- **Relation labels** (role names). Available in principle from the field
  name; omitted because a label on every edge costs more legibility than it
  buys at 200 classes.
- **Call graphs.** vizzle draws structure, not behaviour.

### 5.4 Decision: aggregation and composition diamonds

**Status: decided, not yet implemented.**

UML distinguishes a plain association from **aggregation** (hollow diamond at
the owner: "has-a", shared, independent lifetime) and **composition** (filled
diamond: the owner controls the object's lifetime). The question is whether
vizzle can put a diamond on an edge honestly.

**The rendering is not the problem.** Mermaid has native syntax (`*--`, `o--`),
the d3 view needs a diamond marker at the container end, and the model needs
two more `RelationKind` variants. That is an hour of work.

**Detection is the problem, and it splits by language.**

- Languages that encode ownership *in the type system* — Rust's `Foo` versus
  `&Foo`/`Rc<Foo>`, C++'s by-value member versus `unique_ptr` versus
  `shared_ptr`/raw pointer — make the distinction **derivable**. The compiler
  already knows; vizzle would just read it. **vizzle parses neither language
  today.**
- TypeScript and Python — the two vizzle does parse — hold every field by
  reference. **No syntax distinguishes owning from borrowing.** The only
  available signal is behavioural: does the owner *construct* the object
  (`this.x = new Foo()`, `self.x = Foo()`, a dataclass `field(default_factory=Foo)`)
  or merely *hold* one it was given?

**Evidence.** Measured across h: **3** fields constructed into an owner (1
TypeScript, 2 Python) and **0** constructor-injected class-typed fields, out of
202 classes. h is interface-dominated, and interfaces have no constructors and
no ownership semantics at all. A pure inference approach would draw three
diamonds in the whole repository.

**Decision.** Adopt the common modelling **convention**, and be explicit in the
UI and the docs that it is a convention rather than an inference:

| Notation | Rule | Basis |
|---|---|---|
| **Composition** (filled diamond) | The owner constructs the object into its own field | Direct evidence in the AST |
| **Aggregation** (hollow diamond) | A field holds another class, with no construction evidence | Convention: a held reference is a "has-a" |
| **Dependency** (dashed arrow) | The type appears only in a method signature | Direct evidence |
| **Association** (plain arrow) | Retained as the neutral fallback for a structural relation a future extractor can see but cannot classify | — |

Rationale: the filled diamond — the strong claim, that this object's lifetime
is controlled here — is only ever drawn from direct construction evidence, so
it never over-claims. The hollow diamond restates a fact we already have (this
class holds that one) in the notation a UML reader expects, which costs nothing
in accuracy. This lights up roughly 24 edges on h instead of 3.

**Consequences.**

- The spec, the legend, and the `--help` text must call aggregation a
  convention. If a reader believes vizzle *inferred* shared ownership, the
  diagram has lied to them.
- Composition requires new extraction: constructor bodies and field
  initialisers (`this.x = new T()`, `self.x = T()` in `__init__`/`__post_init__`,
  `field(default_factory=T)`). The parsers already walk `self` assignments for
  field discovery but discard the right-hand side.
- `RelationKind` ordering must keep composition stronger than aggregation for
  the one-edge-per-pair rule in §5.1.

**What would change this decision.** Adding Rust or C++ support. There
ownership is derivable rather than conventional, and both diamonds could be
drawn from evidence — at which point the convention should be reconsidered, and
possibly confined to the languages that need it. That is the version where
diamonds genuinely earn their place.

## 6. Rendering

**Mermaid** (`classDiagram`): boxes with a stereotype line and member rows,
`--group` wrapping modules in `namespace` blocks. Mermaid 11 quirk, learned the
hard way: `classDef` statements only apply when they appear *after* the
`cssClass` attachments, so the renderer emits them last.

**Interactive HTML**: a force layout with weak per-module gravity, ticked
synchronously so the page opens settled, with zoom, pan, drag, a filter box,
and a viewport that survives live-reload. The box itself is drawn by
`viz-core.js` (`classBoxLayout` / `drawClassBox`) — one renderer, shared with
the component diagram's exploded view, so a class looks the same wherever you
meet it.

Change colors come from vizzle-core's palette in both formats (see
[component.md §9.1](component.md#91-implementation-notes)).

## 7. Diff semantics

| Element | Added | Removed | Modified |
|---|---|---|---|
| Class | only at head | only at base | same key, different fingerprint |
| Member | new name | gone at head | same name, changed signature |

Classes are keyed by qualified name and fingerprinted over their members,
bases, stereotype and language; members are fingerprinted over their signature.
Removed members are re-attached to the class so the diagram can show them
struck through. Unchanged classes in touched files render as context.

`vizzle diff` parses only the files git reports as changed — unlike the
component diff, which needs both revisions in full.

## 8. CLI surface

```sh
vizzle class <repo> [-o out.mmd|out.html] [-I glob] [-E glob] [-l python|typescript]
                   [--no-members] [--group] [--externals] [--direction LR] [--title]
                   [--modules]                        # §2.5, off by default
vizzle diff <repo> [--base REV] [--head REV]          # --type class is the default
vizzle serve <repo> [--diff]
```

## 9. Out of scope

- Type inference of any kind: no symbol table, no import resolution, no
  checker. §4 is the ceiling. This is what bounds §2.3 to type aliases whose
  structure is *syntactically* visible.
- Generic parameters as first-class model elements (they are rendered as part
  of the type string, and tokenised for resolution).
- Languages beyond Python and TypeScript — see §5.4 for why Rust and C++ are
  the most interesting next ones.
- **A JSON input path.** Accepting a graph built elsewhere — by the TypeScript
  compiler API, which resolves everything §2.3 case 3 skips — is the obvious
  way past the ceiling above, and is deliberately not built. A format you
  *accept* is a promise to everyone who generates it, and there is no second
  producer yet to shape it; designing one now means guessing. Revisit when a
  real consumer is pushing against the limit.
