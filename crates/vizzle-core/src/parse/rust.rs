//! Rust source extraction using tree-sitter-rust.

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

use super::{clean_type, text, text_hash};
use crate::model::*;

pub fn parse(module: &str, source: &str) -> Result<CodeGraph> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .context("loading rust grammar")?;
    let tree = parser
        .parse(source, None)
        .context("tree-sitter failed to parse rust source")?;

    let mut graph = CodeGraph::default();
    let mut module_fns = Vec::new();
    let mut impls: Vec<(String, Vec<Member>)> = Vec::new();
    let mut realization_edges: Vec<(String, String)> = Vec::new();

    collect(
        tree.root_node(),
        source,
        module,
        &mut graph,
        &mut module_fns,
        &mut impls,
        &mut realization_edges,
    );

    // Apply impl merging: for each impl T, find the matching type and merge members
    for (self_type, members) in impls {
        if let Some(class) = graph.classes.iter_mut().find(|c| c.qualified == self_type) {
            class.members.extend(members);
        }
    }

    // Add realization edges: for each "impl Trait for Type", add Type ..|> Trait
    for (ty_name, trait_name) in realization_edges {
        if let Some(class) = graph.classes.iter_mut().find(|c| c.qualified == ty_name) {
            class.bases.push(Relation {
                from: ty_name.clone(),
                to: trait_name,
                kind: RelationKind::Implements,
            });
        }
    }

    super::push_module_box(module, module_fns, Language::Rust, &mut graph);
    Ok(graph)
}

/// Walk the tree collecting type declarations, impls, and imports.
fn collect(
    node: Node,
    src: &str,
    module: &str,
    graph: &mut CodeGraph,
    module_fns: &mut Vec<Member>,
    impls: &mut Vec<(String, Vec<Member>)>,
    realization_edges: &mut Vec<(String, String)>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "struct_item" => extract_struct(child, src, module, graph),
            "enum_item" => extract_enum(child, src, module, graph),
            "union_item" => extract_union(child, src, module, graph),
            "trait_item" => extract_trait(child, src, module, graph),
            "type_alias_item" => extract_type_alias(child, src, module, graph),
            "impl_item" => extract_impl(
                child,
                src,
                module,
                graph,
                module_fns,
                impls,
                realization_edges,
            ),
            "use_declaration" => extract_imports(child, src, graph),
            "mod_item" => {
                // For now, don't recursively parse nested modules
                // They should be handled by the file walker
            }
            _ => {}
        }
    }
}

fn extract_struct(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{module}::{name}");

    let mut members = Vec::new();

    // Extract fields from struct body
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for field in body.named_children(&mut cursor) {
            if field.kind() == "field_declaration" {
                extract_field(field, src, &mut members);
            }
        }
    }

    // Sort members by name for determinism
    members.sort_by(|a, b| a.name.cmp(&b.name));

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module.to_owned(),
        file: String::new(),
        name,
        annotation: None,
        bases: Vec::new(),
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_enum(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{module}::{name}");

    let mut members = Vec::new();

    // Extract enum variants
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for variant in body.named_children(&mut cursor) {
            if variant.kind() == "enum_variant" {
                extract_enum_variant(variant, src, &mut members);
            }
        }
    }

    // Sort members by name
    members.sort_by(|a, b| a.name.cmp(&b.name));

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module.to_owned(),
        file: String::new(),
        name,
        annotation: Some("<<enumeration>>".to_owned()),
        bases: Vec::new(),
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_union(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{module}::{name}");

    let mut members = Vec::new();

    // Extract fields from union body
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for field in body.named_children(&mut cursor) {
            if field.kind() == "field_declaration" {
                extract_field(field, src, &mut members);
            }
        }
    }

    members.sort_by(|a, b| a.name.cmp(&b.name));

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module.to_owned(),
        file: String::new(),
        name,
        annotation: Some("<<union>>".to_owned()),
        bases: Vec::new(),
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_trait(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{module}::{name}");

    let mut members = Vec::new();
    let mut bases = Vec::new();

    // Extract trait bounds (super traits)
    if let Some(bounds) = node.child_by_field_name("bounds") {
        let mut cursor = bounds.walk();
        for bound in bounds.named_children(&mut cursor) {
            if bound.kind() == "trait_bound" {
                if let Some(ty) = bound.child_by_field_name("type") {
                    let target = text(ty, src);
                    bases.push(Relation {
                        from: qualified.clone(),
                        to: target,
                        kind: RelationKind::Inherits,
                    });
                }
            }
        }
    }

    // Extract trait items
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for item in body.named_children(&mut cursor) {
            match item.kind() {
                "function_item" => extract_trait_method(item, src, &mut members),
                "type_item" => extract_trait_type(item, src, &mut members),
                _ => {}
            }
        }
    }

    members.sort_by(|a, b| a.name.cmp(&b.name));

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module.to_owned(),
        file: String::new(),
        name,
        annotation: Some("<<interface>>".to_owned()),
        bases,
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_type_alias(node: Node, src: &str, module: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{module}::{name}");

    let mut members = Vec::new();

    // Extract the aliased type
    if let Some(ty) = node.child_by_field_name("type") {
        let ty_str = clean_type(&text(ty, src));
        members.push(Member {
            name: "type".to_owned(),
            detail: ty_str,
            visibility: Visibility::Public,
            ..Default::default()
        });
    }

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module.to_owned(),
        file: String::new(),
        name,
        annotation: Some("<<type>>".to_owned()),
        bases: Vec::new(),
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_field(node: Node, src: &str, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let visibility = extract_visibility(node, src);

    let mut type_refs = Vec::new();
    if let Some(ty) = node.child_by_field_name("type") {
        let ty_str = text(ty, src);
        type_refs = extract_type_refs(&ty_str);
    }

    members.push(Member {
        name,
        detail: type_refs.join(", "),
        visibility,
        type_refs,
        is_method: false,
        is_static: false,
        is_abstract: false,
        body_hash: text_hash(node, src),
        change: ChangeKind::Unchanged,
        ..Default::default()
    });
}

fn extract_enum_variant(node: Node, src: &str, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);

    // For tuple or struct variants, include the payload in detail
    let mut detail = String::new();
    if let Some(body) = node.child_by_field_name("body") {
        if body.kind() == "field_declaration_list" {
            // Struct variant: collect field types
            let mut cursor = body.walk();
            let fields: Vec<String> = body
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "field_declaration")
                .filter_map(|f| f.child_by_field_name("type"))
                .map(|t| clean_type(&text(t, src)))
                .collect();
            detail = fields.join(", ");
        } else if body.kind() == "tuple_struct_body" {
            // Tuple variant: collect tuple element types
            let mut cursor = body.walk();
            let fields: Vec<String> = body
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "type_identifier" || n.kind().ends_with("type"))
                .map(|t| clean_type(&text(t, src)))
                .collect();
            detail = format!("({})", fields.join(", "));
        }
    }

    members.push(Member {
        name,
        detail,
        visibility: Visibility::Public,
        ..Default::default()
    });
}

