//! Language-neutral code graph extracted from source files.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Python,
    TypeScript,
}

impl Language {
    pub fn from_path(path: &str) -> Option<Self> {
        let path = path.strip_suffix(".d.ts").map(|_| "").unwrap_or(path);
        match path.rsplit('.').next()? {
            "py" => Some(Language::Python),
            "ts" | "tsx" | "mts" | "cts" => Some(Language::TypeScript),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::TypeScript => "typescript",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Public,
    Protected,
    Private,
}

impl Visibility {
    pub fn sigil(&self) -> char {
        match self {
            Visibility::Public => '+',
            Visibility::Protected => '#',
            Visibility::Private => '-',
        }
    }
}

/// Change status relative to a git base revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChangeKind {
    #[default]
    Unchanged,
    Added,
    Removed,
    Modified,
}

impl ChangeKind {
    /// Marker appended to a label in diff mode. Every renderer uses these, so
    /// a class, a member, and a component all read the same way.
    pub fn glyph(&self) -> &'static str {
        match self {
            ChangeKind::Added => " ✚",
            ChangeKind::Removed => " ✖",
            ChangeKind::Modified => " ✱",
            ChangeKind::Unchanged => "",
        }
    }
}

/// How many elements of a diff graph carry each change kind: the verdict a
/// consumer wants from a diff ("did anything change, and how much") without
/// reading the drawing. Exposed as `stats.changes` in both JSON exports and
/// through the CLI's `--stats` sidecar, so tooling never has to key on class
/// names or glyphs that are free to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChangeCounts {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

impl ChangeCounts {
    pub fn tally<I: IntoIterator<Item = ChangeKind>>(changes: I) -> Self {
        let mut counts = Self::default();
        for change in changes {
            match change {
                ChangeKind::Added => counts.added += 1,
                ChangeKind::Removed => counts.removed += 1,
                ChangeKind::Modified => counts.modified += 1,
                ChangeKind::Unchanged => {}
            }
        }
        counts
    }

    pub fn changed(&self) -> bool {
        self.added + self.removed + self.modified > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Member {
    pub name: String,
    pub visibility: Visibility,
    /// Rendered parameter list for methods (`name: Type, other`), type for fields.
    pub detail: String,
    pub returns: Option<String>,
    /// Type expressions this member mentions — a field's type, a method's
    /// parameter and return types. Resolved into association and dependency
    /// edges at render time; kept raw so resolution stays a graph-level
    /// decision rather than a parser one.
    pub type_refs: Vec<String>,
    /// Parameter names alone. `detail` renders them with their types, which is
    /// right for an exhaustive diagram and unreadable in a curated one — a
    /// typer command renders 700 characters wide (curated-diagrams.md §5.1).
    pub param_names: Vec<String>,
    pub is_method: bool,
    pub is_static: bool,
    pub is_abstract: bool,
    /// Hash of the member's defining source text — a method's whole
    /// definition, a field's assignment. Two revisions of a member with the
    /// same signature and different bodies are a change a reviewer must see
    /// (class.md §7): a bug fix rarely touches a signature, and a diff that
    /// fingerprints signatures alone draws the fix's component as changed
    /// with nothing inside it. Zero when the parser had no text to hash.
    pub body_hash: u64,
    pub change: ChangeKind,
}

impl Member {
    /// A stable fingerprint used to detect modifications between revisions:
    /// the signature and the body. Two members that differ only in body
    /// carry the same name, so [`crate::diff`] reads them as `Modified`.
    pub fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.name,
            self.detail,
            self.returns.as_deref().unwrap_or(""),
            self.is_method,
            self.is_static,
            self.is_abstract,
            self.body_hash
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord)]
pub enum RelationKind {
    /// `Derived --|> Base`
    Inherits,
    /// `Impl ..|> Interface`
    Implements,
    /// `Holder --> Held`: a field's type names another class (structural).
    Association,
    /// `User ..> Used`: a method signature names another class (uses).
    Dependency,
}

impl RelationKind {
    /// Mermaid arrow for this relation.
    pub fn arrow(&self) -> &'static str {
        match self {
            RelationKind::Inherits => "--|>",
            RelationKind::Implements => "..|>",
            RelationKind::Association => "-->",
            RelationKind::Dependency => "..>",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            RelationKind::Inherits => "inherits",
            RelationKind::Implements => "implements",
            RelationKind::Association => "association",
            RelationKind::Dependency => "dependency",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    /// Qualified name of the subtype.
    pub from: String,
    /// Base type name as written in source (resolved against the graph at render time).
    pub to: String,
    pub kind: RelationKind,
}

/// The stereotype marking a `<<module>>` box (class.md §2.4). Named once: the
/// parsers write it, the renderer keys its filter on it.
pub const MODULE_ANNOTATION: &str = "module";

#[derive(Debug, Clone)]
pub struct Class {
    /// Bare class name, e.g. `AgentRunner`.
    pub name: String,
    /// Unique key: `<module>.<name>` (nested classes: `<module>.<Outer>.<Inner>`).
    pub qualified: String,
    /// Dotted module path derived from the file path, e.g. `apps.dapr_agent.main`.
    pub module: String,
    /// Repo-relative path of the defining file, e.g. `apps/dapr_agent/main.py`.
    /// The module path is derived from it and loses the extension and the
    /// `__init__` spelling; the file is what a reader opens.
    pub file: String,
    /// UML stereotype: `interface`, `abstract`, `enumeration`, ...
    pub annotation: Option<String>,
    pub bases: Vec<Relation>,
    pub members: Vec<Member>,
    pub lang: Language,
    pub change: ChangeKind,
}

impl Class {
    /// Whether this is a `<<module>>` box (class.md §2.4) rather than a type.
    pub fn is_module_box(&self) -> bool {
        self.annotation.as_deref() == Some(MODULE_ANNOTATION)
    }

