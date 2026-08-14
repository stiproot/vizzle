//! TypeScript class/interface/enum extraction using tree-sitter-typescript.

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

use super::{clean_type, text};
use crate::model::*;

pub fn parse(module: &str, source: &str) -> Result<CodeGraph> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .context("loading typescript grammar")?;
    let tree = parser
        .parse(source, None)
        .context("tree-sitter failed to parse typescript source")?;

    let mut graph = CodeGraph::default();
    collect(tree.root_node(), source, module, &mut graph);
    Ok(graph)
}

fn collect(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "class_declaration" | "abstract_class_declaration" => {
                extract_class(child, src, module, graph)
            }
            "interface_declaration" => extract_interface(child, src, module, graph),
            "enum_declaration" => extract_enum(child, src, module, graph),
            // Unwrap `export class ...`, namespaces, and top-level statements.
            "export_statement"
            | "internal_module"
            | "module"
            | "statement_block"
            | "ambient_declaration" => collect(child, src, module, graph),
            _ => {}
        }
    }
}

fn qualified(module: &str, name: &str) -> String {
    format!("{module}.{name}")
}

fn extract_class(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = qualified(module, &name);

    let mut bases = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "class_heritage" {
            continue;
        }
        let mut hc = child.walk();
        for clause in child.named_children(&mut hc) {
            let kind = match clause.kind() {
                "extends_clause" => RelationKind::Inherits,
                "implements_clause" => RelationKind::Implements,
                _ => continue,
            };
            let mut cc = clause.walk();
            for ty in clause.named_children(&mut cc) {
                if ty.kind() == "type_arguments" {
                    continue;
                }
                bases.push(Relation {
                    from: qualified.clone(),
                    to: bare_type_name(&text(ty, src)),
                    kind,
                });
            }
        }
    }

    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        extract_class_body(body, src, &mut members);
    }

    let is_abstract = node.kind() == "abstract_class_declaration";
    graph.classes.push(Class {
        qualified,
        module: module.to_owned(),
        annotation: is_abstract.then(|| "abstract".to_owned()),
        bases,
        members,
        lang: Language::TypeScript,
        change: ChangeKind::Unchanged,
        name,
    });
}

fn extract_interface(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = qualified(module, &name);

    let mut bases = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "extends_type_clause" {
            let mut cc = child.walk();
            for ty in child.named_children(&mut cc) {
                bases.push(Relation {
                    from: qualified.clone(),
                    to: bare_type_name(&text(ty, src)),
                    kind: RelationKind::Inherits,
                });
            }
        }
    }

    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut bc = body.walk();
        for item in body.named_children(&mut bc) {
            match item.kind() {
                "property_signature" => extract_property(item, src, &mut members),
                "method_signature" => extract_method(item, src, false, &mut members),
                _ => {}
            }
        }
    }

    graph.classes.push(Class {
        qualified,
        module: module.to_owned(),
        annotation: Some("interface".to_owned()),
        bases,
        members,
        lang: Language::TypeScript,
        change: ChangeKind::Unchanged,
        name,
    });
}

fn extract_enum(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut bc = body.walk();
        for item in body.named_children(&mut bc) {
            let variant = match item.kind() {
                "property_identifier" => Some(text(item, src)),
                "enum_assignment" => item.child_by_field_name("name").map(|n| text(n, src)),
                _ => None,
            };
            if let Some(variant) = variant {
                members.push(Member {
                    visibility: Visibility::Public,
                    detail: String::new(),
                    returns: None,
                    is_method: false,
                    is_static: false,
                    is_abstract: false,
                    change: ChangeKind::Unchanged,
                    name: variant,
                });
            }
        }
    }
    graph.classes.push(Class {
        qualified: qualified(module, &name),
        module: module.to_owned(),
        annotation: Some("enumeration".to_owned()),
        bases: Vec::new(),
        members,
        lang: Language::TypeScript,
        change: ChangeKind::Unchanged,
        name,
    });
}

fn extract_class_body(body: Node, src: &str, members: &mut Vec<Member>) {
    let mut cursor = body.walk();
    for item in body.named_children(&mut cursor) {
        match item.kind() {
            "method_definition" => extract_method(item, src, false, members),
            "abstract_method_signature" => extract_method(item, src, true, members),
            "public_field_definition" | "property_signature" => {
                extract_property(item, src, members)
            }
            _ => {}
        }
    }
}