fn extract_impl(
    node: Node,
    src: &str,
    module: &str,
    _graph: &mut CodeGraph,
    module_fns: &mut Vec<Member>,
    impls: &mut Vec<(String, Vec<Member>)>,
    realization_edges: &mut Vec<(String, String)>,
) {
    // Determine if it's "impl Trait for Type" or just "impl Type"
    let trait_name = node.child_by_field_name("trait");
    let ty_node = node.child_by_field_name("type");

    let self_type = ty_node.map(|t| normalize_type_name(&text(t, src)));
    let trait_ty = trait_name.map(|t| normalize_type_name(&text(t, src)));

    // Extract methods from impl body
    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for item in body.named_children(&mut cursor) {
            if item.kind() == "function_item" {
                extract_method(item, src, &mut members);
            }
        }
    }

    // If it's a trait impl "impl Trait for Type", add a realization edge
    if let (Some(ty), Some(tr)) = (self_type.clone(), trait_ty.clone()) {
        let qualified_type = format!("{module}::{ty}");
        let qualified_trait = format!("{module}::{tr}");
        realization_edges.push((qualified_type, qualified_trait));
    }

    // If it's an inherent impl "impl Type", merge into the type's members
    if let Some(ty) = self_type {
        let qualified = format!("{module}::{ty}");
        impls.push((qualified, members));
    } else if !members.is_empty() {
        // If we can't determine the type, add to module functions
        module_fns.extend(members);
    }
}

fn extract_method(node: Node, src: &str, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let visibility = extract_visibility(node, src);

    // Check for self parameter
    let is_method = has_self_param(node);

    // Extract parameter types and names
    let (param_types, param_names) = extract_parameters(node, src);

    // Extract return type
    let return_type = extract_return_type(node, src);

    // Check if it's abstract (no body in a trait)
    let is_abstract = node.child_by_field_name("body").is_none();

    let detail = format!("({})", param_types.join(", "));
    let mut type_refs = param_types.clone();
    if let Some(ref ret) = return_type {
        type_refs.push(ret.clone());
    }

    members.push(Member {
        name,
        visibility,
        detail,
        returns: return_type,
        type_refs,
        param_names,
        is_method,
        is_static: !is_method,
        is_abstract,
        body_hash: node
            .child_by_field_name("body")
            .map(|b| text_hash(b, src))
            .unwrap_or(0),
        change: ChangeKind::Unchanged,
    });
}

fn extract_imports(node: Node, src: &str, graph: &mut CodeGraph) {
    // Parse the use statement text directly as a fallback approach
    let full_text = text(node, src);

    // Extract "use " followed by the path
    if let Some(rest) = full_text.strip_prefix("use ") {
        // Find the first segment: everything up to ::, {, as, or ;
        let path = rest
            .trim_start_matches("::") // strip leading ::
            .split([':', '{', ';', ' '])
            .next()
            .unwrap_or("")
            .trim();

        if !path.is_empty() && path != "crate" && path != "super" && path != "self" {
            let segments: Vec<&str> = path.split("::").collect();
            if !segments.is_empty() {
                let first = segments[0].trim();
                if !first.is_empty() {
                    graph.imports.push(Import {
                        file: String::new(),
                        target: first.to_owned(),
                        lang: Language::Rust,
                    });
                }
            }
        }
    }
}