    /// The file's base name — `worker.py`, `index.ts` — which is how the
    /// zoom (component.md §6.4) tells a reader where a class lives.
    pub fn file_name(&self) -> &str {
        self.file.rsplit('/').next().unwrap_or(&self.file)
    }

    /// The members a renderer draws. A module box holds every module-level
    /// function the parser saw, private helpers included, because a diff has
    /// to notice a changed helper; but a helper is not module surface
    /// (class.md §2.5), so an *unchanged* private one is never drawn. Types
    /// draw all their members — a class's private methods are its shape.
    pub fn drawn_members(&self) -> impl Iterator<Item = &Member> {
        let module_box = self.is_module_box();
        self.members.iter().filter(move |m| {
            !module_box || m.visibility == Visibility::Public || m.change != ChangeKind::Unchanged
        })
    }

    pub fn fingerprint(&self) -> String {
        let mut members: Vec<String> = self.members.iter().map(Member::fingerprint).collect();
        members.sort();
        let mut bases: Vec<String> = self.bases.iter().map(|r| r.to.clone()).collect();
        bases.sort();
        format!(
            "{}|{}|{}|{}",
            self.annotation.as_deref().unwrap_or(""),
            bases.join(","),
            members.join(";"),
            self.lang.name()
        )
    }
}

/// A file-level import, the raw material of the component graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// Repo-relative path of the importing file.
    pub file: String,
    /// Specifier as written in source: `@h/core` or `./util` (TypeScript),
    /// `a.b.c` or `..mod` (Python, relative dots preserved).
    pub target: String,
    pub lang: Language,
}

/// Everything extracted from one revision of a set of source files.
#[derive(Debug, Clone, Default)]
pub struct CodeGraph {
    pub classes: Vec<Class>,
    pub imports: Vec<Import>,
}

impl CodeGraph {
    /// Whether anything in the graph carries a change — i.e. render the diff lens.
    pub fn diff_mode(&self) -> bool {
        self.classes
            .iter()
            .any(|c| c.change != ChangeKind::Unchanged)
    }

    pub fn merge(&mut self, other: CodeGraph) {
        self.classes.extend(other.classes);
        self.imports.extend(other.imports);
    }

    /// Sort classes and imports for deterministic output.
    pub fn normalize(&mut self) {
        self.classes.sort_by(|a, b| a.qualified.cmp(&b.qualified));
        self.classes.dedup_by(|a, b| a.qualified == b.qualified);
        self.imports
            .sort_by(|a, b| (&a.file, &a.target).cmp(&(&b.file, &b.target)));
        self.imports.dedup();
    }
}

impl CodeGraph {
    /// Drop the `<<module>>` boxes (class.md §2.4). Their relations go with
    /// them, and a kept class naming one simply fails to resolve — §4's
    /// "ambiguity resolves to nothing" already covers that.
    pub fn without_module_boxes(&self) -> CodeGraph {
        CodeGraph {
            classes: self
                .classes
                .iter()
                .filter(|c| !c.is_module_box())
                .cloned()
                .collect(),
            ..self.clone()
        }
    }
}

impl CodeGraph {
    /// Change counts over classes. Classes are the unit: a changed member
    /// marks its class modified, and relations carry no change status of
    /// their own.
    pub fn change_counts(&self) -> ChangeCounts {
        ChangeCounts::tally(self.classes.iter().map(|c| c.change))
    }
}

#[cfg(test)]
mod change_count_tests {
    use super::*;

    #[test]
    fn tallies_each_kind_and_ignores_unchanged() {
        let counts = ChangeCounts::tally([
            ChangeKind::Added,
            ChangeKind::Unchanged,
            ChangeKind::Modified,
            ChangeKind::Added,
            ChangeKind::Removed,
        ]);
        assert_eq!(
            counts,
            ChangeCounts {
                added: 2,
                removed: 1,
                modified: 1
            }
        );
        assert!(counts.changed());
        assert!(!ChangeCounts::tally([ChangeKind::Unchanged; 3]).changed());
    }
}
