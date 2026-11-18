# Go language specification

Status: draft for review, 2026-11-18; revised the same day after an independent
review, which type-checked h's kernel with `go/types` to test §5 and found the
spec leaning on per-file, name-based machinery that is wrong for Go (§4.1,
§5.1). §12 holds the open decisions, each with a recommendation.

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
checked in, because the Go tool never builds them: anything under `vendor/` or
a `testdata/` directory, and any file or directory whose name starts with `_`
or `.`. Everything else uses the existing gitignore-aware walk and the
language, include and exclude filters.

**Test files.** `*_test.go` is how Go names a test file: the toolchain enforces
it, so it is exact, not a guess. It joins the default test globs (`lib.rs`
`DEFAULT_TEST_GLOBS`), so tests are excluded by default and `--tests` brings
them back, as for Python and TypeScript. External test packages (`package
store_test`) are test files by the same rule.

**Build constraints are not evaluated** (`//go:build linux`, `_linux.go`), with
one exception: a file whose constraint is exactly `//go:build ignore` is
skipped. That tag means "never part of this package" (it marks `go run`
scripts sitting in a library directory, often `package main`), and keeping it
would put two packages in one directory. Every other constrained file is
parsed; §4.1 says how same-named declarations from twins merge.

**Generated files** (`// Code generated … DO NOT EDIT.` before the package
clause) are included: they are real API (protobuf messages, mocks). Excluding
them is `-E`.

**Decision.** `Language::from_path` maps `.go` → `Language::Go`, selector name
`go`; the CLI's `SUPPORTED_SUFFIXES` (`git.py`, which names its twin) gains
`.go`, `-l` gains `go`, and the class diff's manifest collection
(`cli.py` `_diff_manifests`, which today fires on a changed `.rs` file) fires on
a changed `.go` file too, because Go names need `go.mod` (§2).

**Worked example.** In a module with `store/store.go`, `store/store_test.go`,
`store/store_linux.go`, `store/gen.go` (`//go:build ignore`),
`store/testdata/fixture.go`, `store/_old.go` and `vendor/x/y.go`, the default
selection is `store/store.go` and `store/store_linux.go`; `--tests` adds
`store/store_test.go`.

## 2. Packages, modules and names

A **package** is a directory of selected non-test `.go` files. A **module** is
the tree under a `go.mod`, named by its `module` line. A package's **import
path** is the module path joined with the package directory relative to the
module root: `github.com/h/kernel` + `lease` = `github.com/h/kernel/lease`. A
package outside any `go.mod` uses its directory path as its import path.

- A type's `qualified` name is `<import path>.<Name>`, the way Go prints it
  (`github.com/h/kernel/lease.Lease`).
- `module` (in the model and in `-f json`) is the import path.
- The package's `<<module>>` box is keyed by the import path and **named by the
  last path element** (`lease`). The shared `push_module_box` names a box by the
  last dotted segment (`parse/mod.rs`), which would make
  `github.com/h/kernel/lease` read `com/h/kernel/lease`; Go gets its own rule.
- If one directory's selected non-test files still declare two package names
  (only possible with constraints vizzle does not evaluate), the package whose
  name matches the directory wins, else the first in path order, and the files
  of the other package are dropped with a note on stderr.

Go parsing needs the manifests, as Rust's crate names do: the parse entry
points that take `manifests` (`parse_files_with_manifests`) are the ones Go
uses.

## 3. Elements become boxes

| Go declaration | Box | Members |
|---|---|---|
| `type T struct{…}` | class (no stereotype) | named fields, then methods (§4) |
| `type I interface{…}` with only methods and embedded interfaces | `<<interface>>` | its methods; embedded interfaces are relations (§5) |
| `type C interface{…}` with a type element (`~int \| ~float64`, or a bare non-interface type) | `<<constraint>>` | its methods, and one row `type` listing the type set |
| `type T <other type>` (a defined type over a basic, slice, map, func, or named type) | `<<type>>` | one row `type` whose detail is the underlying type, then methods |
| `type A = B` (an alias) | `<<type>>` | one row `type` whose detail is `= B` |
| a defined type with a matching `const` block (§3.1) | `<<enumeration>>` | one row per constant, then methods |
| package-level `func`, `const`, `var` | the package's `<<module>>` box (class.md §2.5), behind `--modules` | one row each |

