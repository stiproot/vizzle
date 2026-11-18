//! Go extraction using tree-sitter-go (docs/languages/go.md).
//!
//! A Go type's methods normally live in other files of its package, so a file
//! is parsed into fragments: the types it declares, plus stubs carrying the
//! methods and enumeration constants it contributes to types declared anywhere
//! in the package. [`merge_packages`] joins them once every file is parsed
//! (§4.1), then infers which types implement which interfaces (§5.2).
//!
//! Every type reference is qualified here, through the file's imports, so the
//! resolver can look it up by exact name: Go's names are exact, and the shared
//! "globally unique name" fallback would point `http.Client` at an in-tree
//! `api.Client` (§5.1).

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

use super::{clean_type, text, text_hash};
use crate::model::*;

/// A stub carrying methods (or an explicit `var _ I = …` implementation) for a
/// type declared somewhere in the package. Never survives [`merge_packages`].
const METHODS: &str = "go:methods";
/// A stub carrying the constants of one typed `const` block (§3.1).
const ENUM: &str = "go:enum";
const INTERFACE: &str = "interface";
const CONSTRAINT: &str = "constraint";
const DEFINED: &str = "type";

/// Go's predeclared types: never a box, never qualified into the package.
const PREDECLARED: &[&str] = &[
    "any",
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
];

/// A file the Go tool never builds into its package (§1).
fn build_ignored(source: &str) -> bool {
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("package ") {
            return false;
        }
        if line == "//go:build ignore" || line == "// +build ignore" {
            return true;
        }
    }
    false
}

/// The nearest `go.mod` above a file: `(module directory, module path)`.
pub(crate) fn go_module_of<'m>(
    rel_path: &str,
    manifests: &'m [(String, String)],
) -> Option<(&'m str, String)> {
    manifests
        .iter()
        .filter(|(path, _)| path == "go.mod" || path.ends_with("/go.mod"))
        .filter_map(|(path, contents)| {
            let dir = path
                .strip_suffix("go.mod")
                .unwrap_or("")
                .trim_end_matches('/');
            let ancestor = dir.is_empty() || rel_path.starts_with(&format!("{dir}/"));
            let module = contents.lines().find_map(|line| {
                let rest = line.trim().strip_prefix("module")?;
                let first = rest.split("//").next()?.trim().trim_matches('"');
                (!first.is_empty() && rest.starts_with(char::is_whitespace))
                    .then(|| first.to_owned())
            })?;
            ancestor.then_some((dir, module))
        })
        .max_by_key(|(dir, _)| dir.len())
}

/// A package's import path: its module path joined with its directory below
/// the module root; outside any module, the directory itself (§2).
pub(crate) fn import_path(rel_path: &str, manifests: &[(String, String)], package: &str) -> String {
    let dir = rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    match go_module_of(rel_path, manifests) {
        Some((module_dir, module)) => {
            let below = if module_dir.is_empty() {
                dir
            } else {
                dir.strip_prefix(module_dir)
                    .unwrap_or(dir)
                    .trim_start_matches('/')
            };
            if below.is_empty() {
                module
            } else {
                format!("{module}/{below}")
            }
        }
        None if dir.is_empty() => package.to_owned(),
        None => dir.to_owned(),
    }
}

/// The name a package is referred to by when imported without an alias. Go
/// takes it from the package clause, which only the imported package's files
/// say; this is the convention most paths follow. A wrong guess leaves the
/// reference unresolved, never pointed elsewhere.
fn default_alias(path: &str) -> String {
    let mut parts = path.rsplit('/');
    let mut last = parts.next().unwrap_or(path);
    let is_major =
        |s: &str| s.len() > 1 && s.starts_with('v') && s[1..].chars().all(|c| c.is_ascii_digit());
    if is_major(last) {
        last = parts.next().unwrap_or(last);
    }
    let last = match last.rsplit_once(".v") {
        Some((head, version))
            if !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()) =>
        {
            head
        }
        _ => last,
    };
    let last = last.strip_prefix("go-").unwrap_or(last);
    let last = last.strip_suffix("-go").unwrap_or(last);
    last.to_owned()
}

