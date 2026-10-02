# Rust language specification

Status: proposed

This document fixes the Rust-to-vizzle model before an implementation exists.
It extends the language-neutral class and import graphs described by the
[class](../diagram-types/class.md) and
[component](../diagram-types/component.md) specifications. Every resolver below
inherits the repository rule that a wrong edge is worse than a missing one.

## 1. Detection and source selection

Rust source is a UTF-8 file whose path ends in `.rs`. `target/` is always
excluded, even when checked in, because it is Cargo output. All other selection
uses the existing gitignore-aware walk followed by language, include, and
exclude filters: `build.rs`, file-backed and inline `#[cfg(test)]` modules, and
files below `tests/`, `benches/`, and `examples/` are included unless an ignore
file or explicit `-E` removes them. Vizzle does not evaluate `cfg` expressions.
This matches today's selector, which admits supported suffixes before applying
globs (`crates/vizzle-core/src/walk.rs:103`) and whose fixture proves that a
`tests/fixtures` path survives without an explicit exclusion
(`crates/vizzle-core/src/walk.rs:235` and
`crates/vizzle-core/src/walk.rs:239`).

**Worked example.** For a walked crate containing `src/lib.rs`,
`src/tests.rs` declared by `#[cfg(test)] mod tests;`, `tests/api.rs`,
`benches/read.rs`, `examples/demo.rs`, `build.rs`, and `target/generated.rs`,
the selected list is the first six paths in lexicographic order. With
`-E 'tests/**'`, `tests/api.rs` alone also disappears. The inline module remains
part of `src/lib.rs`; it is not a separately selected file.

**Decision.** Detect `.rs`, unconditionally exclude `target/`, and otherwise
apply existing selection without test/build/example conventions. Change this
only if vizzle introduces a cross-language target-role filter or evaluates
build configurations.

## 2. Elements become boxes

Rust declarations use the existing `Class` element and its annotation string
(`crates/vizzle-core/src/model.rs:192`):

| Rust declaration | Box | Members |
|---|---|---|
| named struct | class (no stereotype) | named fields |
| tuple struct | class | fields named `0`, `1`, … |
| unit struct | class | none |
| enum | `<<enumeration>>` | one row per variant; tuple/record payloads retained |
| union | `<<union>>` | named fields |
| trait | `<<interface>>` | associated items from §3 |
| type alias | `<<type>>` | one row named `type` whose detail is the aliased type |

Unlike TypeScript's density rule, every Rust alias earns a box: Rust aliases
are named API items and their target is syntactically available. Anonymous
struct/union-like macro tokens and generated declarations produce no box.

**Worked example.** `Language` at `crates/vizzle-core/src/model.rs:4` produces
`vizzle_core::model::Language <<enumeration>>` with rows `Python` and
`TypeScript`. `Member` at `crates/vizzle-core/src/model.rs:100` produces the
plain class `vizzle_core::model::Member` with named field rows including
`name: String`, `returns: Option<String>`, and `type_refs: Vec<String>`.
`Files` at `crates/vizzle-core/src/lib.rs:69` produces
`vizzle_core::Files <<type>>` with `type: Vec<(String, String)>`.

**Decision.** Use the mapping table exactly; the existing annotation string is
sufficient, including `union`, so Rust adds no new graph element kind. Revisit
only if a renderer needs semantics that cannot be expressed by `Class` plus a
stereotype.

## 3. Members, impl merging, visibility, and signatures

Fields attach to their declaring struct or union; tuple fields use their
zero-based ordinal. Every inherent `impl T` whose normalized self type resolves
to the qualified key for `T` contributes its items to that box, regardless of
the impl's file or module. Strip references and fully qualified path syntax
only when resolution stays unique. An unresolved, ambiguous, blanket, generic
parameter, or other non-nominal self type contributes nothing.

A function with `self`, `&self`, `&mut self`, `self: Box<Self>`, or another
typed self receiver is an instance method. A function without a self receiver
is a static associated function. An associated constant is a static field-like
member; an associated type is a type member with its bounds/default as detail.
Trait functions are abstract when they have no body and concrete otherwise.
Items from `impl Trait for Type` are not copied onto the type: the trait box
owns the contract and the impl produces the realization edge in §4.

Visibility maps onto the model's three markers
(`crates/vizzle-core/src/model.rs:35`): `pub` is `+`; `pub(crate)`,
`pub(super)`, and `pub(in path)` are `#`; inherited/private visibility is `-`.
Trait items are `+` because callers may use them through the public trait even
though Rust omits a visibility token.

