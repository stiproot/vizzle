//! Python class extraction using tree-sitter-python.

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

use super::{clean_type, text, text_hash};
use crate::model::*;

pub fn parse(module: &str, source: &str) -> Result<CodeGraph> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .context("loading python grammar")?;
    let tree = parser
        .parse(source, None)
        .context("tree-sitter failed to parse python source")?;

    let mut graph = CodeGraph::default();
    let mut module_fns = Vec::new();
    let mut values = Vec::new();
    collect(
        tree.root_node(),
        source,
        module,
        None,
        &mut graph,
        &mut ModuleSurface {
            members: &mut module_fns,
            values: &mut values,
        },
    );
    super::push_module_box(module, module_fns, Language::Python, &mut graph);
    if !values.is_empty() {
        values.sort_by(|a, b| a.name.cmp(&b.name));
        graph.module_values.insert(module.to_owned(), values);
    }
    Ok(graph)
}

/// What a module declares at its top level besides classes: the members of
/// its `<<module>>` box, and the assignments only a manifest may ask for.
struct ModuleSurface<'a> {
    members: &'a mut Vec<Member>,
    values: &'a mut Vec<Member>,
}

/// Walk top-level (and nested) statements, collecting class definitions.
fn collect(
    node: Node,
    src: &str,
    module: &str,
    outer: Option<&str>,
    graph: &mut CodeGraph,
    surface: &mut ModuleSurface,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "class_definition" => extract_class(child, src, module, outer, &[], graph),
            // Only reached at module level: the walk never descends into a
            // class body (extract_class owns that) or a function body, so any
            // definition seen here is module surface (class.md §2.4).
            "function_definition" => extract_module_function(child, src, &[], surface.members),
            "decorated_definition" => {
                let decorators = decorator_names(child, src);
                if let Some(def) = child.child_by_field_name("definition") {
                    match def.kind() {
                        "class_definition" => {
                            extract_class(def, src, module, outer, &decorators, graph)
                        }
                        "function_definition" => {
                            extract_module_function(def, src, &decorators, surface.members)
                        }
                        _ => {}
                    }
                }
            }
            // A module-level UPPER_SNAKE assignment is a constant by Python's
            // own convention and joins the module box; any other public name
            // is kept aside for a manifest's `consts` to name (curated-diagrams.md §4).
            "expression_statement" => extract_module_const(child, src, surface),
            "import_statement" | "import_from_statement" => extract_imports(child, src, graph),
            // Classes (and imports) can hide inside `if TYPE_CHECKING:` etc.
            "if_statement" | "try_statement" | "block" => {
                collect(child, src, module, outer, graph, surface)
            }
            // The other branches have never fed the box or the class list, and
            // changing that would change every diagram that has one. A value set
            // there (`except ImportError: tomllib = None`) is still one a manifest
            // may name, so these branches feed `values`, whatever the name's case.
            "else_clause" | "elif_clause" | "except_clause" | "finally_clause" => {
                collect_branch_values(child, src, surface.values)
            }
            _ => {}
        }
    }
}

/// Every public plain-name assignment in a branch `collect` does not walk for
/// the box, nested branches included, as values a manifest's `consts` may name.
fn collect_branch_values(node: Node, src: &str, values: &mut Vec<Member>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "expression_statement" => {
                let mut members = Vec::new();
                let mut surface = ModuleSurface {
                    members: &mut members,
                    values,
                };
                extract_module_const(child, src, &mut surface);
                values.append(&mut members);
            }
            "block" | "if_statement" | "try_statement" | "else_clause" | "elif_clause"
            | "except_clause" | "finally_clause" => collect_branch_values(child, src, values),
            _ => {}
        }
    }
}

/// A public module-level assignment to a plain name. UPPER_SNAKE is how
/// Python says "constant", and only those join the module box: that is what
/// keeps ordinary module-level assignment out of every diagram. The rest go to
/// `values`, which nothing draws; a manifest that lists one by name is the
/// author saying "this one" (curated-diagrams.md §4).
fn extract_module_const(stmt: Node, src: &str, surface: &mut ModuleSurface) {
    let Some(assign) = ({
        let mut cursor = stmt.walk();
        let found = stmt
            .named_children(&mut cursor)
            .find(|c| c.kind() == "assignment");
        found
    }) else {
        return;
    };
    let Some(left) = assign.child_by_field_name("left") else {
        return;
    };
    if left.kind() != "identifier" {
        return;
    }
    let name = text(left, src);
    if name.starts_with('_') {
        return;
    }
    let is_const = name
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name.chars().any(|c| c.is_ascii_uppercase());
    let detail = assign
        .child_by_field_name("type")
        .map(|ty| clean_type(&text(ty, src)))
        .unwrap_or_default();
    let member = Member {
        name,
        detail,
        body_hash: text_hash(assign, src),
        ..Default::default()
    };
    if is_const {
        surface.members.push(member);
    } else {
        surface.values.push(member);
    }
}