fn visibility(name: &str) -> Visibility {
    if name.chars().next().is_some_and(char::is_uppercase) {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

fn package_name(root: Node, src: &str) -> String {
    let mut cursor = root.walk();
    let clause = root
        .named_children(&mut cursor)
        .find(|n| n.kind() == "package_clause");
    let Some(clause) = clause else {
        return String::new();
    };
    let mut c = clause.walk();
    let name = clause
        .named_children(&mut c)
        .find(|n| n.kind() == "package_identifier");
    name.map(|n| text(n, src)).unwrap_or_default()
}

pub fn parse(rel_path: &str, source: &str, manifests: &[(String, String)]) -> Result<CodeGraph> {
    if build_ignored(source) {
        return Ok(CodeGraph::default());
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .context("loading go grammar")?;
    let tree = parser
        .parse(source, None)
        .context("tree-sitter failed to parse go source")?;
    let root = tree.root_node();
    let package = package_name(root, source);
    let mut file = File {
        src: source,
        ip: import_path(rel_path, manifests, &package),
        imports: HashMap::new(),
        type_params: HashSet::new(),
        graph: CodeGraph::default(),
        module_members: Vec::new(),
    };
    file.read_imports(root);
    let mut cursor = root.walk();
    for decl in root.named_children(&mut cursor) {
        match decl.kind() {
            "type_declaration" => file.type_declaration(decl),
            "method_declaration" => file.method(decl),
            "function_declaration" => file.function(decl),
            "const_declaration" => file.consts(decl),
            "var_declaration" => file.vars(decl),
            _ => {}
        }
    }
    let File {
        ip,
        mut graph,
        module_members,
        ..
    } = file;
    if !module_members.is_empty() {
        let name = ip.rsplit('/').next().unwrap_or(&ip).to_owned();
        graph.classes.push(class(
            &ip,
            &name,
            Some(MODULE_ANNOTATION),
            module_members,
            Vec::new(),
        ));
    }
    Ok(graph)
}

fn class(
    module: &str,
    name: &str,
    annotation: Option<&str>,
    members: Vec<Member>,
    bases: Vec<Relation>,
) -> Class {
    // A module box is keyed by the import path itself; a type by `<path>.<Name>`.
    let qualified = if annotation == Some(MODULE_ANNOTATION) {
        module.to_owned()
    } else {
        format!("{module}.{name}")
    };
    Class {
        name: name.to_owned(),
        qualified,
        module: module.to_owned(),
        file: String::new(),
        annotation: annotation.map(str::to_owned),
        bases,
        members,
        lang: Language::Go,
        change: ChangeKind::Unchanged,
    }
}

/// One file's parsing state.
struct File<'s> {
    src: &'s str,
    /// The package's import path (§2).
    ip: String,
    /// Local name → import path. `None` marks a dot import, which makes an
    /// unqualified name ambiguous (§5.1).
    imports: HashMap<String, Option<String>>,
    /// Type parameter names in scope: never qualified into the package.
    type_params: HashSet<String>,
    graph: CodeGraph,
    module_members: Vec<Member>,
}

impl<'s> File<'s> {
    fn text(&self, node: Node) -> String {
        text(node, self.src)
    }

    fn read_imports(&mut self, root: Node) {
        let mut specs = Vec::new();
        let mut cursor = root.walk();
        for decl in root.named_children(&mut cursor) {
            if decl.kind() != "import_declaration" {
                continue;
            }
            collect_kind(decl, "import_spec", &mut specs);
        }
        for spec in specs {
            let Some(path) = spec.child_by_field_name("path") else {
                continue;
            };
            let path = self.text(path).trim_matches(['"', '`']).to_owned();
            self.graph.imports.push(Import {
                file: String::new(),
                target: path.clone(),
                lang: Language::Go,
            });
            match spec
                .child_by_field_name("name")
                .map(|n| (n.kind(), self.text(n)))
            {
                Some(("dot", _)) => {
                    self.imports.insert(".".to_owned(), None);
                }
                Some(("blank_identifier", _)) => {}
                Some((_, alias)) => {
                    self.imports.insert(alias, Some(path));
                }
                None => {
                    self.imports.insert(default_alias(&path), Some(path));
                }
            }
        }
    }

    /// `Name` in this package, unless it is predeclared or a type parameter.
    fn qualify_local(&self, name: &str) -> Option<String> {
        (!PREDECLARED.contains(&name) && !self.type_params.contains(name))
            .then(|| format!("{}.{name}", self.ip))
    }

    /// `pkg.Name` through the file's imports; nothing for an unknown alias.
    fn qualify_selector(&self, node: Node) -> Option<String> {
        let package = self.text(node.child_by_field_name("package")?);
        let name = self.text(node.child_by_field_name("name")?);
        let path = self.imports.get(&package)?.as_ref()?;
        Some(format!("{path}.{name}"))
    }

    /// Every type a type expression names, qualified (§5.1).
    fn refs(&self, node: Node, out: &mut Vec<String>) {
        match node.kind() {
            "type_identifier" => out.extend(self.qualify_local(&self.text(node))),
            "qualified_type" => out.extend(self.qualify_selector(node)),
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.refs(child, out);
                }
            }
        }
    }

    fn refs_of(&self, node: Node) -> Vec<String> {
        let mut out = Vec::new();
        self.refs(node, &mut out);
        out.sort();
        out.dedup();
        out
    }

    /// A type expression with every name qualified and spacing fixed, so two
    /// files spelling one type differently (`Lease` in its package, `lease.Lease`
    /// outside) compare equal (§9).
    fn normalized(&self, node: Node) -> String {
        match node.kind() {
            "type_identifier" => {
                let name = self.text(node);
                self.qualify_local(&name).unwrap_or(name)
            }
            "qualified_type" => self
                .qualify_selector(node)
                .unwrap_or_else(|| self.text(node)),
            _ if node.child_count() == 0 => self.text(node),
            _ => {
                let mut cursor = node.walk();
                let parts: Vec<String> = node
                    .children(&mut cursor)
                    .map(|c| self.normalized(c))
                    .collect();
                parts.join(" ")
            }
        }
    }

    /// The qualified type a receiver or an embedded field names, through any
    /// pointer and with type arguments dropped.
    fn base_type(&self, node: Node) -> Option<String> {
        match node.kind() {
            "pointer_type" | "parenthesized_type" => self.base_type(node.named_child(0)?),
            "generic_type" => self.base_type(node.child_by_field_name("type")?),
            "type_identifier" => self.qualify_local(&self.text(node)),
            "qualified_type" => self.qualify_selector(node),
            _ => None,
        }
    }

    fn with_type_params<T>(
        &mut self,
        params: Option<Node>,
        body: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved = self.type_params.clone();
        if let Some(params) = params {
            let mut names = Vec::new();
            collect_kind(params, "type_parameter_declaration", &mut names);
            for decl in names {
                let mut cursor = decl.walk();
                for name in decl.children_by_field_name("name", &mut cursor) {
                    self.type_params.insert(text(name, self.src));
                }
            }
        }
        let out = body(self);
        self.type_params = saved;
        out
    }

    fn type_declaration(&mut self, decl: Node) {
        let mut cursor = decl.walk();
        for spec in decl.named_children(&mut cursor) {
            match spec.kind() {
                "type_spec" => {
                    let params = spec.child_by_field_name("type_parameters");
                    self.with_type_params(params, |file| file.type_spec(spec));
                }
                "type_alias" => self.type_alias(spec),
                _ => {}
            }
        }
    }

    fn type_spec(&mut self, spec: Node) {
        let (Some(name), Some(ty)) = (
            spec.child_by_field_name("name"),
            spec.child_by_field_name("type"),
        ) else {
            return;
        };
        let name = self.text(name);
        let qualified = format!("{}.{name}", self.ip);
        let (annotation, members, bases) = match ty.kind() {
            "struct_type" => {
                let (members, bases) = self.struct_fields(ty, &qualified);
                (None, members, bases)
            }
            "interface_type" => self.interface(ty, &qualified),
            _ => {
                let row = Member {
                    name: "type".to_owned(),
                    detail: clean_type(&self.text(ty)),
                    type_refs: self.refs_of(ty),
                    body_hash: text_hash(spec, self.src),
                    ..Default::default()
                };
                (Some(DEFINED), vec![row], Vec::new())
            }
        };
        let ip = self.ip.clone();
        self.graph
            .classes
            .push(class(&ip, &name, annotation, members, bases));
    }

    fn type_alias(&mut self, spec: Node) {
        let (Some(name), Some(ty)) = (
            spec.child_by_field_name("name"),
            spec.child_by_field_name("type"),
        ) else {
            return;
        };
        let name = self.text(name);
        let row = Member {
            name: "type".to_owned(),
            detail: format!("= {}", clean_type(&self.text(ty))),
            type_refs: self.refs_of(ty),
            body_hash: text_hash(spec, self.src),
            ..Default::default()
        };
        let ip = self.ip.clone();
        self.graph
            .classes
            .push(class(&ip, &name, Some(DEFINED), vec![row], Vec::new()));
    }

    /// Named fields become rows, one per name; an embedded field becomes an
    /// `inherits` relation (§4, §5.1).
    fn struct_fields(&self, ty: Node, qualified: &str) -> (Vec<Member>, Vec<Relation>) {
        let mut fields = Vec::new();
        collect_kind(ty, "field_declaration", &mut fields);
        let mut members = Vec::new();
        let mut bases = Vec::new();
        for field in fields {
            // Only this struct's own fields: an anonymous struct type inside a
            // field holds field declarations of its own.
            if field.parent().and_then(|p| p.parent()).map(|s| s.id()) != Some(ty.id()) {
                continue;
            }
            let Some(field_type) = field.child_by_field_name("type") else {
                continue;
            };
            let mut cursor = field.walk();
            let names: Vec<String> = field
                .children_by_field_name("name", &mut cursor)
                .map(|n| text(n, self.src))
                .collect();
            if names.is_empty() {
                if let Some(target) = self.base_type(field_type) {
                    bases.push(Relation {
                        from: qualified.to_owned(),
                        to: target,
                        kind: RelationKind::Inherits,
                        via: None,
                    });
                }
                continue;
            }
            let detail = clean_type(&self.text(field_type));
            let type_refs = self.refs_of(field_type);
            for name in names {
                members.push(Member {
                    visibility: visibility(&name),
                    name,
                    detail: detail.clone(),
                    type_refs: type_refs.clone(),
                    body_hash: text_hash(field, self.src),
                    ..Default::default()
                });
            }
        }
        (members, bases)
    }

    fn interface(
        &mut self,
        ty: Node,
        qualified: &str,
    ) -> (Option<&'static str>, Vec<Member>, Vec<Relation>) {
        let mut members = Vec::new();
        let mut bases = Vec::new();
        let mut type_set = Vec::new();
        let mut cursor = ty.walk();
        for element in ty.named_children(&mut cursor) {
            match element.kind() {
                "method_elem" => {
                    if let Some(member) = self.signature_member(element, qualified, None) {
                        members.push(member);
                    }
                }
                "type_elem" => {
                    let written = self.text(element);
                    let only = element
                        .named_child(0)
                        .filter(|_| element.named_child_count() == 1);
                    let embedded = only.filter(|n| {
                        matches!(
                            n.kind(),
                            "type_identifier" | "qualified_type" | "generic_type"
                        ) && !written.contains(['|', '~'])
                            && !PREDECLARED.contains(&written.trim())
                    });
                    match embedded.and_then(|n| self.base_type(n)) {
                        Some(target) => bases.push(Relation {
                            from: qualified.to_owned(),
                            to: target,
                            kind: RelationKind::Inherits,
                            via: None,
                        }),
                        None => type_set.push(written),
                    }
                }
                _ => {}
            }
        }
        if type_set.is_empty() {
            return (Some(INTERFACE), members, bases);
        }
        members.insert(
            0,
            Member {
                name: "type".to_owned(),
                detail: clean_type(&type_set.join(" ; ")),
                ..Default::default()
            },
        );
        (Some(CONSTRAINT), members, bases)
    }

    /// A method row from a `method_elem` or a `method_declaration`, recording
    /// its normalized signature under `<owner>::<name>`. A pointer receiver is
    /// recorded with a leading `*`, which §5.2's `via` reads.
    fn signature_member(
        &mut self,
        node: Node,
        owner: &str,
        pointer: Option<bool>,
    ) -> Option<Member> {
        let name = self.text(node.child_by_field_name("name")?);
        let (rendered, names, normalized) = self.parameters(node.child_by_field_name("parameters"));
        let result = node.child_by_field_name("result");
        let (returns, result_norm) = match result {
            None => (None, String::new()),
            Some(r) if r.kind() == "parameter_list" => {
                let (shown, _, norm) = self.parameters(Some(r));
                (Some(clean_type(&shown)), norm.join(","))
            }
            Some(r) => (Some(clean_type(&self.text(r))), self.normalized(r)),
        };
        let mut type_refs = Vec::new();
        if let Some(params) = node.child_by_field_name("parameters") {
            self.refs(params, &mut type_refs);
        }
        if let Some(r) = result {
            self.refs(r, &mut type_refs);
        }
        type_refs.sort();
        type_refs.dedup();
        let signature = format!("({});({result_norm})", normalized.join(","));
        let marker = if pointer == Some(true) { "*" } else { "" };
        self.graph
            .go_signatures
            .insert(format!("{owner}::{name}"), format!("{marker}{signature}"));
        Some(Member {
            visibility: visibility(&name),
            name,
            detail: rendered,
            returns,
            param_names: names,
            type_refs,
            is_method: true,
            body_hash: text_hash(node, self.src),
            ..Default::default()
        })
    }

    /// `(shown, parameter names, normalized types)` for a parameter list; `a, b
    /// int` is two parameters.
    fn parameters(&self, list: Option<Node>) -> (String, Vec<String>, Vec<String>) {
        let Some(list) = list else {
            return (String::new(), Vec::new(), Vec::new());
        };
        let mut shown = Vec::new();
        let mut names = Vec::new();
        let mut normalized = Vec::new();
        let mut cursor = list.walk();
        for param in list.named_children(&mut cursor) {
            let variadic = param.kind() == "variadic_parameter_declaration";
            if param.kind() != "parameter_declaration" && !variadic {
                continue;
            }
            let Some(ty) = param.child_by_field_name("type") else {
                continue;
            };
            let prefix = if variadic { "..." } else { "" };
            let type_shown = format!("{prefix}{}", clean_type(&self.text(ty)));
            let type_norm = format!("{prefix}{}", self.normalized(ty));
            let mut c = param.walk();
            let declared: Vec<String> = param
                .children_by_field_name("name", &mut c)
                .map(|n| text(n, self.src))
                .collect();
            if declared.is_empty() {
                shown.push(type_shown.clone());
                names.push(type_shown);
                normalized.push(type_norm);
            } else {
                for name in declared {
                    shown.push(format!("{name} {type_shown}"));
                    names.push(name);
                    normalized.push(type_norm.clone());
                }
            }
        }
        (shown.join(", "), names, normalized)
    }

    fn method(&mut self, decl: Node) {
        let Some(receiver) = decl
            .child_by_field_name("receiver")
            .and_then(|r| r.named_child(0))
            .and_then(|p| p.child_by_field_name("type"))
        else {
            return;
        };
        let pointer = receiver.kind() == "pointer_type";
        // A generic receiver's arguments (`*Store[K, V]`) name the type's
        // parameters for this method.
        let mut receiver_params = HashSet::new();
        let mut args = Vec::new();
        collect_kind(receiver, "type_identifier", &mut args);
        for arg in args.iter().skip(1) {
            receiver_params.insert(text(*arg, self.src));
        }
        let saved = self.type_params.clone();
        self.type_params.extend(receiver_params);
        let owner = self.base_type(receiver);
        let member = owner
            .as_deref()
            .and_then(|owner| self.signature_member(decl, owner, Some(pointer)));
        self.type_params = saved;
        let (Some(owner), Some(member)) = (owner, member) else {
            return;
        };
        let name = owner.rsplit('.').next().unwrap_or(&owner).to_owned();
        let ip = self.ip.clone();
        self.graph
            .classes
            .push(class(&ip, &name, Some(METHODS), vec![member], Vec::new()));
    }

    fn function(&mut self, decl: Node) {
        let params = decl.child_by_field_name("type_parameters");
        let ip = self.ip.clone();
        if let Some(member) = self.with_type_params(params, |file| {
            file.signature_member(decl, &format!("{ip}#func"), None)
        }) {
            self.module_members.push(member);
        }
    }

    /// Package-level constants join the module box; a typed block of two or
    /// more also makes its type an enumeration (§3.1).
    fn consts(&mut self, decl: Node) {
        let mut specs = Vec::new();
        collect_kind(decl, "const_spec", &mut specs);
        let mut current: Option<String> = None;
        let mut typed: BTreeMap<String, Vec<Member>> = BTreeMap::new();
        for spec in specs {
            let ty = spec.child_by_field_name("type");
            let value = spec.child_by_field_name("value");
            match (ty, value) {
                (Some(t), _) if t.kind() == "type_identifier" => current = Some(self.text(t)),
                (Some(_), _) | (None, Some(_)) => current = None,
                (None, None) => {}
            }
            let mut cursor = spec.walk();
            let names: Vec<String> = spec
                .children_by_field_name("name", &mut cursor)
                .map(|n| text(n, self.src))
                .filter(|n| n != "_")
                .collect();
            for name in names {
                let member = Member {
                    visibility: visibility(&name),
                    detail: current.clone().unwrap_or_default(),
                    body_hash: text_hash(spec, self.src),
                    name,
                    ..Default::default()
                };
                if let Some(t) = &current {
                    typed.entry(t.clone()).or_default().push(Member {
                        detail: String::new(),
                        ..member.clone()
                    });
                }
                self.module_members.push(member);
            }
        }
        let ip = self.ip.clone();
        for (ty, constants) in typed {
            if constants.len() >= 2 {
                self.graph
                    .classes
                    .push(class(&ip, &ty, Some(ENUM), constants, Vec::new()));
            }
        }
    }

    /// Package-level variables join the module box; `var _ I = (*T)(nil)` is an
    /// explicit implementation (§5.1).
    fn vars(&mut self, decl: Node) {
        let mut specs = Vec::new();
        collect_kind(decl, "var_spec", &mut specs);
        for spec in specs {
            let mut cursor = spec.walk();
            let names: Vec<String> = spec
                .children_by_field_name("name", &mut cursor)
                .map(|n| text(n, self.src))
                .collect();
            if names == ["_"] {
                self.assertion(spec);
                continue;
            }
            let ty = spec.child_by_field_name("type");
            for name in names.into_iter().filter(|n| n != "_") {
                self.module_members.push(Member {
                    visibility: visibility(&name),
                    detail: ty.map(|t| clean_type(&self.text(t))).unwrap_or_default(),
                    type_refs: ty.map(|t| self.refs_of(t)).unwrap_or_default(),
                    body_hash: text_hash(spec, self.src),
                    name,
                    ..Default::default()
                });
            }
        }
    }

    fn assertion(&mut self, spec: Node) {
        let Some(interface) = spec
            .child_by_field_name("type")
            .and_then(|t| self.base_type(t))
        else {
            return;
        };
        let Some(value) = spec
            .child_by_field_name("value")
            .and_then(|v| v.named_child(0))
        else {
            return;
        };
        let (implementer, via) = match value.kind() {
            // (*T)(nil)
            "type_conversion_expression" => {
                let Some(ty) = value.child_by_field_name("type") else {
                    return;
                };
                let pointer = self.text(ty).contains('*');
                (
                    self.base_type(ty),
                    if pointer { "pointer" } else { "value" },
                )
            }
            // (*T)(nil) again: without types, tree-sitter cannot tell a
            // conversion from a call, and reads `(*T)` as an expression.
            "call_expression" => {
                let Some(callee) = value
                    .child_by_field_name("function")
                    .filter(|f| f.kind() == "parenthesized_expression")
                    .and_then(|f| f.named_child(0))
                else {
                    return;
                };
                let (target, via) = match callee.kind() {
                    "unary_expression" if self.text(callee).starts_with('*') => {
                        (callee.child_by_field_name("operand"), "pointer")
                    }
                    _ => (Some(callee), "value"),
                };
                (target.and_then(|t| self.expression_type(t)), via)
            }
            // T{} or &T{}
            "composite_literal" => (
                value
                    .child_by_field_name("type")
                    .and_then(|t| self.base_type(t)),
                "value",
            ),
            "unary_expression" => (
                value
                    .child_by_field_name("operand")
                    .filter(|o| o.kind() == "composite_literal")
                    .and_then(|o| o.child_by_field_name("type"))
                    .and_then(|t| self.base_type(t)),
                "pointer",
            ),
            _ => return,
        };
        let Some(implementer) = implementer else {
            return;
        };
        let name = implementer
            .rsplit('.')
            .next()
            .unwrap_or(&implementer)
            .to_owned();
        let module = implementer
            .rsplit_once('.')
            .map(|(m, _)| m)
            .unwrap_or(&self.ip)
            .to_owned();
        let relation = Relation {
            from: implementer.clone(),
            to: interface,
            kind: RelationKind::Implements,
            via: Some(via),
        };
        self.graph.classes.push(class(
            &module,
            &name,
            Some(METHODS),
            Vec::new(),
            vec![relation],
        ));
    }
}