Display type parameters and non-lifetime bounds compactly (`map<T: Read>`),
but drop declared lifetimes, lifetime arguments, and lifetime-only bounds from
display and edge extraction. Preserve reference shape (`&T`, `&mut T`) and the
unabridged normalized signature/body in the fingerprint. This removes diagram
noise without hiding ownership-relevant reference shape or behavioral changes.

Member identity is `(kind, name, normalized parameter types)`. Return type,
visibility, modifiers, normalized signature, and defining source affect the
fingerprint, not identity. Overloads are therefore distinct while impl-block
location is irrelevant.

**Worked example.** `impl Language` at
`crates/vizzle-core/src/model.rs:9` merges into the `Language` box even though
the enum declaration ends at line 7. It adds static
`+from_path(path: &str): Option<Self>` and instance
`+name(&self): &'static str`. If the same impl moved to `src/language.rs`, both
members would keep their keys. `impl<T> T` or `impl Unknown` with no unique
local resolution would add nothing.

**Decision.** Merge all and only uniquely resolved inherent impls, classify
receivers as above, use the stated visibility mapping, and key members
independently of impl placement. Change lifetime display only if real diagrams
show two otherwise indistinguishable APIs whose distinction matters to readers.

## 4. Relations and type wrappers

The existing four relation kinds (`crates/vizzle-core/src/model.rs:146`) map as
follows:

| Rust evidence | Relation |
|---|---|
| `impl Trait for Type` | `Type ..|> Trait` realization |
| `trait Child: Parent` | `Child --|> Parent` generalization |
| a struct/union field type | association from holder to resolved type |
| a function/method parameter or return type | dependency from owning box to resolved type |

Type traversal sees through references, raw pointers, slices, arrays, tuples,
and `Box`, `Rc`, `Arc`, `Vec`, `Option`, `Result`, `HashMap`, and `BTreeMap`.
It emits candidates for resolvable named arguments, never primitives or the
wrapper itself. Existing strongest-edge and one-edge-per-pair rules still
apply. `#[derive(...)]` creates no stereotype, member, or relation: expansion
is absent and a derive name is not evidence that a local trait resolves.
Generic-only, associated-type-only, ambiguous, and unresolved names produce no
edge. With `--externals`, an unambiguously named external trait/base may use the
existing external stub behavior; unresolved member types still produce none.

**Worked example.** `Member` has `returns: Option<String>` and
`type_refs: Vec<String>` at `crates/vizzle-core/src/model.rs:105`; traversal
sees through both wrappers, but `String` is external/standard, so these fields
produce no internal edge. `Class` has `members: Vec<Member>` at
`crates/vizzle-core/src/model.rs:207`, producing the association
`vizzle_core::model::Class --> vizzle_core::model::Member`. Its
`drawn_members(&self) -> impl Iterator<Item = &Member>` signature at
`crates/vizzle-core/src/model.rs:229` also mentions `Member`, but the single
strongest edge remains the association. The unresolved `Iterator::Item`
projection creates no edge.

**Decision.** Apply the table and wrapper traversal conservatively; derives
are invisible and uncertainty resolves to nothing. Add a wrapper only when it
is demonstrably a transparent/container type rather than a domain type.

## 5. Module tree, imports, re-exports, and qualified names

Build a crate module tree from declarations, not directory inference alone.
An inline `mod foo {}` owns inline children. A file-backed `mod foo;` searches
Rust's `foo.rs` and `foo/mod.rs` forms relative to the declaring module, with
children below `foo/`; finding both or neither is unresolved. A static string
`#[path = "..."]` is followed only when its normalized path stays inside the
walk root. `lib.rs` and `main.rs` are crate roots, not `lib`/`main` segments.
Separate `src/bin/*` roots receive stable `bin::<target>` segments to prevent
collisions.

Resolve `crate::`, `self::`, and repeated `super::` lexically. Explicit `use`
bindings and aliases enter the module scope. `pub use` additionally creates a
re-exported alias that downstream paths may resolve. Expand a glob only when
its local target module and exported symbol set are known and exactly one
candidate results; otherwise it contributes no binding. A leading external
crate segment stays external. Cargo package names use Rust identifier spelling
(`-` becomes `_`). A type key is
`<crate-identifier>::<module-path>::<Type>`, omitting an empty module path.

