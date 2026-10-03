# The core model

`crates/vizzle-core/src/model.rs` defines the language-neutral shape every
parser produces and every renderer consumes. A `CodeGraph` is one revision's
worth of code: the `Class` boxes it contains and the `Import` edges between
files that make up the component view. A `Class` is a named type (or a
`<<module>>` box), holding the `Member` rows — fields, methods, enum variants —
that a reader sees inside it. A `Relation` is one edge between two classes,
carrying a `RelationKind` that says whether it is inheritance, implementation,
association or a dependency. `Import` records a file's import of another,
before classes are grouped into components.

Enumerations such as `Language`, `Visibility`, `ChangeKind` and `RelationKind`
are part of the same model: their variants become members, which is how a
constructed diagram stays honest about the values a type can take. `ChangeCounts`
summarises how many elements of a diff carry each change kind.

<!-- gen:c4-code {
  "classes": [
    {"id": "CodeGraph", "kind": "class", "file": "crates/vizzle-core/src/model.rs", "symbol": "CodeGraph"},
    {"id": "Class", "kind": "class", "file": "crates/vizzle-core/src/model.rs", "symbol": "Class"},
    {"id": "Member", "kind": "class", "file": "crates/vizzle-core/src/model.rs", "symbol": "Member"},
    {"id": "Relation", "kind": "class", "file": "crates/vizzle-core/src/model.rs", "symbol": "Relation"},
    {"id": "Import", "kind": "class", "file": "crates/vizzle-core/src/model.rs", "symbol": "Import"},
    {"id": "ChangeCounts", "kind": "class", "file": "crates/vizzle-core/src/model.rs", "symbol": "ChangeCounts"},
    {"id": "Language", "kind": "enumeration", "file": "crates/vizzle-core/src/model.rs", "symbol": "Language"},
    {"id": "Visibility", "kind": "enumeration", "file": "crates/vizzle-core/src/model.rs", "symbol": "Visibility"},
    {"id": "ChangeKind", "kind": "enumeration", "file": "crates/vizzle-core/src/model.rs", "symbol": "ChangeKind"},
    {"id": "RelationKind", "kind": "enumeration", "file": "crates/vizzle-core/src/model.rs", "symbol": "RelationKind"}
  ],
  "relations": [
    ["CodeGraph", "Class", null, "classes"],
    ["CodeGraph", "Import", null, "imports"],
    ["Class", "Member", null, "members"],
    ["Class", "Relation", null, "bases"],
    ["Class", "Language", null, "lang"],
    ["Member", "Visibility", null, "visibility"],
    ["Relation", "RelationKind", null, "kind"]
  ]
} -->

```mermaid
classDiagram
  class CodeGraph {
    <<class>>
    +change_counts(self) ChangeCounts
    +classes : Vec~Class~
    +diff_mode(self) bool
    +imports : Vec~Import~
    +merge(self, other)
    +normalize(self)
    +without_module_boxes(self) CodeGraph
  }

  class Class {
    <<class>>
    +annotation : Option~String~
    +bases : Vec~Relation~
    +change : ChangeKind
    +drawn_members(self) impl Iterator~Item = &Member~
    +file : String
    +file_name(self) &str
    +fingerprint(self) String
    +is_module_box(self) bool
    +lang : Language
    +members : Vec~Member~
    +module : String
    +name : String
    +qualified : String
  }

  class Member {
    <<class>>
    +body_hash : u64
    +change : ChangeKind
    +detail : String
    +fingerprint(self) String
    +is_abstract : bool
    +is_method : bool
    +is_static : bool
    +name : String
    +param_names : Vec~String~
    +returns : Option~String~
    +type_refs : Vec~String~
    +visibility : Visibility
  }

  class Relation {
    <<class>>
    +from : String
    +kind : RelationKind
    +to : String
  }

  class Import {
    <<class>>
    +file : String
    +lang : Language
    +target : String
  }

  class ChangeCounts {
    <<class>>
    +added : usize
    +changed(self) bool
    +modified : usize
    +removed : usize
    +tally(changes)$ Self
  }

  class Language {
    <<enumeration>>
    +Python
    +Rust
    +TypeScript
    +from_path(path)$ Option~Self~
    +name(self) &str
  }

  class Visibility {
    <<enumeration>>
    +Private
    +Protected
    +Public
    +sigil(self) char
  }

  class ChangeKind {
    <<enumeration>>
    +Added
    +Modified
    +Removed
    +Unchanged
    +glyph(self) &str
  }

  class RelationKind {
    <<enumeration>>
    +Association
    +Dependency
    +Implements
    +Inherits
    +arrow(self) &str
    +name(self) &str
  }

  CodeGraph --> Class : classes
  CodeGraph --> Import : imports
  Class --> Member : members
  Class --> Relation : bases
  Class --> Language : lang
  Member --> Visibility : visibility
  Relation --> RelationKind : kind
```