impl File<'_> {
    /// The qualified type an expression names: `T`, or `pkg.T` through the imports.
    fn expression_type(&self, node: Node) -> Option<String> {
        match node.kind() {
            "identifier" => self.qualify_local(&self.text(node)),
            "selector_expression" => {
                let package = self.text(node.child_by_field_name("operand")?);
                let name = self.text(node.child_by_field_name("field")?);
                let path = self.imports.get(&package)?.as_ref()?;
                Some(format!("{path}.{name}"))
            }
            _ => None,
        }
    }
}

/// Every descendant of `node` of `kind`, not descending into a match.
fn collect_kind<'t>(node: Node<'t>, kind: &str, out: &mut Vec<Node<'t>>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == kind {
            out.push(child);
        } else {
            collect_kind(child, kind, out);
        }
    }
}

fn combine(a: u64, b: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (a, b).hash(&mut hasher);
    hasher.finish()
}

/// Add `from` to `into` by name; a name already there (a build-constrained
/// twin, §4.1) keeps one row whose body hash covers both definitions.
fn merge_members(into: &mut Vec<Member>, from: Vec<Member>) {
    for member in from {
        match into
            .iter_mut()
            .find(|m| m.name == member.name && m.is_method == member.is_method)
        {
            Some(existing) => existing.body_hash = combine(existing.body_hash, member.body_hash),
            None => into.push(member),
        }
    }
}