- Both forms count: `type T …` and a grouped `type ( … )` block.
- Function-local types produce no box: they are not package surface (h has 7).
- Anonymous struct and interface types produce no box; a field of that type
  renders its type as `struct{…}` / `interface{…}` (h has 9 such fields).
- A function-typed field is an ordinary field whose type is the `func(…)`
  signature (h has 89).
- Type parameters stay in the type string and render with mermaid's `~`
  generics (`Store~K, V~`), as the other languages do.

### 3.1 Enumerations

Go has no enum keyword; the idiom is a defined type plus a `const` block of
that type, usually with `iota`. A defined type becomes `<<enumeration>>` when
one `const` declaration in its package declares two or more constants whose
explicit type is that type, counting those that follow such a constant in the
same block with no type or value of their own (Go's implicit repetition, the
`iota` pattern). The constants are its members in declaration order; blank
(`_`) constants are skipped. An untyped `iota` block is not an enumeration: it
names no type to attach to. A constant of the type declared elsewhere is not
pulled in: an enumeration is the block someone wrote as one.

**Worked example.** `type Color int` followed by `const ( Red Color = iota;
Green; Blue )` is `<<enumeration>> Color` with members `Red`, `Green`, `Blue`.
`const ( secondField = iota; minuteField )` (h's `cm/schedule.go:50`) is not.

## 4. Members, methods and visibility

**Visibility is the first letter**: an exported name is public (`+`), an
unexported one private (`-`). Go has no protected.

**Fields** are `name : type`; a struct tag is dropped. `a, b int` is two rows.
An **embedded** field (no name: `*kv.Base`, `sync.Mutex`) is not a row: it is a
relation (§5).

**Methods** are declared outside the type, anywhere in its package, with a
receiver, and attach to the box of the receiver's base type:

- `*Store[K, V]` and `Store[K, V]` both attach to `Store`: pointer and value
  receivers draw alike (the difference matters for §5.2, not the picture), and
  type arguments are stripped to the head type.
- A receiver that is an alias (`type A = B`) attaches to `B`, which is what Go
  does. A defined type `type A B` is a new type and gets none of `B`'s methods.
- A method whose receiver type is not declared in the package attaches to
  nothing (it cannot occur in code that builds).

Signatures render as `name(params) results`, parameter names as written and a
multi-value result in parentheses: `+Get(ctx context.Context, key string)
([]byte, error)`. Curated diagrams render names only (`Params::NamesOnly`).

**Package-level functions** go to the module box only. A constructor (`func
NewStore() *Store`) is not moved into `Store`'s box: Go has no such association,
and a name-based guess is the kind of edge this repository refuses.

**Promoted methods** (gained through embedding) are not drawn as members of the
embedding type: the `inherits` edge (§5.1) says they are there.

### 4.1 The package merge

vizzle parses file by file and merges with a plain extend, then keeps the
first box per qualified name (`CodeGraph::normalize`). Rust's impl merging
happens inside one file only. That is fatal for Go, where a type's methods
normally live in other files of its package (h's `cm.WatchController` has
methods in both `watch_scan.go` and `watch_after.go`), and where every file
would push its own module box for the package, so most of h's 1,100 functions
would vanish.

So Go adds a **package merge pass** after per-file parsing and before
normalization, keyed by import path:

1. One box per declared type, carrying the fields of its declaration and the
   methods attached to it from every file of the package.
2. One module box per package, carrying the package-level functions, consts and
   vars of every file.
3. **Twins** (the same name declared in two constrained files: h's
   `proc/token_{darwin,linux}.go` `tokenHolders`, `proc/watch_{darwin,linux}.go`
   `waitExit`, `proc/containment_{linux,other}.go` `newContainment`) merge into
   one, for types, methods, functions, consts and vars alike. The merged
   item's body hash covers every definition in path order, so a change to either
   marks it modified; a type's fields and methods are the union. Nothing is
   dropped by de-duplication.

The pass is Go's; other languages keep today's merge.

## 5. Relations

### 5.1 Declared, and how a name resolves

**Resolution is through the file's imports, and nothing else.** The shared
resolver tries the same module, then any globally unique name (`resolve.rs`).
For Go that draws wrong edges in h: `cm/github.go`'s `Client *http.Client`
would resolve to the unique in-tree `api.Client`, and `fabric`'s
`*server.Server` to `api.Server`. So the Go parser emits targets already
qualified:

- `pkg.Name`: `pkg` is looked up in the file's import map (alias, else the
  import path's last element). An internal package (§6) gives
  `<import path>.Name`; a standard-library or external package gives an
  external target that never resolves to a box.
- An unqualified `Name`: a type declared in the same package, else a
  predeclared type (`string`, `error`), else nothing. In a file with a dot
  import (`import . "x"`), an unqualified name not declared in its own package
  resolves to nothing.
- The global-unique fallback is off for Go (a Go branch in `resolve.rs`, or the
  parser marking its targets as already resolved).

| Go source | Edge |
|---|---|
| struct embeds a type (`*kv.Base`, `Base`) | `inherits` (`--|>`) |
| interface embeds an interface (`io.Closer`) | `inherits` |
| a field's type names a box | `association` |
| a method's parameter or result type names a box | `dependency` |
| `var _ I = (*T)(nil)` or `var _ I = T{}` | `implements` (`..|>`), always drawn |

Embedding is drawn as `inherits` with no label: class.md §5.3 rejects relation
labels, and method promotion is the behaviour the reader must see.

### 5.2 Inferred implementation (§12 D2)

Go interfaces are satisfied implicitly, and h's kernel has 50 interfaces and no
`var _ I = …` assertions. Without inference every interface in it draws
unconnected.

**`T ..|> I` is drawn when all of these hold:**

- `I` is declared in the scanned tree (never an external interface such as
  `io.Reader`: its method set is not in the graph), has at least one method,
  and has no type element (a `<<constraint>>` is not something a type
  implements in the diagram's sense).
- Every interface `I` embeds is in the tree too, so its method set is known
  after flattening. An embedded external interface means no inferred edges for
  `I`.
- `T` is a struct or defined type in the tree, and the method set of `*T` (its
  pointer- and value-receiver methods) has every method of `I` with the same
  name and the same normalized parameter and result types (§9).
- A method of `I` with an unexported name matches only within `I`'s own
  package, as Go's rules say.

**What the edge means.** Measured with `go/types` on h's kernel: of 57 true
implementations, **54 hold only for `*T`**. A `T` value never has
pointer-receiver methods, so using `T`'s own method set would draw almost
nothing. The edge therefore means "`T` or `*T` implements `I`", and `-f json`
says which: `"via": "pointer"` or `"via": "value"` on the relation. An explicit
`var _ I = (*T)(nil)` assertion is `"via": "pointer"` too.

Type comparison is textual after normalization (qualified through imports,
parameter names dropped), because vizzle does not type-check. Two spellings of
one type that normalization cannot equate miss the edge, which is the safe
direction. Methods promoted through embedding are not counted, so a type that
satisfies `I` only through an embedded field gets no inferred edge; no h edge
depends on that.

**Worked example.** `type Reader interface { Get(ctx context.Context, key
string) ([]byte, error) }` and `func (s *Store) Get(ctx context.Context, k
string) ([]byte, error)` in the tree: `Store ..|> Reader`, `"via": "pointer"`.
If `Reader` also embeds `io.Closer`, no edge is inferred to `Reader`.

## 6. Imports

An import spec gives a path and an optional name (`kv "example.com/x/kv"`,
`_ "embed"`, `. "x"`). It resolves:

1. **Internal**: the path equals, or starts with `<module path>/`, the module
   path of a `go.mod` in the tree (longest match). It resolves to that package
   directory if the directory has selected files, otherwise to nothing. (Every
   module in a multi-module tree has its own `go.mod`, so `go.work` adds nothing
   to resolution and is not read.)
2. **Standard library**: the first path element has no dot (`context`,
   `net/http`). Never a node, even with `--externals`.
3. **External**: anything else. With `--externals`, one node per module: the
   longest `require` path in the importing module's `go.mod` that prefixes the
   import path; with no matching `require`, the path itself.

Blank imports (`_`) are edges like any other: a driver registered for side
effects is a real dependency.

## 7. Components and edges (§12 D1)

**A Go component is a package**, not a `go.mod`. Go defines the package as the
unit of import, and its import paths are exact, so package-level edges need no
best-effort guessing. Treating a module as one component would draw h's kernel
(one `go.mod`, 26 packages) as a single box, the same failure a one-manifest
Python service has (which needed `--split`).

This amends component.md §3.1 for Go only:

- **Rule 1 and 2:** a `go.mod` does not make a component; it makes a *module
  block*. Its packages are the components. A `go.mod` at the repository root is
  a module like any other (most Go repositories, chi included, have one), so
  rule 2's "the root is never a component" does not hide it.
- **Ownership:** a Go package claims only its `.go` files. A non-Go file inside
  a Go module's tree (a `pyproject.toml` package under it, a script) belongs to
  whatever the existing rules give it.
- **Names:** a package component is named by its directory relative to its
  module root (`lease`, `cli/render`; the module root package by the module
  path's last element). When that collides with another component (h has a
  root `cli/` Python package and kernel packages `cli` and `api`), the Go name
  is prefixed with its module path's last element (`kernel/cli`), then with the
  full import path as a last resort. The existing `disambiguate_names` prefixes
  by last path segment, which would mangle a name that already contains `/`;
  Go names take this rule instead.
- **Classes to components:** a Go class belongs to the component of its import
  path. The existing map (`module_owner`) keys on a file's dotted module path,
  so Go needs its own lookup, or every Go class lands in no component and
  `--classes` and the zoom show nothing.
- **Grouping:** a `subgraph` per module, then by parent directory within it, as
  component.md §2.3 and §3.2 group by directory.
- **`package main`** gets the stereotype `«main»` in place of `«component»`.
- **Edges:** `A → B` when a selected file of package `A` imports package `B`
  (§6 rule 1); weight is the number of distinct importing files.
- **`--split`** does nothing for Go packages (with a note on stderr): it exists
  to recover the package structure Go already declares.
- **`--around` and `--highlight`** take the component's display name, as
  today.

**Worked example.** h's kernel: one block `github.com/h/kernel` holding 26
package boxes, seven of them `«main»` under `cmd/`, with edges from the 19
internal import paths that 65 non-test files use. In h's whole repository the
kernel's `cli` and `api` packages render as `kernel/cli` and `kernel/api`
beside the Python `cli` component.

## 8. `vizzle doc` (curated manifests)

A `gen:c4-code` entry addresses Go as other languages do: `file` names a `.go`
file, `symbol` a type in its package. Because Go types belong to packages, an
entry resolves `symbol` in the package of `file`, after the merge (§4.1), so
methods from other files of the package appear. `kind: "module"` names a file
and lists `functions`, and `consts` for any package-level const or var by name.
`scope.lang` accepts `go`.

## 9. Diff identity and fingerprints

A type is keyed by its qualified name (§2). A method is keyed by its name
within its receiver's box (Go has no overloading); fields by name.
Fingerprints include the normalized signature and the body hash, so a
body-only change marks a method modified. Moving a method between files of a
package, or switching its receiver between `T` and `*T`, changes neither key
nor fingerprint. A package rename or move is remove plus add. Twins merge as
§4.1 says.

Normalization, for §5.2 and fingerprints alike: types qualified through the
file's imports, parameter names dropped, whitespace collapsed, a variadic
`...T` kept distinct from `[]T`.

## 10. Grammar and parser binding

`tree-sitter-go = "0.25"` with the repository's `tree-sitter = "0.27"`. Checked
on 2026-11-18 in a scratch crate: it loads with
`parser.set_language(&tree_sitter_go::LANGUAGE.into())` and parses, without
error nodes, a file holding every construct above. The node kinds this spec
relies on, as that grammar names them:

- `package_clause` and `import_spec` (`name`, `path`)
- `type_spec` and `type_alias` (`name`, `type_parameters`, `type`)
- `struct_type` → `field_declaration` (a nameless one is embedded)
- `interface_type` → `method_elem`, and `type_elem`. A `type_elem` holding a
  single interface type name is an embedded interface; one holding a union
  (`|`), a `~` term or a non-interface type makes the interface a
  `<<constraint>>` (§3).
- `method_declaration` (`receiver`, `name`, `parameters`, `result`, `body`)
- `function_declaration`, `const_spec`, `var_spec`, `iota`

## 11. Acceptance criteria for the implementation PR

### 11.1 h's kernel

Measured on h main `1866f3594`, `kernel/`, module `github.com/h/kernel`:

- 256 `.go` files, 103 of them tests (153 non-test);
- 26 non-test packages, 7 of them under `cmd/`;
- 50 interfaces;
- 369 package-level non-test struct types, plus 7 function-local;
- 869 methods and 1,100 functions;
- 1 alias, 3 typed `iota` enumerations, 1 generated file;
- 0 `var _ I =` assertions;
- 19 distinct internal import paths, used by 65 non-test files;
- 57 true implementations by `go/types` on darwin, 54 of them pointer-only.

The criteria:

1. `vizzle component kernel` draws one module block with 26 package boxes, 7 of
   them `«main»`. Its internal edges must be confirmed by a `grep` of the
   importing files (spot-check every edge into `lease` and `ledger`).
2. `vizzle class kernel --no-members -I 'lease/**'` lists every type declared in
   `kernel/lease/*.go`, grouped `type ( … )` blocks included. Compare against a
   parse, not `grep '^type '`. With members on, a type's methods from every file
   of its package appear (`cm.WatchController` from both `watch_scan.go` and
   `watch_after.go`).
3. `vizzle class kernel --modules -f json` holds 26 module boxes with the
   package-level functions of every file, none named `com/…`.
4. Inferred `implements`:
   - Compare against the `go/types` result (57 on darwin). Report misses and
     extras in the PR; any extra is a defect.
   - Report the largest implementer count per interface (today `cm.Kind`, 7) and
     the largest interface count per type (today `*cm.Registries`, 17), so the
     reader load on both sides is visible before merge.
5. No member edge points into the tree from a standard-library or external type:
   `cm/github.go`'s `*http.Client` field draws no edge to `api.Client`.
6. The three typed `iota` types draw as `<<enumeration>>` with their constants,
   and `cm/schedule.go`'s untyped block does not.
7. The `proc` twins each draw once.
8. `vizzle diff kernel --base <a kernel commit> --type component` names
   packages, and a body-only change in one method draws that method `✱`.
9. `--tests` adds the `*_test.go` files; without it, the trailer says
   `tests excluded`.

### 11.2 A small external repository

`go-chi/chi` at a pinned tag: root `go.mod`, a multi-package single module,
middleware and router interfaces, external imports. `component` draws its
packages (the root `go.mod` is not hidden), and `component --externals` shows
no standard-library nodes and one node per required module.

### 11.3 This repository

The plugin's skills say vizzle parses Go (the precondition in `vizzle-diagrams`
and the levels table in `explain-code`), and `test_plugin.py` still passes.
`docs/distribution.md`, `class.md` §2 and component.md §3.1 gain a line each.

## 12. Decisions (open; each with a recommendation)

| | Question | Recommendation | Why |
|---|---|---|---|
| D1 | Is a Go component a package or a module (`go.mod`)? | **A package**, in a block per module (§7) | Go's unit of import is the package and its import paths are exact. A module-level component draws h's kernel as one box. The cost is Go-specific rules for naming, ownership and the class map, all listed in §7 |
| D2 | Infer `implements` from method sets? | **Yes**, under §5.2's rule; explicit assertions always drawn | Satisfying an interface is a fact in Go, not a guess. Measured on h with `go/types`: 57 implementations, the busiest interface has 7 implementers, the busiest type implements 17; acceptance 4 holds the inference to `go/types` |
| D3 | Struct embedding: which edge? | **`inherits`, unlabelled** | Method promotion is what the reader must see; class.md §5.3 rejects labels |
| D4 | Constructors (`NewT`) in `T`'s box? | **No** | No such association exists in Go; a name-based guess is the wrong-edge risk |
| D5 | Standard library as nodes under `--externals`? | **Never** | Every package imports `fmt` and `context`; drawing them is noise |
| D6 | Mark `package main`? | **Yes, `«main»`** | Entry points are what a reader of a Go repository looks for first |
| D7 | Build constraints | **Not evaluated, except `//go:build ignore`; twins merge** (§4.1) | Evaluating needs a target platform; merging keeps one row and still sees a change to either file. `ignore` means "not in this package" on every platform |
| D8 | Generated files | **Included** | They are the real API of many Go packages; `-E` removes them |
| D9 | What does an inferred edge mean, given 54 of h's 57 implementations are pointer-only? | **"`T` or `*T` implements `I`"**, with `via` in JSON (§5.2) | `T`'s own method set would draw almost nothing; the box is the type either way |
