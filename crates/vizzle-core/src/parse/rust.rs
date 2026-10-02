//! Rust source extraction using tree-sitter-rust.

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

use super::{clean_type, text, text_hash};
use crate::model::*;

pub fn parse(rel_path: &str, source: &str, manifests: &[(String, String)]) -> Result<CodeGraph> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .context("loading rust grammar")?;
    let tree = parser
        .parse(source, None)
        .context("tree-sitter failed to parse rust source")?;

    let qualified_prefix = super::rust_qualified_prefix(rel_path, manifests);
    let module_dotted = qualified_prefix.replace("::", ".");

    let mut graph = CodeGraph::default();
    let mut module_fns = Vec::new();
    let mut impls: Vec<(String, Vec<Member>)> = Vec::new();
    let mut realization_edges: Vec<(String, String)> = Vec::new();

    collect(
        tree.root_node(),
        source,
        &qualified_prefix,
        &mut graph,
        &mut module_fns,
        &mut impls,
        &mut realization_edges,
    );

    // Apply impl merging: for each impl T, find the matching type and merge members
    for (self_type, members) in impls {
        if let Some(class) = graph.classes.iter_mut().find(|c| c.qualified == self_type) {
            class.members.extend(members);
            class.members.sort_by(|a, b| a.name.cmp(&b.name));
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

    super::push_module_box(&module_dotted, module_fns, Language::Rust, &mut graph);
    Ok(graph)
}

/// Walk the tree collecting type declarations, impls, and imports.
fn collect(
    node: Node,
    src: &str,
    qualified_prefix: &str,
    graph: &mut CodeGraph,
    module_fns: &mut Vec<Member>,
    impls: &mut Vec<(String, Vec<Member>)>,
    realization_edges: &mut Vec<(String, String)>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "struct_item" => extract_struct(child, src, qualified_prefix, graph),
            "enum_item" => extract_enum(child, src, qualified_prefix, graph),
            "union_item" => extract_union(child, src, qualified_prefix, graph),
            "trait_item" => extract_trait(child, src, qualified_prefix, graph),
            "type_item" => extract_type_alias(child, src, qualified_prefix, graph),
            "impl_item" => extract_impl(
                child,
                src,
                qualified_prefix,
                graph,
                module_fns,
                impls,
                realization_edges,
            ),
            "use_declaration" => extract_imports(child, src, graph),
            "mod_item" => {
                // Recurse into inline modules; file-backed `mod foo;` (no body) is
                // handled by the file walker and must not be visited here.
                if let Some(body) = child.child_by_field_name("body") {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let mod_name = text(name_node, src);
                        let sub_prefix = format!("{qualified_prefix}::{mod_name}");
                        collect(
                            body,
                            src,
                            &sub_prefix,
                            graph,
                            module_fns,
                            impls,
                            realization_edges,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_struct(node: Node, src: &str, qualified_prefix: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{qualified_prefix}::{name}");
    let module_dotted = qualified_prefix.replace("::", ".");

    let mut members = Vec::new();

    if let Some(body) = node.child_by_field_name("body") {
        match body.kind() {
            "field_declaration_list" => {
                let mut cursor = body.walk();
                for field in body.named_children(&mut cursor) {
                    if field.kind() == "field_declaration" {
                        extract_field(field, src, &mut members);
                    }
                }
            }
            "ordered_field_declaration_list" => {
                // Tuple struct: fields named by ordinal
                let mut cursor = body.walk();
                let mut ordinal = 0usize;
                for field in body.named_children(&mut cursor) {
                    if field.kind() == "ordered_field_declaration_list" {
                        // skip nested list nodes
                        continue;
                    }
                    let visibility = extract_visibility(field, src);
                    let ty_str = text(field, src);
                    let type_refs = extract_type_refs(&ty_str);
                    members.push(Member {
                        name: ordinal.to_string(),
                        detail: rust_clean_type(&ty_str),
                        visibility,
                        type_refs,
                        is_method: false,
                        is_static: false,
                        is_abstract: false,
                        body_hash: text_hash(field, src),
                        change: ChangeKind::Unchanged,
                        ..Default::default()
                    });
                    ordinal += 1;
                }
            }
            _ => {}
        }
    }

    members.sort_by(|a, b| a.name.cmp(&b.name));

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module_dotted,
        file: String::new(),
        name,
        annotation: None,
        bases: Vec::new(),
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_enum(node: Node, src: &str, qualified_prefix: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{qualified_prefix}::{name}");
    let module_dotted = qualified_prefix.replace("::", ".");

    let mut members = Vec::new();

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for variant in body.named_children(&mut cursor) {
            if variant.kind() == "enum_variant" {
                extract_enum_variant(variant, src, &mut members);
            }
        }
    }

    members.sort_by(|a, b| a.name.cmp(&b.name));

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module_dotted,
        file: String::new(),
        name,
        annotation: Some("enumeration".to_owned()),
        bases: Vec::new(),
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_union(node: Node, src: &str, qualified_prefix: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{qualified_prefix}::{name}");
    let module_dotted = qualified_prefix.replace("::", ".");

    let mut members = Vec::new();

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
        module: module_dotted,
        file: String::new(),
        name,
        annotation: Some("union".to_owned()),
        bases: Vec::new(),
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_trait(node: Node, src: &str, qualified_prefix: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{qualified_prefix}::{name}");
    let module_dotted = qualified_prefix.replace("::", ".");

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
                "function_item" | "function_signature_item" => {
                    extract_trait_method(item, src, &mut members);
                }
                "associated_type" | "type_item" => extract_trait_type(item, src, &mut members),
                _ => {}
            }
        }
    }

    members.sort_by(|a, b| a.name.cmp(&b.name));

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module_dotted,
        file: String::new(),
        name,
        annotation: Some("interface".to_owned()),
        bases,
        members,
        lang: Language::Rust,
        change: ChangeKind::Unchanged,
    });
}

fn extract_type_alias(node: Node, src: &str, qualified_prefix: &str, graph: &mut CodeGraph) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let qualified = format!("{qualified_prefix}::{name}");
    let module_dotted = qualified_prefix.replace("::", ".");

    let mut members = Vec::new();

    if let Some(ty) = node.child_by_field_name("type") {
        let ty_str = text(ty, src);
        members.push(Member {
            name: "type".to_owned(),
            detail: rust_clean_type(&ty_str),
            visibility: Visibility::Public,
            ..Default::default()
        });
    }

    graph.classes.push(Class {
        qualified: qualified.clone(),
        module: module_dotted,
        file: String::new(),
        name,
        annotation: Some("type".to_owned()),
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

    let (detail, type_refs) = if let Some(ty) = node.child_by_field_name("type") {
        let ty_str = text(ty, src);
        (rust_clean_type(&ty_str), extract_type_refs(&ty_str))
    } else {
        (String::new(), Vec::new())
    };

    members.push(Member {
        name,
        detail,
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

    let mut detail = String::new();
    if let Some(body) = node.child_by_field_name("body") {
        match body.kind() {
            "field_declaration_list" => {
                // Struct variant: collect field types
                let mut cursor = body.walk();
                let fields: Vec<String> = body
                    .named_children(&mut cursor)
                    .filter(|n| n.kind() == "field_declaration")
                    .filter_map(|f| f.child_by_field_name("type"))
                    .map(|t| rust_clean_type(&text(t, src)))
                    .collect();
                detail = fields.join(", ");
            }
            "ordered_field_declaration_list" => {
                // Tuple variant: collect element types via the `type` field
                let mut cursor = body.walk();
                let fields: Vec<String> = body
                    .named_children(&mut cursor)
                    .map(|t| rust_clean_type(&text(t, src)))
                    .filter(|s| !s.is_empty())
                    .collect();
                detail = format!("({})", fields.join(", "));
            }
            _ => {}
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
    qualified_prefix: &str,
    _graph: &mut CodeGraph,
    module_fns: &mut Vec<Member>,
    impls: &mut Vec<(String, Vec<Member>)>,
    realization_edges: &mut Vec<(String, String)>,
) {
    let trait_name = node.child_by_field_name("trait");
    let ty_node = node.child_by_field_name("type");

    let self_type = ty_node.map(|t| normalize_type_name(&text(t, src)));
    let trait_ty = trait_name.map(|t| normalize_type_name(&text(t, src)));

    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for item in body.named_children(&mut cursor) {
            if item.kind() == "function_item" {
                extract_method(item, src, &mut members);
            }
        }
    }

    if let (Some(ty), Some(tr)) = (self_type.clone(), trait_ty.clone()) {
        let qualified_type = format!("{qualified_prefix}::{ty}");
        let qualified_trait = format!("{qualified_prefix}::{tr}");
        realization_edges.push((qualified_type, qualified_trait));
    }

    // Only merge inherent impls (not trait impls) into the type's members
    if let Some(ty) = self_type {
        if trait_ty.is_none() {
            let qualified = format!("{qualified_prefix}::{ty}");
            impls.push((qualified, members));
        }
    } else if !members.is_empty() {
        module_fns.extend(members);
    }
}

fn extract_method(node: Node, src: &str, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let visibility = extract_visibility(node, src);
    let has_self = has_self_param(node);

    let (param_types, param_names) = extract_parameters(node, src);
    let return_type = extract_return_type(node, src);
    let is_abstract = node.child_by_field_name("body").is_none();

    let detail = format!("({})", param_types.join(", "));
    let mut type_refs = param_types.clone();
    if let Some(ref ret) = return_type {
        type_refs.extend(extract_type_refs(ret));
    }

    members.push(Member {
        name,
        visibility,
        detail,
        returns: return_type,
        type_refs,
        param_names,
        is_method: true,
        is_static: !has_self,
        is_abstract,
        body_hash: node
            .child_by_field_name("body")
            .map(|b| text_hash(b, src))
            .unwrap_or(0),
        change: ChangeKind::Unchanged,
    });
}

fn extract_imports(node: Node, src: &str, graph: &mut CodeGraph) {
    // Walk the use declaration tree to find the first external crate segment.
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(krate) = use_first_segment(child, src) {
            if !matches!(krate.as_str(), "crate" | "super" | "self") {
                graph.imports.push(Import {
                    file: String::new(),
                    target: krate,
                    lang: Language::Rust,
                });
            }
        }
    }
}

/// Recursively find the leftmost identifier in a use path — the external crate name.
fn use_first_segment(node: Node, src: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(text(node, src)),
        "scoped_identifier" => {
            if let Some(path) = node.child_by_field_name("path") {
                use_first_segment(path, src)
            } else {
                node.child_by_field_name("name").map(|n| text(n, src))
            }
        }
        "scoped_use_list" => node
            .child_by_field_name("path")
            .and_then(|p| use_first_segment(p, src)),
        _ => None,
    }
}

fn extract_visibility(node: Node, _src: &str) -> Visibility {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            // Bare `pub` has no named children; `pub(crate)`, `pub(super)`, etc. do.
            if child.named_child_count() > 0 {
                return Visibility::Protected;
            }
            return Visibility::Public;
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
                names.push("self".to_owned());
            }
            "parameter" => {
                if let Some(name_node) = param
                    .child_by_field_name("pattern")
                    .or_else(|| param.child_by_field_name("name"))
                {
                    let name = text(name_node, src);
                    names.push(name);
                    if let Some(ty) = param.child_by_field_name("type") {
                        let ty_str = text(ty, src);
                        types.push(rust_clean_type(&ty_str));
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
    // Try the `type` field first (tree-sitter-rust ≥ 0.23); fall back to first child.
    let ty_text = if let Some(t) = return_type.child_by_field_name("type") {
        text(t, src)
    } else {
        let mut cursor = return_type.walk();
        let first = return_type.named_children(&mut cursor).next()?;
        text(first, src)
    };
    Some(rust_clean_type(&ty_text))
}

fn extract_trait_method(node: Node, src: &str, members: &mut Vec<Member>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);

    let (param_types, param_names) = extract_parameters(node, src);
    let return_type = extract_return_type(node, src);
    let has_self = has_self_param(node);
    let is_abstract = node.child_by_field_name("body").is_none();

    let detail = format!("({})", param_types.join(", "));
    let mut type_refs = param_types.clone();
    if let Some(ref ret) = return_type {
        type_refs.extend(extract_type_refs(ret));
    }

    members.push(Member {
        name,
        visibility: Visibility::Public,
        detail,
        returns: return_type,
        type_refs,
        param_names,
        is_method: true,
        is_static: !has_self,
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

    let detail = if let Some(ty) = node
        .child_by_field_name("type")
        .or_else(|| node.child_by_field_name("bounds"))
    {
        rust_clean_type(&text(ty, src))
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
    let ty = ty.trim();
    let ty = ty.strip_prefix("&mut").map(str::trim).unwrap_or(ty);
    let ty = ty.strip_prefix('&').map(str::trim).unwrap_or(ty);
    let ty = ty.strip_prefix('*').map(str::trim).unwrap_or(ty);
    // Strip lifetime annotations like `'a`
    let ty = strip_lifetimes(ty);
    let ty = ty.trim().to_owned();
    if let Some(bracket_pos) = ty.find('<') {
        ty[..bracket_pos].trim().to_owned()
    } else {
        ty
    }
}

/// Remove lifetime annotations (`'a`, `'static`, etc.) from a type string.
fn strip_lifetimes(ty: &str) -> String {
    let mut result = String::new();
    let mut chars = ty.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            // Consume the lifetime identifier
            while chars
                .peek()
                .is_some_and(|&c| c.is_alphanumeric() || c == '_')
            {
                chars.next();
            }
            // Skip trailing whitespace after lifetime
            while chars.peek() == Some(&' ') {
                chars.next();
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Rust-specific type cleaner: strip lifetimes then apply the shared clean_type.
fn rust_clean_type(raw: &str) -> String {
    clean_type(&strip_lifetimes(raw))
}

fn extract_type_refs(ty_str: &str) -> Vec<String> {
    // Tokenise on generic delimiters and path separators; emit only nominal
    // identifiers that start with an uppercase letter (probable type names).
    let mut refs = Vec::new();
    let mut current = String::new();

    for ch in ty_str.chars() {
        match ch {
            '<' | '>' | ',' | '(' | ')' | '[' | ']' | '&' | '*' | ' ' | '\t' | '\n' => {
                if !current.is_empty() {
                    let candidate = current.trim_matches(':').to_owned();
                    if candidate.chars().next().is_some_and(|c| c.is_uppercase()) {
                        refs.push(candidate);
                    }
                    current.clear();
                }
            }
            ':' => {
                // `::` path separator: flush current (it's a module segment) and start fresh
                if !current.is_empty() {
                    current.clear();
                }
            }
            _ if ch.is_alphanumeric() || ch == '_' => {
                current.push(ch);
            }
            '\'' => {
                // Skip lifetime annotations by discarding current
                current.clear();
            }
            _ => {
                current.clear();
            }
        }
    }

    if !current.is_empty() {
        let candidate = current.trim_matches(':').to_owned();
        if candidate.chars().next().is_some_and(|c| c.is_uppercase()) {
            refs.push(candidate);
        }
    }

    refs
}

#[cfg(test)]
mod tests {
    use crate::parse::parse_file_with_manifests;

    fn fake_manifests(crate_dir: &str, crate_name: &str) -> Vec<(String, String)> {
        vec![(
            format!("{crate_dir}/Cargo.toml"),
            format!("[package]\nname = \"{crate_name}\"\n"),
        )]
    }

    fn parse_rust(
        crate_dir: &str,
        crate_name: &str,
        file_stem: &str,
        src: &str,
    ) -> crate::model::CodeGraph {
        let path = format!("{crate_dir}/src/{file_stem}.rs");
        parse_file_with_manifests(&path, src, &fake_manifests(crate_dir, crate_name)).unwrap()
    }

    #[test]
    fn grammar_loads() {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        assert!(p.parse("fn main() {}", None).is_some());
    }

    #[test]
    fn enum_gets_enumeration_annotation() {
        let src = "pub enum Color { Red, Green, Blue }";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph
            .classes
            .iter()
            .find(|c| c.name == "Color")
            .expect("Color box");
        assert_eq!(class.annotation.as_deref(), Some("enumeration"));
        assert_eq!(class.qualified, "mycrate::Color");
        assert_eq!(class.members.len(), 3);
    }

    #[test]
    fn impl_merges_into_type() {
        let src = r#"
pub enum Language { Python, Rust }
impl Language {
    pub fn name(&self) -> &'static str { "" }
    pub fn from_path(path: &str) -> Option<Self> { None }
}
"#;
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph
            .classes
            .iter()
            .find(|c| c.name == "Language")
            .expect("Language box");
        assert!(
            class
                .members
                .iter()
                .any(|m| m.name == "name" && m.is_method),
            "name method should be merged"
        );
        assert!(
            class
                .members
                .iter()
                .any(|m| m.name == "from_path" && m.is_static),
            "from_path static method should be merged"
        );
    }

    #[test]
    fn struct_and_enum_counts() {
        let src = "
pub struct A {} pub struct B {} pub struct C {}
pub enum X {} pub enum Y {}";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let structs: Vec<_> = graph
            .classes
            .iter()
            .filter(|c| c.annotation.is_none())
            .collect();
        let enums: Vec<_> = graph
            .classes
            .iter()
            .filter(|c| c.annotation.as_deref() == Some("enumeration"))
            .collect();
        assert_eq!(structs.len(), 3, "3 structs");
        assert_eq!(enums.len(), 2, "2 enums");
    }

    #[test]
    fn derive_creates_no_realization_edge() {
        let src = "#[derive(Debug, Clone)]\npub struct Foo {}";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph.classes.iter().find(|c| c.name == "Foo").expect("Foo");
        assert!(
            class.bases.is_empty(),
            "derive must not create realization edges"
        );
    }

    #[test]
    fn trait_impl_creates_realization_edge() {
        let src = "
pub trait Greet { fn hello(&self); }
pub struct Person;
impl Greet for Person { fn hello(&self) {} }";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let person = graph
            .classes
            .iter()
            .find(|c| c.name == "Person")
            .expect("Person");
        assert!(
            person.bases.iter().any(|r| r.to.contains("Greet")),
            "Person should have a realization edge to Greet"
        );
    }

    #[test]
    fn type_alias_gets_type_annotation() {
        let src = "pub type Files = Vec<(String, String)>;";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph
            .classes
            .iter()
            .find(|c| c.name == "Files")
            .expect("Files");
        assert_eq!(class.annotation.as_deref(), Some("type"));
        assert_eq!(class.qualified, "mycrate::Files");
    }

    #[test]
    fn tuple_struct_has_ordinal_members() {
        let src = "pub struct Pair(pub i32, pub String);";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph
            .classes
            .iter()
            .find(|c| c.name == "Pair")
            .expect("Pair");
        let names: Vec<&str> = class.members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"0"), "field 0 present: {:?}", names);
        assert!(names.contains(&"1"), "field 1 present: {:?}", names);
    }

    #[test]
    fn tuple_enum_variant_has_detail() {
        let src = "pub enum Foo { Tuple(i32, String), Unit }";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph.classes.iter().find(|c| c.name == "Foo").expect("Foo");
        let tuple_variant = class
            .members
            .iter()
            .find(|m| m.name == "Tuple")
            .expect("Tuple variant");
        assert!(
            !tuple_variant.detail.is_empty(),
            "tuple variant should have payload detail: {:?}",
            tuple_variant.detail
        );
    }

    #[test]
    fn generic_field_extracts_inner_type_ref() {
        let src = "
pub struct Member {}
pub struct Class { pub members: Vec<Member> }";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph
            .classes
            .iter()
            .find(|c| c.name == "Class")
            .expect("Class");
        let field = class
            .members
            .iter()
            .find(|m| m.name == "members")
            .expect("members field");
        assert!(
            field.type_refs.iter().any(|r| r == "Member"),
            "Member should be in type_refs: {:?}",
            field.type_refs
        );
    }

    #[test]
    fn qualified_name_uses_crate_prefix() {
        let src = "pub struct Foo {}";
        let graph = parse_rust("mycrate", "mycrate", "model", src);
        let class = graph.classes.iter().find(|c| c.name == "Foo").expect("Foo");
        assert_eq!(class.qualified, "mycrate::model::Foo");
        assert_eq!(class.module, "mycrate.model");
    }

    #[test]
    fn visibility_pub_crate_is_protected() {
        let src = "pub struct Foo { pub(crate) x: i32 }";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph.classes.iter().find(|c| c.name == "Foo").expect("Foo");
        let field = class.members.iter().find(|m| m.name == "x").expect("x");
        assert_eq!(field.visibility, crate::model::Visibility::Protected);
    }

    #[test]
    fn inline_mod_items_are_extracted() {
        let src = "mod inner { pub struct Bar; }";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        assert!(
            graph.classes.iter().any(|c| c.name == "Bar"),
            "Bar from inline mod should be extracted"
        );
    }

    #[test]
    fn trait_items_are_interface_box() {
        let src = "pub trait Serializer { type Ok; fn serialize_bool(&self, v: bool); }";
        let graph = parse_rust("mycrate", "mycrate", "lib", src);
        let class = graph
            .classes
            .iter()
            .find(|c| c.name == "Serializer")
            .expect("Serializer");
        assert_eq!(class.annotation.as_deref(), Some("interface"));
        assert!(
            class.members.iter().any(|m| m.name == "Ok"),
            "associated type Ok should be present"
        );
        assert!(
            class.members.iter().any(|m| m.name == "serialize_bool"),
            "serialize_bool method should be present"
        );
    }
}
