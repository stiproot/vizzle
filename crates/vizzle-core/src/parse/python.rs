//! Python class extraction using tree-sitter-python.

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

use super::{clean_type, text};
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
    collect(
        tree.root_node(),
        source,
        module,
        None,
        &mut graph,
        &mut module_fns,
    );
    super::push_module_box(module, module_fns, Language::Python, &mut graph);
    Ok(graph)
}

/// Walk top-level (and nested) statements, collecting class definitions.
fn collect(
    node: Node,
    src: &str,
    module: &str,
    outer: Option<&str>,
    graph: &mut CodeGraph,
    module_fns: &mut Vec<Member>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "class_definition" => extract_class(child, src, module, outer, &[], graph),
            // Only reached at module level: the walk never descends into a
            // class body (extract_class owns that) or a function body, so any
            // definition seen here is module surface (class.md §2.4).
            "function_definition" => extract_module_function(child, src, &[], module_fns),
            "decorated_definition" => {
                let decorators = decorator_names(child, src);
                if let Some(def) = child.child_by_field_name("definition") {
                    match def.kind() {
                        "class_definition" => {
                            extract_class(def, src, module, outer, &decorators, graph)
                        }
                        "function_definition" => {
                            extract_module_function(def, src, &decorators, module_fns)
                        }
                        _ => {}
                    }
                }
            }
            // A module-level UPPER_SNAKE assignment is a constant by Python's
            // own convention, and the manifest's `consts` list selects from
            // these (curated-diagrams.md §4).
            "expression_statement" => extract_module_const(child, src, module_fns),
            "import_statement" | "import_from_statement" => extract_imports(child, src, graph),
            // Classes (and imports) can hide inside `if TYPE_CHECKING:` etc.
            "if_statement" | "try_statement" | "block" => {
                collect(child, src, module, outer, graph, module_fns)
            }
            _ => {}
        }
    }
}

/// A module-level constant: public, and UPPER_SNAKE, which is how Python says
/// "constant" and what keeps ordinary module-level assignment out of the box.
fn extract_module_const(stmt: Node, src: &str, members: &mut Vec<Member>) {
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
    let name = text(left, src);
    let is_const = !name.starts_with('_')
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name.chars().any(|c| c.is_ascii_uppercase());
    if !is_const {
        return;
    }
    let detail = assign
        .child_by_field_name("type")
        .map(|ty| clean_type(&text(ty, src)))
        .unwrap_or_default();
    members.push(Member {
        name,
        detail,
        ..Default::default()
    });
}

/// A module-level `def`. Private helpers (leading underscore) are not module
/// surface and are dropped; the shape is otherwise a method's.
fn extract_module_function(
    func: Node,
    src: &str,
    decorators: &[String],
    members: &mut Vec<Member>,
) {
    let Some(name_node) = func.child_by_field_name("name") else {
        return;
    };
    if text(name_node, src).starts_with('_') {
        return;
    }
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
        // Nested classes only. The discard buffer is deliberate: this body's
        // `def`s are methods, already taken by extract_body above, and must not
        // also land in the module box.
        collect(body, src, module, Some(&local_name), graph, &mut Vec::new());
    }

    let is_abstract = members.iter().any(|m: &Member| m.is_abstract);
    if is_abstract && annotation.is_none() {
        annotation = Some("abstract".to_owned());
    }

    graph.classes.push(Class {
        name: local_name,
        qualified,
        module: module.to_owned(),
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
    push_field(name, ty, members);
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
        push_field(name, returns, members);
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
        name,
        ..Default::default()
    });
}

fn push_field(name: String, ty: Option<String>, members: &mut Vec<Member>) {
    if members.iter().any(|m| !m.is_method && m.name == name) {
        return;
    }
    members.push(Member {
        visibility: visibility_of(&name),
        detail: ty.clone().unwrap_or_default(),
        type_refs: ty.into_iter().collect(),
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
                            push_field(text(attr, src), ty, members);
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
        assert_eq!(names, vec!["build", "flush"], "sorted, public only");
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
}
