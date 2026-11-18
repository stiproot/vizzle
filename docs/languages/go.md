# Go language specification

Status: draft for review, 2026-11-18. §12 holds the open decisions, each with a
recommendation.

This document fixes the Go-to-vizzle model before an implementation exists, as
[`rust.md`](rust.md) did for Rust. It extends the language-neutral class and
import graphs of the [class](../diagram-types/class.md) and
[component](../diagram-types/component.md) specifications, and inherits the
repository rule that **a wrong edge is worse than a missing one**.

Why now: vizzle finds `go.mod` today (`MANIFEST_NAMES`) but parses no `.go`
file, so a Go repository draws empty, and the skills tell agents so. The
operator asked for Go support on 2026-11-18. The first real target is h's own
kernel, which is Go.

## 1. Detection and source selection

Go source is a UTF-8 file whose path ends in `.go`. Always excluded, even when
checked in, because the Go tool never builds them as part of the module:
`vendor/`, any `testdata/` directory, and directories whose name starts with
`_` or `.`. Everything else uses the existing gitignore-aware walk and the
language, include and exclude filters.

**Test files.** `*_test.go` is how Go names a test file: the convention is
enforced by the toolchain, so it is exact, not a guess. It joins the default
test globs (`lib.rs` `DEFAULT_TEST_GLOBS`), so tests are excluded by default and
`--tests` brings them back, exactly as for Python and TypeScript. External test
packages (`package store_test`) are test files by the same rule.

**Not evaluated:** build constraints (`//go:build linux`, `_linux.go`
suffixes) and cgo. Every selected file is parsed, so two files that define the
same name under different constraints both contribute (§9 says how identity
copes).

**Generated files** (`// Code generated … DO NOT EDIT.`) are included: they are
real API (protobuf messages, mocks), and h's kernel has two. Excluding them is
`-E`.

**Decision.** `Language::from_path` maps `.go` → `Language::Go`, selector name
`go`; the CLI's `SUPPORTED_SUFFIXES` (`git.py`, which names its twin) gains
`.go`, and `-l` gains `go`. Change only if vizzle starts evaluating build
constraints.

**Worked example.** In a module with `store/store.go`, `store/store_test.go`,
`store/store_linux.go`, `store/testdata/fixture.go`, `vendor/x/y.go` and
`internal/_scratch/a.go`, the default selection is `store/store.go` and
`store/store_linux.go`; `--tests` adds `store/store_test.go`.

## 2. Packages, modules and qualified names

A **package** is a directory of selected non-test `.go` files sharing one
`package` clause. A **module** is the tree under a `go.mod`, named by its
`module` line. A package's **import path** is the module path joined with the
package directory relative to the module root: for h,
`github.com/h/kernel` + `lease` = `github.com/h/kernel/lease`.

A type's `qualified` name is `<import path>.<Name>`, the way Go itself prints
it (`github.com/h/kernel/lease.Lease`). `module` is the import path. A package
outside any `go.mod` (rare; GOPATH-era trees) uses its directory path as the
import path.

Because the qualified name needs the module path, Go parsing needs the
manifests, the same way Rust's crate names do. The class diff already collects
manifests when a Rust file changed (`cli.py` `_diff_manifests`); that test
becomes "a Rust or Go file changed".

A directory with files from two packages (only `package x` and `package x_test`
is legal, and tests are excluded by default) keeps the non-test package.
`package main` is a package like any other; §7 marks it.

## 3. Elements become boxes

| Go declaration | Box | Members |
|---|---|---|
| `type T struct{…}` | class (no stereotype) | named fields, then methods (§4) |
| `type I interface{…}` | `<<interface>>` | its methods; embedded interfaces are relations (§5), not members |
| `type T <other type>` (a defined type over a basic, slice, map or func type) | `<<type>>` | one row `type` whose detail is the underlying type, then methods |
| `type A = B` (an alias) | `<<type>>` | one row `type` whose detail is `= B` |
| defined type with a matching `const` block (§3.1) | `<<enumeration>>` | one row per constant, then methods |
| `func`, `const`, `var` at package level | the package's `<<module>>` box (class.md §2.5), behind `--modules` | one row each |

