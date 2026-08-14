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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub visibility: Visibility,
    /// Rendered parameter list for methods (without parens), type for fields.
    pub detail: String,
    pub returns: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum RelationKind {
    /// `Derived --|> Base`
    Inherits,
    /// `Impl ..|> Interface`
    Implements,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    /// Qualified name of the subtype.
    pub from: String,
    /// Base type name as written in source (resolved against the graph at render time).
    pub to: String,
    pub kind: RelationKind,
}

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

/// Everything extracted from one revision of a set of source files.
#[derive(Debug, Clone, Default)]
pub struct CodeGraph {
    pub classes: Vec<Class>,
}

impl CodeGraph {
    pub fn merge(&mut self, other: CodeGraph) {
        self.classes.extend(other.classes);
    }

    /// Sort classes for deterministic output.
    pub fn normalize(&mut self) {
        self.classes.sort_by(|a, b| a.qualified.cmp(&b.qualified));
        self.classes.dedup_by(|a, b| a.qualified == b.qualified);
    }
}