fn extract_visibility(node: Node, _src: &str) -> Visibility {
    // Check if there's a visibility modifier
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let vis_text = child.child(0).map(|c| c.kind()).unwrap_or("");
            if vis_text == "pub" {
                // Check if it's "pub" alone or "pub(...)"
                if child.child_by_field_name("path").is_some() {
                    // pub(crate), pub(super), etc.
                    return Visibility::Protected;
                }
                return Visibility::Public;
            }
        }
    }
    Visibility::Private
}

fn has_self_param(node: Node) -> bool {
    let Some(params) = node.child_by_field_name("parameters") else {
        return false;
    };
    let mut cursor = params.walk();
    for p in params.named_children(&mut cursor) {
        if p.kind() == "self_parameter" {
            return true;
        }
    }
    false
}

fn extract_parameters(node: Node, src: &str) -> (Vec<String>, Vec<String>) {
    let Some(params) = node.child_by_field_name("parameters") else {
        return (Vec::new(), Vec::new());
    };

    let mut types = Vec::new();
    let mut names = Vec::new();

    let mut cursor = params.walk();
    for param in params.named_children(&mut cursor) {
        match param.kind() {
            "self_parameter" => {
                // Skip self in the type list but mark it as a method
                names.push("self".to_owned());
            }
            "parameter" => {
                if let Some(name_node) = param.child_by_field_name("name") {
                    let name = text(name_node, src);
                    names.push(name);
                    if let Some(ty) = param.child_by_field_name("type") {
                        let ty_str = clean_type(&text(ty, src));
                        types.push(ty_str);
                    }
                }
            }
            _ => {}
        }
    }

    (types, names)
}

fn extract_return_type(node: Node, src: &str) -> Option<String> {
    let return_type = node.child_by_field_name("return_type")?;
    let ty = return_type.child_by_field_name("type")?;
    Some(clean_type(&text(ty, src)))
}

fn extract_trait_method(node: Node, src: &str, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);

    let (param_types, param_names) = extract_parameters(node, src);
    let return_type = extract_return_type(node, src);
    let is_method = has_self_param(node);
    let is_abstract = node.child_by_field_name("body").is_none();

    let detail = format!("({})", param_types.join(", "));
    let mut type_refs = param_types.clone();
    if let Some(ref ret) = return_type {
        type_refs.push(ret.clone());
    }

    members.push(Member {
        name,
        visibility: Visibility::Public, // Trait items are public
        detail,
        returns: return_type,
        type_refs,
        param_names,
        is_method,
        is_static: !is_method,
        is_abstract,
        body_hash: node
            .child_by_field_name("body")
            .map(|b| text_hash(b, src))
            .unwrap_or(0),
        change: ChangeKind::Unchanged,
    });
}

fn extract_trait_type(node: Node, src: &str, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);

    let detail = if let Some(ty) = node.child_by_field_name("type") {
        clean_type(&text(ty, src))
    } else {
        String::new()
    };

    members.push(Member {
        name,
        visibility: Visibility::Public,
        detail,
        ..Default::default()
    });
}

fn normalize_type_name(ty: &str) -> String {
    // Extract just the base type name, stripping generics and references
    // W<T> -> W
    // &T -> T
    // etc.
    let ty = ty.trim();
    let ty = ty.strip_prefix("&").unwrap_or(ty).trim();
    let ty = ty.strip_prefix("&").unwrap_or(ty).trim(); // &mut
    let ty = ty.strip_prefix("*").unwrap_or(ty).trim();

    if let Some(bracket_pos) = ty.find('<') {
        ty[..bracket_pos].to_owned()
    } else {
        ty.to_owned()
    }
}

fn extract_type_refs(ty_str: &str) -> Vec<String> {
    // Extract type names from a type string, filtering through wrappers
    let mut refs = Vec::new();

    // Simple extraction: find valid identifiers that look like type names
    let mut current = String::new();
    let mut in_generic = false;

    for ch in ty_str.chars() {
        match ch {
            '<' => {
                in_generic = true;
            }
            '>' if in_generic => {
                in_generic = false;
            }
            ':' if !current.is_empty() => {
                current.push(ch);
            }
            _ if ch.is_alphanumeric() || ch == '_' => {
                current.push(ch);
            }
            _ => {
                if !current.is_empty() && current.chars().next().unwrap_or('_').is_alphabetic() {
                    refs.push(current.clone());
                }
                current.clear();
            }
        }
    }

    if !current.is_empty() && current.chars().next().unwrap_or('_').is_alphabetic() {
        refs.push(current);
    }

    refs
}