**Worked example.** This crate's `lib.rs` declares `pub mod model;` at
`crates/vizzle-core/src/lib.rs:16`, resolving to
`crates/vizzle-core/src/model.rs`; `pub use model::ChangeCounts;` at
`crates/vizzle-core/src/lib.rs:29` re-exports that type. The exact defining key
is `vizzle_core::model::ChangeCounts`, while the crate-root alias resolves to it
and does not create a second `vizzle_core::ChangeCounts` box. A hypothetical
`use missing::*;` in that file would contribute no binding because its module
cannot be resolved.

**Decision.** Use the declaration-built module tree and lexical/import rules
above; qualified identity follows package, module, and type rather than file.
Change this only when rustc/Cargo metadata becomes an explicit resolution
source.

## 6. Macros are not expanded

Tree-sitter exposes macro definitions and invocations as syntax but does not
expand them. `macro_rules!`-generated declarations, function-like proc-macro
output, attribute-macro output, and derive output are therefore absent. Vizzle
extracts only literal declarations and items present in the parsed source.

**Worked example.** `#[pyfunction]` annotates the literal
`class_diagram_from_dir` function at `crates/vizzle-py/src/lib.rs:52`; the
function beginning at `crates/vizzle-py/src/lib.rs:70` remains syntactically
visible, but wrapper functions and registration machinery generated by PyO3 do
not. Likewise `#[pymodule]` at `crates/vizzle-py/src/lib.rs:492` does not create
an extra box. If a future `#[pyclass] struct X` and `#[pymethods] impl X` are
literal in this crate, `X` and the written methods are visible; PyO3-generated
members are absent.

**Decision.** Never infer macro output in v1. Change this only by adding an
explicit expansion input whose build, feature, and security costs are specified.

## 7. Cargo components and crate edges

`Cargo.toml` already marks a component (`crates/vizzle-core/src/component.rs:24`).
The detector deliberately skips a root manifest
(`crates/vizzle-core/src/component.rs:299`) and drops manifest components with
no selected parseable files (`crates/vizzle-core/src/component.rs:408`). A
`[workspace]` root without `[package]` is therefore grouping metadata, never a
component. Each member package with selected `.rs` files is named from
`[package].name`.

An internal crate edge is supported by either (a) a resolved Rust path whose
first external-looking segment matches another workspace crate identifier, or
(b) a normal, dev, or build dependency whose `path` resolves to another
detected package. Renamed dependencies use the dependency key in source and
the optional `package` value for the target; hyphens normalize to underscores
for source matching. Registry/git dependencies do not create internal nodes or
edges by default.

Both evidence sources merge into one edge. Its weight remains the number of
distinct importing source files, as the current model defines
(`crates/vizzle-core/src/component.rs:70`); a path dependency with no observed
import has weight `0`. The edge still renders without `--weights`, preserving
declared architecture while truthfully reporting no importing file. Record
both provenance flags internally so a later exporter can explain the edge.
Ambiguous package/path matching produces nothing.

**Worked example.** The root workspace lists `crates/vizzle-core` and
`crates/vizzle-py` at `Cargo.toml:3`. Their package names are declared at
`crates/vizzle-core/Cargo.toml:2` and `crates/vizzle-py/Cargo.toml:2`, and
`vizzle-py` declares `vizzle-core = { path = "../vizzle-core" }` at
`crates/vizzle-py/Cargo.toml:18`. The output contains components `vizzle-core`
and `vizzle-py`, not a root component, and exactly one internal edge
`vizzle-py → vizzle-core`; its manifest evidence exists even if all Rust
imports are filtered out, in which case its weight is `0`.

**Decision.** Combine resolved imports and local path dependencies, render
manifest-only edges at weight zero, and never turn a virtual workspace root or
registry dependency into an internal component. Reconsider only if users need
a switch between declared and observed dependency views.

## 8. Rust diff identity and fingerprints

A type is keyed by the qualified name from §5, not its file. A merged member
is keyed by `(kind, name, normalized parameter types)` from §3. Member
fingerprints include return type, modifiers, visibility, normalized signature,
and defining source/body, but not impl-block path, location, or order. Sort
fields and merged impl members by their keys before class fingerprinting. This
extends the existing model, which sorts member fingerprints
(`crates/vizzle-core/src/model.rs:236`) and diffs classes by `qualified`
(`crates/vizzle-core/src/diff.rs:15`). A module/type rename is remove+add.

**Worked example.** Moving `Language::name` from the impl beginning at
`crates/vizzle-core/src/model.rs:9` into another `impl Language` in
`language_display.rs` leaves the type and member keys/fingerprints unchanged:
no diff. Changing its signature from `name(&self)` to `name(&mut self)` changes
the normalized parameter type, so the old member is removed and the new one
added; changing only its body preserves the key and marks the member modified.