Generic type parameters stay in the type string (`Store[T any]`), as for the
other languages. Anonymous struct types inside a field or signature produce no
box.

### 3.1 Enumerations

Go has no enum keyword; the idiom is a defined type plus a `const` block of
that type, usually with `iota`. A defined type becomes `<<enumeration>>` when
one `const` declaration in its package declares two or more constants whose
explicit type is that type, or which follow such a constant in the same block
with no type or value of their own (Go's implicit repetition, the `iota`
pattern). The constants become its members, in declaration order. A constant
of that type declared elsewhere is not pulled in: an enumeration is the block
someone wrote as one.

**Worked example.** `type Color int` followed by
`const ( Red Color = iota; Green; Blue )` is `<<enumeration>> Color` with
members `Red`, `Green`, `Blue`. h's kernel has four `iota` blocks.

## 4. Members, methods and visibility

**Visibility is the first letter**: an exported name (upper case) is public
(`+`), an unexported one is private (`-`). Go has no protected.

**Fields** are `name : type`; a struct tag is dropped. An **embedded** field
(no name: `*kv.Base`, `sync.Mutex`) is not a member row: it is a relation (§5).

**Methods** are declared outside the type, anywhere in its package, with a
receiver. A method attaches to the box of its receiver's base type: `func (s
*Store[T]) Get(…)` and `func (s Store[T]) Len()` both attach to `Store`, with
no distinction drawn between pointer and value receivers (the method set rule
matters for §5.2, not for the picture). A method whose receiver type is not
declared in the same package cannot exist in valid Go; if the parser meets one
(a build-constrained twin, §1), it attaches to nothing.

Signatures render as `name(params) results`, parameter names kept as written
and a multi-value result in parentheses: `+Get(ctx context.Context, key
string) ([]byte, error)`. A curated diagram renders names only, as for the
other languages (`Params::NamesOnly`).

**Package-level functions** go to the module box only. A constructor
(`func NewStore() *Store`) is not moved into `Store`'s box: Go has no such
association, and guessing one by name is the kind of edge this repository
refuses.

## 5. Relations

### 5.1 Declared

| Go source | Edge | Notes |
|---|---|---|
| struct embeds a type (`*kv.Base`, `Base`) | `inherits` (`--|>`) | method promotion is what a reader of the diagram needs to know; the edge is labelled `embeds` so it is not read as Java inheritance |
| interface embeds an interface (`io.Closer`) | `inherits` | interface extension: the method set is a superset |
| a field's type, or a method's parameter or result type, names a box | `association` (field) / `dependency` (signature) | the existing member-type resolution (class.md §5.1), on the tokens of the type string |
| `var _ I = (*T)(nil)` or `var _ I = T{}` | `implements` | the author asserting it; always drawn |

Type references resolve through the file's imports: `kv.Base` is the type
`Base` in the package imported as `kv` (or by its last path element when there
is no alias). An unqualified name resolves within the same package. A dot
import (`import . "x"`) makes unqualified names ambiguous, so in a file with a
dot import an unqualified name that is not declared in its own package
resolves to nothing.

### 5.2 Inferred implementation (§12 D2)

Go interfaces are satisfied implicitly, and h's kernel has 50 interfaces and
**no** `var _ I = …` assertions. Without inference every interface in it draws
unconnected, which is the opposite of what a reader opens a class diagram for.

Rule: `T implements I` is drawn when all of these hold:

- `I` is an interface declared in the scanned tree (never an external one such
  as `io.Reader` or `fmt.Stringer`: their method sets are not in the graph);
- `I` has at least one method, after flattening its embedded interfaces that
  are also in the tree (an embedded external interface makes `I`'s method set
  unknown, so `I` gets no inferred edges);
- `T` is a struct or defined type in the tree, and the method set of `*T`
  (its pointer and value receiver methods) contains every method of `I` with
  the same name and the same normalized parameter and result types (§9).

Type comparison is textual after normalization (qualified through imports,
parameter names dropped), because vizzle does not type-check; two spellings of
the same type that normalization cannot equate miss the edge, which is the
safe direction.

**Worked example.** `type Reader interface { Get(ctx context.Context, key
string) ([]byte, error) }` and `func (s *Store) Get(ctx context.Context, k
string) ([]byte, error)` in the tree: `Store ..|> Reader`. If `Reader` also
embeds `io.Closer`, no edge is inferred to `Reader` at all.

## 6. Imports

An import spec gives a path and an optional name (`kv "example.com/x/kv"`,
`_ "embed"`, `. "x"`). It resolves:

1. **Internal**: the path equals, or starts with `<path>/`, a module path of a
   `go.mod` in the tree (longest match), or a `replace` directive's module whose
   target is a local directory in the tree. It resolves to that package
   directory, if the directory has selected files; otherwise to nothing.
2. **Standard library**: the first path element has no dot (`context`,
   `net/http`). Never a node, even with `--externals`: drawing `fmt` beside
   every package is noise, not architecture.
3. **External**: anything else. With `--externals` it becomes one node per
   module, found as the longest `require` path in the importing module's
   `go.mod` that prefixes the import path; with no matching `require`, the path
   itself.

Blank imports (`_`) are edges like any other: a driver registered for side
effects is a real dependency. Import aliases only matter for §5.1 resolution.

## 7. Components and edges (§12 D1)

**A component is a package**, not a `go.mod`. Go defines the package as the
unit of import, and its import paths are exact, so package-level edges need no
best-effort guessing. Treating a module as one component would draw h's kernel
(one `go.mod`, 26 packages) as a single box, the same failure a one-manifest
Python service has (which needed `--split`).

- Each package is a `«component»` box named by its import path relative to its
  module root (`lease`, `cli/render`; the module root package is `.`, named by
  the module path's last element).
- `subgraph` blocks group packages by their module (one block per `go.mod`) and
  then by parent directory, as component.md §2.3 and §3.2 already group by directory.
  A single-module repository therefore shows one module block holding its
  packages.
- `package main` gets the stereotype `«main»` in place of `«component»`:
  entry points are the first thing a reader of a Go repository looks for, and
  `cmd/*` holds seven of them in h.
- An edge `A → B` is drawn when a selected file of package `A` imports package
  `B` (§6 rule 1). Weight is the number of distinct importing files, as today.
- Across modules (a `go.work`, or `replace` to a local path), the same rule
  applies: the edge is between packages, and the module blocks show the
  boundary.
- `--split` is not needed for Go and is ignored for Go packages, with a note
  on stderr; component.md's `--split` exists to recover the package structure
  that Go already declares.

A `go.mod` with no selected Go files is no component, as today.

**Worked example.** h's kernel: one block `github.com/h/kernel` holding 26
package boxes, seven of them `«main»` under `cmd/`, with edges from the 19
internal import paths that 65 non-test files use.

## 8. `vizzle doc` (curated manifests)

A `gen:c4-code` entry addresses Go the same way as other languages: `file`
names a `.go` file, `symbol` a type in its package. Because Go types belong to
packages, not files, an entry resolves `symbol` within the package of `file`,
so methods declared in another file of the package still appear. `kind:
"module"` names a file and lists `functions`, `consts` (any listed name, as
D9 of the skill-set spec made it for Python) and package-level `var`s under
`consts`. `scope.lang` accepts `go`.

## 9. Diff identity and fingerprints

A type is keyed by its qualified name (§2). A method is keyed by its name
within its receiver's box (Go has no overloading, so the name is unique);
fields by name. Fingerprints include the normalized signature and the body
hash, so a body-only change marks the method modified and a signature change
modifies it in place (same name). Moving a method between files of a package,
or switching its receiver between `T` and `*T`, changes neither key nor
fingerprint: neither changes what the package offers its callers. A package
rename or move is remove plus add.

**Build-constrained twins** (`store_linux.go` and `store_darwin.go` both
defining `func (s *Store) lock()`): the same key twice. The members are merged
into one row whose body hash covers both definitions in path order, so a change
to either marks it modified and the diagram does not show the method twice.

## 10. Grammar and parser binding

`tree-sitter-go = "0.25"` with the repository's `tree-sitter = "0.27"`. Checked
on 2026-11-18 in a scratch crate: it loads with
`parser.set_language(&tree_sitter_go::LANGUAGE.into())` and parses, without
error nodes, a file holding every construct above. The node kinds this spec
relies on, as that grammar names them: `package_clause`, `import_spec`
(`name`, `path`), `type_spec` and `type_alias` (`name`, `type_parameters`,
`type`), `struct_type` → `field_declaration` (a nameless one is embedded),
`interface_type` → `method_elem` and `type_elem` (an embedded interface),
`method_declaration` (`receiver`, `name`, `parameters`, `result`, `body`),
`function_declaration`, `const_spec` / `var_spec`, `iota`.

## 11. Acceptance criteria for the implementation PR

### 11.1 h's kernel

Measured on h main `1866f3594` (`kernel/`, module `github.com/h/kernel`, 220
`.go` files of which 83 are tests; 26 non-test packages, 7 under `cmd/`; 381
struct declarations and 50 interfaces counted by pattern; 869 methods; 1100
functions; 1 alias; 4 `iota` blocks; 2 generated files; 0 `var _ I =`
assertions; 19 distinct internal import paths used by 65 non-test files):

1. `vizzle component kernel` draws one module block with 26 package boxes, 7 of
   them `«main»`, and only internal edges that a `grep` of the importing file
   confirms (spot-check every edge into `lease` and `ledger`).
2. `vizzle class kernel --no-members -I 'lease/**'` lists every exported and
   unexported type declared in `kernel/lease/*.go` (compare against
   `grep -E '^type '`), with methods attached when members are on.
3. Every inferred `implements` edge in `vizzle class kernel -f json` is
   correct on inspection of a sample of 10, and the count is reported in the
   PR. If more than a third of interfaces with inferred edges have more than
   five implementers, that is the D2 threshold question, raised before merge.
4. The four `iota` types draw as `<<enumeration>>` with their constants.
5. `vizzle diff kernel --base <a kernel commit> --type component` names
   packages, and a body-only change in one method draws that method `✱`.
6. With `--tests`, `*_test.go` files appear; without, the trailer says
   `tests excluded`.

### 11.2 A small external repository

`go-chi/chi` at a pinned tag: middleware and router interfaces, a
multi-package single module, external imports. `component --externals` shows
no standard-library nodes and one node per required module.

### 11.3 This repository

The plugin's skills say vizzle parses Go (the precondition in
`vizzle-diagrams` and the levels table in `explain-code`), and
`test_plugin.py` still passes. `docs/distribution.md` and `class.md` §2 gain a
line each.

## 12. Decisions (open; each with a recommendation)

| | Question | Recommendation | Why |
|---|---|---|---|
| D1 | Is a Go component a package or a module (`go.mod`)? | **A package**, grouped in a block per module (§7) | Go's unit of import is the package and its import paths are exact. A module-level component draws h's kernel as one box. The cost is a second rule for what a component is; the block per module keeps the manifest view visible |
| D2 | Infer `implements` from method sets? | **Yes, for interfaces declared in the tree with a known, non-empty method set**, exact normalized signatures (§5.2); explicit `var _ I =` assertions always drawn | h has 50 interfaces and no assertions: without inference they all float. Satisfying an interface is a fact in Go, not a guess, so the edge is true; the risk is only volume, which acceptance 3 measures before merge |
| D3 | Struct embedding: which edge? | **`inherits`, labelled `embeds`** | Method promotion is the behaviour a reader must see; association would hide it. The label stops a Java reading |
| D4 | Constructors (`NewT`) in `T`'s box? | **No**: they stay in the module box | No such association exists in Go; a name-based guess is the wrong-edge risk |
| D5 | Standard library as nodes under `--externals`? | **Never** | Every package imports `fmt`/`context`; drawing them is noise |
| D6 | `package main`: mark it? | **Yes, `«main»`** | Entry points are what a reader of a Go repository looks for first |
| D7 | Build constraints | **Not evaluated; twins merge into one member** (§9) | Evaluating them needs a target platform; merging keeps one row and still sees a change in either file |
| D8 | Generated files | **Included** | They are the real API of many Go packages (protobuf); `-E` removes them |