/// A module-level `def`; the shape is a method's. Private helpers (leading
/// underscore) are kept with their visibility so a diff can notice one that
/// changed; the renderer leaves an unchanged one out
/// ([`Class::drawn_members`]), because a helper is not module surface.
fn extract_module_function(
    func: Node,
    src: &str,
    decorators: &[String],
    members: &mut Vec<Member>,
) {
    extract_method(func, src, decorators, members);
}

/// `import a.b, c` -> targets `a.b`, `c`; `from ..x import y` -> target `..x`
/// (relative dots preserved). The file path is stamped on by [`super::parse_file`].
fn extract_imports(node: Node, src: &str, graph: &mut CodeGraph) {
    let mut push = |target: String| {
        if !target.is_empty() {
            graph.imports.push(Import {
                file: String::new(),
                target,
                lang: Language::Python,
            });
        }
    };
    if node.kind() == "import_from_statement" {
        if let Some(module_name) = node.child_by_field_name("module_name") {
            push(text(module_name, src));
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "dotted_name" => push(text(child, src)),
            "aliased_import" => {
                if let Some(name) = child.child_by_field_name("name") {
                    push(text(name, src));
                }
            }
            _ => {}
        }
    }
}

fn decorator_names(decorated: Node, src: &str) -> Vec<String> {
    let mut cursor = decorated.walk();
    decorated
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "decorator")
        .filter_map(|n| n.named_child(0))
        .map(|expr| {
            // `@dataclass(frozen=True)` -> `dataclass`
            let base = match expr.kind() {
                "call" => expr.child_by_field_name("function").unwrap_or(expr),
                _ => expr,
            };
            text(base, src)
        })
        .collect()
}

fn extract_class(
    node: Node,
    src: &str,
    module: &str,
    outer: Option<&str>,
    decorators: &[String],
    graph: &mut CodeGraph,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let local_name = match outer {
        Some(outer) => format!("{outer}.{name}"),
        None => name.clone(),
    };
    let qualified = format!("{module}.{local_name}");

    let mut bases = Vec::new();
    let mut annotation = None;
    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        let mut cursor = superclasses.walk();
        for arg in superclasses.named_children(&mut cursor) {
            if arg.kind() == "keyword_argument" {
                continue; // metaclass=...
            }
            let base_text = clean_type(&text(arg, src));
            // Bare name of the base, ignoring module prefix and generics.
            let bare = base_text
                .split('~')
                .next()
                .unwrap_or(&base_text)
                .rsplit('.')
                .next()
                .unwrap_or(&base_text)
                .to_owned();
            match bare.as_str() {
                "object" => {}
                "Protocol" => annotation = Some("interface".to_owned()),
                "ABC" | "ABCMeta" => annotation = Some("abstract".to_owned()),
                "Enum" | "IntEnum" | "StrEnum" | "Flag" | "IntFlag" => {
                    annotation = Some("enumeration".to_owned())
                }
                _ => bases.push(Relation {
                    from: qualified.clone(),
                    to: bare,
                    kind: RelationKind::Inherits,
                }),
            }
        }
    }
    if decorators.iter().any(|d| d.ends_with("dataclass")) {
        annotation.get_or_insert_with(|| "dataclass".to_owned());
    }
    if decorators.iter().any(|d| d.ends_with("runtime_checkable")) {
        annotation.get_or_insert_with(|| "interface".to_owned());
    }

    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        extract_body(body, src, &mut members);
        // Nested classes only. The discard buffers are deliberate: this body's
        // `def`s are methods and its assignments fields, already taken by
        // extract_body above, and must not also land in the module's surface.
        let discard = &mut ModuleSurface {
            members: &mut Vec::new(),
            values: &mut Vec::new(),
        };
        collect(body, src, module, Some(&local_name), graph, discard);
    }

    let is_abstract = members.iter().any(|m: &Member| m.is_abstract);
    if is_abstract && annotation.is_none() {
        annotation = Some("abstract".to_owned());
    }

    graph.classes.push(Class {
        name: local_name,
        qualified,
        module: module.to_owned(),
        file: String::new(),
        annotation,
        bases,
        members,
        lang: Language::Python,
        change: ChangeKind::Unchanged,
    });
}