**Decision.** Make impl placement irrelevant and behavior/signature relevant.
Change the key only if Rust permits two legal members it would collapse.

## 9. Grammar and parser binding

The implementation will add `tree-sitter-rust` from the `0.24.x` family and
load its `LANGUAGE` constant with `parser.set_language(&LANGUAGE.into())`.
That family uses `tree-sitter-language = "0.1"` and its language-function
bridge is compatible with this repository's `tree-sitter = "0.26"` pin at
`crates/vizzle-core/Cargo.toml:16`. The upstream crate documents this exact
`LANGUAGE.into()` binding pattern.

**Decision.** Name `tree-sitter-rust = "0.24"` for the implementation PR; this
documentation PR adds no dependency or `Cargo.toml` change. Re-evaluate the
family only if the existing tree-sitter pin changes or the selected binding
fails a minimal parser compatibility test.

## 10. Acceptance criteria for the implementation PR

### 10.1 This repository

Measured on 2026-10-02 at `fb07ec6de387843eea6a34a4033db34d9c2a5db8`:

```sh
git grep -h -E '^\s*(pub(\([^)]*\))? )?struct ' -- 'crates/*.rs' | wc -l  # 26
git grep -h -E '^\s*(pub(\([^)]*\))? )?enum ' -- 'crates/*.rs' | wc -l    # 9
git grep -h -E '^\s*(pub(\([^)]*\))? )?trait ' -- 'crates/*.rs' | wc -l   # 0
git grep -h -E '^\s*impl\b' -- 'crates/*.rs' | wc -l                     # 20
sed -n '1,3p' Cargo.toml
sed -n '15,18p' crates/vizzle-py/Cargo.toml
```

The future implementation must satisfy:

```sh
uv run vizzle class . -l rust -o /tmp/vizzle-rust-class.mmd
uv run vizzle component . -l rust -o /tmp/vizzle-rust-components.mmd
```

The class output contains boxes from both crates, including
`vizzle_core::model::Language <<enumeration>>`,
`vizzle_core::model::Member`, and merged `Language::from_path` and
`Language::name` members. It contains 26 struct boxes and 9 enum boxes (subject
only to an explicitly documented parser-error exclusion), and no trait box.
The component output contains `vizzle-core` and `vizzle-py`, omits a workspace
root component, and contains `vizzle-py → vizzle-core`.

### 10.2 Serde, which exercises traits

Use public repository `serde-rs/serde`, tag `v1.0.228`, exact revision
`a866b336f14aa57a07f0d0be9f8762746e64ecb4`:

```sh
git clone https://github.com/serde-rs/serde.git /tmp/serde-v1.0.228
git -C /tmp/serde-v1.0.228 checkout a866b336f14aa57a07f0d0be9f8762746e64ecb4
uv run vizzle class /tmp/serde-v1.0.228/serde_core -l rust \
  -o /tmp/serde-rust-class.mmd
```

The output contains `serde_core::ser::Serializer <<interface>>` with associated
type rows including `Ok` and `Error` and method rows including
`serialize_bool`; it contains the class `serde_core::ser::Impossible`; and it
contains realization edges from `Impossible` to each source-visible
serialization trait it implements, including `SerializeSeq`, `SerializeMap`,
and `SerializeStruct`. It must not invent an `Impossible ..|> Serializer` edge:
that impl does not exist. Items generated only by macros are absent and do not
create invented realization edges. These are observable text assertions over
the Mermaid output, not visual judgment.

**Decision.** Implementation is accepted only when both command sets produce
the stated boxes, members, and edges at the pinned revisions. Update counts or
the external revision only with a re-measurement recorded in this section.

## 11. Out of scope for v1

- macro expansion of any kind;
- Cargo feature or `cfg` evaluation;
- rustc/type-checker name, trait, or associated-type resolution;
- build-script execution and generated source;
- call graphs;
- ownership, aggregation, or composition inference beyond existing relations;
- indexing external-crate source;
- full Cargo target semantics, including target-specific dependency activation;
- method-call resolution, blanket-impl expansion, and monomorphization;
- parsing Cargo metadata beyond the local workspace/package/path-dependency
  facts required by §7.

**Decision.** V1 is a deterministic, conservative syntax-and-local-manifest
model. Any item above needs its own evidence, security/cost analysis, and spec
amendment before implementation.