fn extract_method(node: Node, src: &str, is_abstract: bool, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    if name == "constructor" {
        // Parameter properties (`constructor(private x: T)`) become fields.
        if let Some(params) = node.child_by_field_name("parameters") {
            let mut pc = params.walk();
            for p in params.named_children(&mut pc) {
                if matches!(p.kind(), "required_parameter" | "optional_parameter")
                    && p.child(0)
                        .is_some_and(|c| c.kind() == "accessibility_modifier")
                {
                    if let Some(pattern) = p.child_by_field_name("pattern") {
                        members.push(Member {
                            visibility: modifier_visibility(p, src),
                            detail: p
                                .child_by_field_name("type")
                                .map(|t| clean_type(text(t, src).trim_start_matches(':').trim()))
                                .unwrap_or_default(),
                            returns: None,
                            is_method: false,
                            is_static: false,
                            is_abstract: false,
                            change: ChangeKind::Unchanged,
                            name: text(pattern, src),
                        });
                    }
                }
            }
        }
        return;
    }

    let params = node
        .child_by_field_name("parameters")
        .map(|p| parameter_names(p, src))
        .unwrap_or_default();
    let returns = node
        .child_by_field_name("return_type")
        .map(|r| clean_type(text(r, src).trim_start_matches(':').trim()));

    members.push(Member {
        visibility: visibility_of(node, &name, src),
        detail: params.join(", "),
        returns,
        is_method: true,
        is_static: has_keyword(node, "static", src),
        is_abstract,
        change: ChangeKind::Unchanged,
        name,
    });
}

fn extract_property(node: Node, src: &str, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let ty = node
        .child_by_field_name("type")
        .map(|t| clean_type(text(t, src).trim_start_matches(':').trim()));
    members.push(Member {
        visibility: visibility_of(node, &name, src),
        detail: ty.unwrap_or_default(),
        returns: None,
        is_method: false,
        is_static: has_keyword(node, "static", src),
        is_abstract: false,
        change: ChangeKind::Unchanged,
        name,
    });
}

fn parameter_names(params: Node, src: &str) -> Vec<String> {
    let mut cursor = params.walk();
    params
        .named_children(&mut cursor)
        .filter_map(|p| p.child_by_field_name("pattern").map(|n| text(n, src)))
        .collect()
}

fn bare_type_name(raw: &str) -> String {
    let cleaned = clean_type(raw);
    cleaned
        .split('~')
        .next()
        .unwrap_or(&cleaned)
        .rsplit('.')
        .next()
        .unwrap_or(&cleaned)
        .trim()
        .to_owned()
}

fn has_keyword(node: Node, keyword: &str, src: &str) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .take_while(|c| c.kind() != "formal_parameters" && c.kind() != "statement_block")
        .any(|c| !c.is_named() && text(c, src) == keyword);
    found
}

fn modifier_visibility(node: Node, src: &str) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "accessibility_modifier" {
            return match text(child, src).as_str() {
                "private" => Visibility::Private,
                "protected" => Visibility::Protected,
                _ => Visibility::Public,
            };
        }
    }
    Visibility::Public
}

fn visibility_of(node: Node, name: &str, src: &str) -> Visibility {
    if name.starts_with('#') {
        return Visibility::Private;
    }
    modifier_visibility(node, src)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> CodeGraph {
        parse("web.src.app", src).unwrap()
    }

    #[test]
    fn extracts_class_interface_enum() {
        let g = parse_src(
            r#"
export interface Shape {
    area(): number;
}

export abstract class Base {
    protected id: string;
    abstract describe(): string;
}

export class Circle extends Base implements Shape {
    constructor(private radius: number) { super(); }
    area(): number { return Math.PI * this.radius ** 2; }
    static unit(): Circle { return new Circle(1); }
}

export enum Color { Red, Green = "g" }
"#,
        );
        let by_name = |n: &str| g.classes.iter().find(|c| c.name == n).unwrap();
        assert_eq!(by_name("Shape").annotation.as_deref(), Some("interface"));
        assert_eq!(by_name("Base").annotation.as_deref(), Some("abstract"));
        assert_eq!(by_name("Color").annotation.as_deref(), Some("enumeration"));
        assert_eq!(by_name("Color").members.len(), 2);

        let circle = by_name("Circle");
        assert_eq!(circle.bases.len(), 2);
        let radius = circle.members.iter().find(|m| m.name == "radius").unwrap();
        assert_eq!(radius.visibility, Visibility::Private);
        assert!(
            circle
                .members
                .iter()
                .find(|m| m.name == "unit")
                .unwrap()
                .is_static
        );

        let id = by_name("Base")
            .members
            .iter()
            .find(|m| m.name == "id")
            .unwrap();
        assert_eq!(id.visibility, Visibility::Protected);
    }
}