fn extract_body(body: Node, src: &str, members: &mut Vec<Member>) {
    let mut cursor = body.walk();
    for stmt in body.named_children(&mut cursor) {
        match stmt.kind() {
            "function_definition" => extract_method(stmt, src, &[], members),
            "decorated_definition" => {
                let decorators = decorator_names(stmt, src);
                if let Some(def) = stmt.child_by_field_name("definition") {
                    if def.kind() == "function_definition" {
                        extract_method(def, src, &decorators, members);
                    }
                }
            }
            "expression_statement" => {
                if let Some(expr) = stmt.named_child(0) {
                    if expr.kind() == "assignment" {
                        extract_field(expr, src, members);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Class-level attribute: `x: int = 0`, `x = 0`, or bare `x: int`.
fn extract_field(assignment: Node, src: &str, members: &mut Vec<Member>) {
    let Some(left) = assignment.child_by_field_name("left") else {
        return;
    };
    if left.kind() != "identifier" {
        return;
    }
    let name = text(left, src);
    if name.starts_with("__") && name.ends_with("__") {
        return; // __slots__, __tablename__, ...
    }
    let ty = assignment
        .child_by_field_name("type")
        .map(|t| clean_type(&text(t, src)));
    push_field(name, ty, text_hash(assignment, src), members);
}

fn extract_method(func: Node, src: &str, decorators: &[String], members: &mut Vec<Member>) {
    let Some(name_node) = func.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let is_dunder = name.starts_with("__") && name.ends_with("__");

    // Instance attributes assigned in __init__ / __post_init__.
    if let Some(body) = func.child_by_field_name("body") {
        if matches!(name.as_str(), "__init__" | "__post_init__") {
            extract_self_assignments(body, src, members);
        }
    }
    if is_dunder {
        return;
    }

    let is_property = decorators.iter().any(|d| d.ends_with("property"));
    let params = func
        .child_by_field_name("parameters")
        .map(|p| parameters(p, src))
        .unwrap_or_default();
    let returns = func
        .child_by_field_name("return_type")
        .map(|r| clean_type(&text(r, src)));

    if is_property {
        push_field(name, returns, def_hash(func, src), members);
        return;
    }

    let mut type_refs: Vec<String> = params.iter().filter_map(|(_, ty)| ty.clone()).collect();
    type_refs.extend(returns.clone());

    members.push(Member {
        visibility: visibility_of(&name),
        param_names: params.iter().map(|(n, _)| n.clone()).collect(),
        detail: render_params(&params),
        returns,
        type_refs,
        is_method: true,
        is_static: decorators
            .iter()
            .any(|d| d.ends_with("staticmethod") || d.ends_with("classmethod")),
        is_abstract: decorators.iter().any(|d| d.contains("abstractmethod")),
        body_hash: def_hash(func, src),
        name,
        ..Default::default()
    });
}

/// [`Member::body_hash`] for a `def`: its text with the docstring left out.
/// A docstring edit is documentation, not a change to what the function
/// does, and reads as noise next to the real changes in a diff (two of the
/// six `✱` methods on kikimora PR #17759 were docstring-only). Comments stay
/// in: they sit among the statements they explain, and separating them is
/// not worth the parser it would take.
fn def_hash(func: Node, src: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let range = func.byte_range();
    let Some(body) = func.child_by_field_name("body") else {
        return text_hash(func, src);
    };
    // Header and body are hashed as two trimmed pieces whether or not there
    // is a docstring, so adding or removing one is not a change either: the
    // whitespace that framed it must not survive as a difference.
    let header = src[range.start..body.start_byte()].trim_end();
    let rest_from = docstring(func).map_or(body.start_byte(), |doc| doc.end);
    let rest = src[rest_from..range.end].trim_start();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    header.hash(&mut hasher);
    rest.hash(&mut hasher);
    hasher.finish()
}

/// The byte range of a `def`'s docstring: a bare string as the first
/// statement of its body.
fn docstring(func: Node) -> Option<std::ops::Range<usize>> {
    let body = func.child_by_field_name("body")?;
    let first = body.named_child(0)?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let expr = first.named_child(0)?;
    (expr.kind() == "string").then(|| first.byte_range())
}

fn push_field(name: String, ty: Option<String>, body_hash: u64, members: &mut Vec<Member>) {
    if members.iter().any(|m| !m.is_method && m.name == name) {
        return;
    }
    members.push(Member {
        visibility: visibility_of(&name),
        detail: ty.clone().unwrap_or_default(),
        type_refs: ty.into_iter().collect(),
        body_hash,
        name,
        ..Default::default()
    });
}

/// Find `self.x = ...` / `self.x: T = ...` assignments (shallow walk of the body).
fn extract_self_assignments(node: Node, src: &str, members: &mut Vec<Member>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "assignment" {
            if let Some(left) = child.child_by_field_name("left") {
                if left.kind() == "attribute" {
                    let obj = left.child_by_field_name("object");
                    let attr = left.child_by_field_name("attribute");
                    if let (Some(obj), Some(attr)) = (obj, attr) {
                        if obj.kind() == "identifier" && text(obj, src) == "self" {
                            let ty = child
                                .child_by_field_name("type")
                                .map(|t| clean_type(&text(t, src)));
                            push_field(text(attr, src), ty, text_hash(child, src), members);
                        }
                    }
                }
            }
        }
        // Don't descend into nested functions/classes; do descend into blocks.
        if !matches!(child.kind(), "function_definition" | "class_definition") {
            extract_self_assignments(child, src, members);
        }
    }
}

/// `(name, type)` per parameter. Types feed the rendered signature and the
/// dependency edges a method implies.
fn parameters(params: Node, src: &str) -> Vec<(String, Option<String>)> {
    let mut cursor = params.walk();
    params
        .named_children(&mut cursor)
        .filter_map(|p| {
            let ty = p
                .child_by_field_name("type")
                .map(|t| clean_type(&text(t, src)));
            match p.kind() {
                "identifier" => Some((text(p, src), None)),
                "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => {
                    let prefix = match p.kind() {
                        "list_splat_pattern" => "*",
                        "dictionary_splat_pattern" => "**",
                        _ => "",
                    };
                    p.named_child(0)
                        .filter(|n| n.kind() == "identifier")
                        .map(|n| (format!("{prefix}{}", text(n, src)), ty))
                }
                "default_parameter" | "typed_default_parameter" => {
                    p.child_by_field_name("name").map(|n| (text(n, src), ty))
                }
                _ => None, // positional/keyword separators
            }
        })
        .filter(|(name, _)| name != "self" && name != "cls")
        .collect()
}

fn render_params(params: &[(String, Option<String>)]) -> String {
    params
        .iter()
        .map(|(name, ty)| match ty {
            Some(ty) => format!("{name}: {ty}"),
            None => name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn visibility_of(name: &str) -> Visibility {
    if name.starts_with("__") {
        Visibility::Private
    } else if name.starts_with('_') {
        Visibility::Protected
    } else {
        Visibility::Public
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> CodeGraph {
        parse("pkg.mod", src).unwrap()
    }

    #[test]
    fn module_functions_collect_into_one_module_box() {
        let g = parse_src(
            r#"
def build(cfg: Config, retries: int = 3) -> Runner:
    return Runner()

async def flush(target: str) -> None:
    pass

def _private_helper() -> None:
    pass

class NotAFunction:
    def method(self) -> None: pass
"#,
        );
        let module = g
            .classes
            .iter()
            .find(|c| c.annotation.as_deref() == Some("module"))
            .expect("a module box for the public module-level functions");
        let names: Vec<&str> = module.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["_private_helper", "build", "flush"],
            "sorted; the private helper is parsed so a diff can see it change"
        );
        let drawn: Vec<&str> = module.drawn_members().map(|m| m.name.as_str()).collect();
        assert_eq!(drawn, vec!["build", "flush"], "but only surface is drawn");
        assert!(module.members.iter().all(|m| m.is_method));
        // The class's own method must not leak into the module box.
        assert!(!names.contains(&"method"));
        let build = module.members.iter().find(|m| m.name == "build").unwrap();
        assert_eq!(build.returns.as_deref(), Some("Runner"));
    }

    #[test]
    fn extracts_class_with_members() {
        let g = parse_src(
            r#"
class Greeter(Base):
    count: int = 0

    def __init__(self, name: str):
        self.name = name

    def greet(self, loud: bool = False) -> str:
        return self.name

    def _helper(self):
        pass
"#,
        );
        assert_eq!(g.classes.len(), 1);
        let c = &g.classes[0];
        assert_eq!(c.qualified, "pkg.mod.Greeter");
        assert_eq!(c.bases.len(), 1);
        assert_eq!(c.bases[0].to, "Base");
        let names: Vec<&str> = c.members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"count"));
        assert!(names.contains(&"name"));
        assert!(names.contains(&"greet"));
        let helper = c.members.iter().find(|m| m.name == "_helper").unwrap();
        assert_eq!(helper.visibility, Visibility::Protected);
    }

    #[test]
    fn detects_stereotypes() {
        let g = parse_src(
            r#"
from abc import ABC, abstractmethod
from enum import Enum
from dataclasses import dataclass

class Color(Enum):
    RED = 1

class Runner(ABC):
    @abstractmethod
    def run(self): ...

@dataclass
class Point:
    x: int
    y: int
"#,
        );
        let by_name = |n: &str| g.classes.iter().find(|c| c.name == n).unwrap();
        assert_eq!(by_name("Color").annotation.as_deref(), Some("enumeration"));
        assert_eq!(by_name("Runner").annotation.as_deref(), Some("abstract"));
        assert_eq!(by_name("Point").annotation.as_deref(), Some("dataclass"));
        assert!(by_name("Runner").members[0].is_abstract);
    }

    #[test]
    fn a_docstring_edit_does_not_change_the_body_hash_but_a_body_edit_does() {
        let hash_of = |src: &str| parse_src(src).classes[0].members[0].body_hash;
        let before =
            "class A:\n    def go(self):\n        \"\"\"Old words.\"\"\"\n        return 1\n";
        let reworded = "class A:\n    def go(self):\n        \"\"\"New words, same code.\"\"\"\n        return 1\n";
        let rewritten =
            "class A:\n    def go(self):\n        \"\"\"Old words.\"\"\"\n        return 2\n";
        let undocumented = "class A:\n    def go(self):\n        return 1\n";
        assert_eq!(
            hash_of(before),
            hash_of(reworded),
            "documentation is not behaviour"
        );
        assert_ne!(hash_of(before), hash_of(rewritten));
        // Adding or removing the docstring is not a change either.
        assert_eq!(hash_of(before), hash_of(undocumented));
        // A string that is not the first statement is code, and counts.
        let with_string_later = "class A:\n    def go(self):\n        x = 1\n        \"\"\"not a docstring\"\"\"\n        return 1\n";
        let with_string_later_changed = "class A:\n    def go(self):\n        x = 1\n        \"\"\"also not\"\"\"\n        return 1\n";
        assert_ne!(
            hash_of(with_string_later),
            hash_of(with_string_later_changed)
        );
    }

    #[test]
    fn a_lowercase_module_assignment_is_kept_aside_not_drawn() {
        let graph = parse(
            "pkg.app",
            "LIMIT = 3\nregistry: dict[str, int] = {}\n_hidden = 1\na, b = 1, 2\nobj.attr = 4\ndef run() -> None: ...\n",
        )
        .unwrap();
        let module = graph.classes.iter().find(|c| c.is_module_box()).unwrap();
        let boxed: Vec<&str> = module.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            boxed,
            ["LIMIT", "run"],
            "only constants by name join the box"
        );
        let values: Vec<&str> = graph.module_values["pkg.app"]
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(values, ["registry"], "public plain names only");
        assert_eq!(graph.module_values["pkg.app"][0].detail, "dict~str, int~");
    }

    #[test]
    fn a_changed_lowercase_value_is_not_a_change() {
        let base = parse("pkg.app", "registry = {}\ndef run() -> None: ...\n").unwrap();
        let head = parse("pkg.app", "registry = {'a': 1}\ndef run() -> None: ...\n").unwrap();
        let diff = crate::diff::diff_graphs(&base, &head);
        assert!(
            !diff.diff_mode(),
            "a value nothing draws cannot change a diagram"
        );
    }

    #[test]
    fn a_value_set_in_another_branch_is_kept_aside_for_a_manifest() {
        let src = "try:\n    import tomllib\nexcept ImportError:\n    tomllib = None\n    FALLBACK = 1\nelse:\n    in_else = 1\nfinally:\n    fin = 2\nif X:\n    a = 1\nelif Y:\n    b = 2\nelse:\n    if Z:\n        nested = 3\n";
        let graph = parse("pkg.app", src).unwrap();
        let mut values: Vec<&str> = graph.module_values["pkg.app"]
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        values.sort();
        assert_eq!(
            values,
            ["FALLBACK", "a", "b", "fin", "in_else", "nested", "tomllib"]
        );
        assert!(
            graph.classes.iter().all(|c| !c.is_module_box()),
            "no branch value joins the box: {:?}",
            graph.classes
        );
    }
}
