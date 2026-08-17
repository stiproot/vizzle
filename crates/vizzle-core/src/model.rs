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
    pub change: ChangeKind,
}

impl Member {
    /// A stable fingerprint used to detect modifications between revisions.
    pub fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.name,
            self.detail,
            self.returns.as_deref().unwrap_or(""),
            self.is_method,
            self.is_static,
            self.is_abstract
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
    /// UML stereotype: `interface`, `abstract`, `enumeration`, ...
    pub annotation: Option<String>,
    pub bases: Vec<Relation>,
    pub members: Vec<Member>,
    pub lang: Language,
    pub change: ChangeKind,
}

impl Class {
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
                .filter(|c| c.annotation.as_deref() != Some(MODULE_ANNOTATION))
                .cloned()
                .collect(),
            ..self.clone()
        }
    }
}