fn merge_bases(into: &mut Vec<Relation>, from: Vec<Relation>) {
    for relation in from {
        if !into
            .iter()
            .any(|r| r.to == relation.to && r.kind == relation.kind)
        {
            into.push(relation);
        }
    }
}

/// Join each Go package's fragments into one box per type and one module box
/// per package (§4.1), then infer implementations (§5.2). Other languages'
/// classes pass through untouched. Fragments arrive in file-path order, which
/// is what makes a twin's combined hash deterministic.
pub(crate) fn merge_packages(graph: &mut CodeGraph) {
    if !graph.classes.iter().any(|c| c.lang == Language::Go) {
        return;
    }
    let (go, mut others): (Vec<Class>, Vec<Class>) = std::mem::take(&mut graph.classes)
        .into_iter()
        .partition(|c| c.lang == Language::Go);
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Class>> = HashMap::new();
    for class in go {
        if !groups.contains_key(&class.qualified) {
            order.push(class.qualified.clone());
        }
        groups
            .entry(class.qualified.clone())
            .or_default()
            .push(class);
    }

    let mut merged = Vec::new();
    for qualified in order {
        let items = groups.remove(&qualified).unwrap_or_default();
        let is_stub = |c: &Class| matches!(c.annotation.as_deref(), Some(METHODS) | Some(ENUM));
        let (stubs, declarations): (Vec<Class>, Vec<Class>) = items.into_iter().partition(is_stub);
        let mut declarations = declarations.into_iter();
        // A method on a type no file of the package declares cannot build; it
        // attaches to nothing (§4).
        let Some(mut base) = declarations.next() else {
            continue;
        };
        for twin in declarations {
            merge_members(&mut base.members, twin.members);
            merge_bases(&mut base.bases, twin.bases);
        }
        let mut methods = Vec::new();
        let mut constants: Option<Vec<Member>> = None;
        for stub in stubs {
            if stub.annotation.as_deref() == Some(ENUM) {
                constants.get_or_insert(stub.members);
            } else {
                merge_members(&mut methods, stub.members);
                merge_bases(&mut base.bases, stub.bases);
            }
        }
        if base.annotation.as_deref() == Some(DEFINED) {
            if let Some(constants) = constants {
                base.annotation = Some("enumeration".to_owned());
                base.members.retain(|m| m.name != "type");
                let mut rows = constants;
                rows.append(&mut base.members);
                base.members = rows;
            }
        }
        if base.is_module_box() {
            base.members.sort_by(|a, b| a.name.cmp(&b.name));
        }
        methods.sort_by(|a, b| a.name.cmp(&b.name));
        merge_members(&mut base.members, methods);
        merged.push(base);
    }
    infer_implementations(&mut merged, &graph.go_signatures);
    others.extend(merged);
    graph.classes = others;
}

