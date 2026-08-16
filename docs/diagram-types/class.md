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

Every named type the parsers recognise: classes, interfaces, enums, and
dataclasses.

| Attribute | Meaning |
|---|---|
| `name` | Bare name (`AgentRunner`); nested classes read `Outer.Inner` |
| `qualified` | Unique key, `<module>.<name>` |
| `module` | Dotted path derived from the file path |
| `annotation` | UML stereotype: `interface`, `abstract`, `enumeration`, `dataclass` |
| `members` | Fields and methods (§2.2) |
| `change` | `ChangeKind`, shared with every other diagram type |

Stereotypes are inferred from the source, not declared: a Python `Protocol` or
`@runtime_checkable` is an interface, `ABC`/`@abstractmethod` is abstract, an
`Enum` subclass is an enumeration, `@dataclass` is a dataclass; a TypeScript
`interface` and `abstract class` map directly. On h: 74 interfaces, 15
dataclasses, 1 enumeration, 112 plain classes.

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
vizzle diff <repo> [--base REV] [--head REV]          # --type class is the default
vizzle serve <repo> [--diff]
```

## 9. Out of scope

- Type inference of any kind: no symbol table, no import resolution, no
  checker. §4 is the ceiling.
- Generic parameters as first-class model elements (they are rendered as part
  of the type string, and tokenised for resolution).
- Languages beyond Python and TypeScript — see §5.4 for why Rust and C++ are
  the most interesting next ones.