/// An interface's methods with their signatures, its embedded interfaces'
/// included; `None` when any of it is unknown (an embedded interface outside
/// the tree, a method without a recorded signature).
fn interface_methods(
    qualified: &str,
    by_name: &HashMap<&str, &Class>,
    signatures: &BTreeMap<String, String>,
    seen: &mut HashSet<String>,
) -> Option<Vec<(String, String)>> {
    if !seen.insert(qualified.to_owned()) {
        return Some(Vec::new());
    }
    let interface = by_name.get(qualified)?;
    if interface.annotation.as_deref() != Some(INTERFACE) {
        return None;
    }
    let mut methods = Vec::new();
    for member in interface.members.iter().filter(|m| m.is_method) {
        let signature = signatures.get(&format!("{qualified}::{}", member.name))?;
        methods.push((member.name.clone(), signature.clone()));
    }
    for base in interface
        .bases
        .iter()
        .filter(|b| b.kind == RelationKind::Inherits)
    {
        methods.extend(interface_methods(&base.to, by_name, signatures, seen)?);
    }
    Some(methods)
}

/// `T ..|> I` for every type in the tree whose pointer method set has all of
/// an in-tree interface's methods, matched by name and normalized signature
/// (§5.2). An unexported method matches only within its own package.
fn infer_implementations(classes: &mut [Class], signatures: &BTreeMap<String, String>) {
    let mut found: Vec<(usize, Relation)> = Vec::new();
    {
        let by_name: HashMap<&str, &Class> =
            classes.iter().map(|c| (c.qualified.as_str(), c)).collect();
        let interfaces: Vec<(&Class, Vec<(String, String)>)> = classes
            .iter()
            .filter(|c| c.annotation.as_deref() == Some(INTERFACE))
            .filter_map(|c| {
                let methods =
                    interface_methods(&c.qualified, &by_name, signatures, &mut HashSet::new())?;
                (!methods.is_empty()).then_some((c, methods))
            })
            .collect();
        for (index, ty) in classes.iter().enumerate() {
            let concrete = matches!(
                ty.annotation.as_deref(),
                None | Some(DEFINED) | Some("enumeration")
            );
            if !concrete {
                continue;
            }
            for (interface, methods) in &interfaces {
                if ty.bases.iter().any(|b| b.to == interface.qualified) {
                    continue;
                }
                let mut pointer = false;
                let satisfied = methods.iter().all(|(name, wanted)| {
                    if visibility(name) == Visibility::Private && ty.module != interface.module {
                        return false;
                    }
                    match signatures.get(&format!("{}::{name}", ty.qualified)) {
                        Some(have) => {
                            let own = have.strip_prefix('*');
                            pointer |= own.is_some();
                            own.unwrap_or(have) == wanted.trim_start_matches('*')
                        }
                        None => false,
                    }
                });
                if satisfied {
                    found.push((
                        index,
                        Relation {
                            from: ty.qualified.clone(),
                            to: interface.qualified.clone(),
                            kind: RelationKind::Implements,
                            via: Some(if pointer { "pointer" } else { "value" }),
                        },
                    ));
                }
            }
        }
    }
    for (index, relation) in found {
        classes[index].bases.push(relation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(files: &[(&str, &str)]) -> CodeGraph {
        let mut all: Vec<(String, String)> = vec![(
            "go.mod".to_owned(),
            "module example.com/demo\n\ngo 1.22\n".to_owned(),
        )];
        all.extend(files.iter().map(|(p, s)| (p.to_string(), s.to_string())));
        let (manifests, sources): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|(p, _)| p.ends_with("go.mod"));
        crate::parse::parse_files_with_manifests(&sources, &manifests).unwrap()
    }

    fn get<'g>(g: &'g CodeGraph, qualified: &str) -> &'g Class {
        g.classes
            .iter()
            .find(|c| c.qualified == qualified)
            .unwrap_or_else(|| {
                panic!(
                    "{qualified} not in {:?}",
                    g.classes.iter().map(|c| &c.qualified).collect::<Vec<_>>()
                )
            })
    }

    fn names(class: &Class) -> Vec<&str> {
        class.members.iter().map(|m| m.name.as_str()).collect()
    }

    #[test]
    fn the_grammar_loads() {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_go::LANGUAGE.into()).unwrap();
        assert!(!p
            .parse("package x\nfunc main() {}\n", None)
            .unwrap()
            .root_node()
            .has_error());
    }

    #[test]
    fn names_follow_the_module_path() {
        assert_eq!(default_alias("gopkg.in/yaml.v3"), "yaml");
        assert_eq!(default_alias("github.com/x/y/v2"), "y");
        assert_eq!(default_alias("github.com/mattn/go-isatty"), "isatty");
        let g = graph(&[("store/store.go", "package store\ntype Store struct{}\n")]);
        let store = get(&g, "example.com/demo/store.Store");
        assert_eq!(store.module, "example.com/demo/store");
    }

    #[test]
    fn methods_from_every_file_of_the_package_join_their_type() {
        let g = graph(&[
            ("store/a.go", "package store\ntype Store struct{ Name string; a, b int }\nfunc (s *Store) Get() string { return \"\" }\n"),
            ("store/b.go", "package store\nfunc (s Store) Len() int { return 0 }\nfunc helper() {}\n"),
            ("store/c.go", "package store\nfunc Exported() {}\n"),
        ]);
        let store = get(&g, "example.com/demo/store.Store");
        assert_eq!(names(store), ["Name", "a", "b", "Get", "Len"]);
        assert_eq!(store.file, "store/a.go");
        let module = get(&g, "example.com/demo/store");
        assert!(module.is_module_box());
        assert_eq!(
            module.name, "store",
            "named by the last path element, not com/…"
        );
        assert_eq!(names(module), ["Exported", "helper"]);
        assert_eq!(g.classes.iter().filter(|c| c.is_module_box()).count(), 1);
    }

    #[test]
    fn a_build_constrained_twin_is_one_member_and_either_change_shows() {
        let src = |body: &str| format!("package proc\nfunc waitExit() int {{ {body} }}\n");
        let base = graph(&[
            ("proc/w_darwin.go", &src("return 1")),
            ("proc/w_linux.go", &src("return 2")),
        ]);
        let module = get(&base, "example.com/demo/proc");
        assert_eq!(names(module), ["waitExit"]);
        let head = graph(&[
            ("proc/w_darwin.go", &src("return 1")),
            ("proc/w_linux.go", &src("return 3")),
        ]);
        let diff = crate::diff::diff_graphs(&base, &head);
        assert!(diff.diff_mode(), "a change to one twin is a change");
    }

    #[test]
    fn a_typed_const_block_makes_an_enumeration_and_an_untyped_one_does_not() {
        let g = graph(&[(
            "cm/c.go",
            "package cm\ntype Color int\nconst (\n Red Color = iota\n Green\n _\n Blue\n)\nconst (\n second = iota\n minute\n)\nfunc (c Color) String() string { return \"\" }\n",
        )]);
        let color = get(&g, "example.com/demo/cm.Color");
        assert_eq!(color.annotation.as_deref(), Some("enumeration"));
        assert_eq!(names(color), ["Red", "Green", "Blue", "String"]);
    }

    #[test]
    fn references_resolve_through_imports_only() {
        let g = graph(&[
            ("api/client.go", "package api\ntype Client struct{}\n"),
            (
                "cm/github.go",
                "package cm\nimport (\n \"net/http\"\n \"example.com/demo/api\"\n)\ntype GitHub struct {\n Client *http.Client\n API *api.Client\n}\n",
            ),
        ]);
        let edges = crate::resolve::resolve_all_relations(&g);
        let targets: Vec<String> = edges
            .iter()
            .filter(|r| r.from == "example.com/demo/cm.GitHub")
            .filter_map(|r| match &r.to {
                crate::resolve::Target::Internal(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            targets,
            ["example.com/demo/api.Client"],
            "http.Client is not api.Client: {edges:?}"
        );
    }

    #[test]
    fn embedding_is_inheritance_and_a_constraint_is_not_an_interface() {
        let g = graph(&[(
            "kv/kv.go",
            "package kv\ntype Base struct{}\ntype Store struct{ *Base; n int }\ntype Closer interface{ Close() error }\ntype RC interface{ Closer; Read() int }\ntype Number interface{ ~int | ~float64 }\n",
        )]);
        let store = get(&g, "example.com/demo/kv.Store");
        assert!(store
            .bases
            .iter()
            .any(|b| b.to == "example.com/demo/kv.Base" && b.kind == RelationKind::Inherits));
        assert_eq!(names(store), ["n"]);
        let rc = get(&g, "example.com/demo/kv.RC");
        assert!(rc
            .bases
            .iter()
            .any(|b| b.to == "example.com/demo/kv.Closer"));
        assert_eq!(
            get(&g, "example.com/demo/kv.Number").annotation.as_deref(),
            Some(CONSTRAINT)
        );
    }

    #[test]
    fn implementation_is_inferred_from_the_pointer_method_set() {
        let g = graph(&[
            ("lease/lease.go", "package lease\nimport \"context\"\ntype Lease struct{}\ntype Reader interface{ Get(ctx context.Context, key string) (*Lease, error) }\n"),
            (
                "store/store.go",
                "package store\nimport (\n \"context\"\n \"example.com/demo/lease\"\n)\ntype Store struct{}\nfunc (s *Store) Get(c context.Context, k string) (*lease.Lease, error) { return nil, nil }\ntype Wrong struct{}\nfunc (w Wrong) Get(c context.Context, k int) (*lease.Lease, error) { return nil, nil }\n",
            ),
        ]);
        let store = get(&g, "example.com/demo/store.Store");
        let edge = store
            .bases
            .iter()
            .find(|b| b.kind == RelationKind::Implements)
            .expect("Store implements Reader");
        assert_eq!(edge.to, "example.com/demo/lease.Reader");
        assert_eq!(edge.via, Some("pointer"));
        assert!(
            get(&g, "example.com/demo/store.Wrong").bases.is_empty(),
            "a different signature is no match"
        );
    }

    #[test]
    fn an_interface_embedding_an_external_one_infers_nothing() {
        let g = graph(&[(
            "s/s.go",
            "package s\nimport \"io\"\ntype RC interface{ io.Closer; Read() int }\ntype F struct{}\nfunc (f *F) Read() int { return 0 }\nfunc (f *F) Close() error { return nil }\n",
        )]);
        assert!(get(&g, "example.com/demo/s.F").bases.is_empty());
    }

    #[test]
    fn an_explicit_assertion_is_drawn() {
        let g = graph(&[(
            "s/s.go",
            "package s\ntype I interface{ M() }\ntype T struct{}\nvar _ I = (*T)(nil)\n",
        )]);
        let t = get(&g, "example.com/demo/s.T");
        let edge = t
            .bases
            .iter()
            .find(|b| b.kind == RelationKind::Implements)
            .unwrap();
        assert_eq!(
            (edge.to.as_str(), edge.via),
            ("example.com/demo/s.I", Some("pointer"))
        );
    }

    #[test]
    fn a_build_ignore_file_is_skipped() {
        let g = graph(&[(
            "s/gen.go",
            "//go:build ignore\n\npackage main\nfunc main() {}\n",
        )]);
        assert!(g.classes.is_empty());
    }
}
