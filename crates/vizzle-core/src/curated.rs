//! Curated diagrams: a person chooses the scope, vizzle fills in the members.
//!
//! See `docs/curated-diagrams.md`. The manifest format is the `gen:c4-code`
//! one already in use by managed documents, read as written so those documents
//! keep working.
//!
//! This module takes the manifest *text* and a parsed graph and returns mermaid.
//! Finding the document, splicing the fence back into it and reporting drift are
//! the CLI's job — the core never touches a file.

use std::collections::HashMap;
use std::fmt::Write;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::mermaid::{member_row, Params};
use crate::model::{Class, CodeGraph, Language, Member};
use crate::parse::{module_path, rust_qualified_prefix};

/// One curated box. Field names match the manifest as authored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub kind: String,
    /// Source file, repo-relative. Absent for `external`.
    #[serde(default)]
    pub file: Option<String>,
    /// Declaration name to extract. Unused by `module` and `external`.
    #[serde(default)]
    pub symbol: Option<String>,
    /// For `module`: which of the module's functions to list.
    #[serde(default)]
    pub functions: Option<Vec<String>>,
    /// For a Python `module`: which module-level constants to list.
    #[serde(default)]
    pub consts: Option<Vec<String>>,
    /// Overrides the stereotype line.
    #[serde(default)]
    pub stereotype: Option<String>,
    /// Curated body text. The *whole* body for `const` and `external`.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default)]
    pub direction: Option<String>,
    pub classes: Vec<Entry>,
    /// `[from, to, arrow|null, label?]` — a null arrow renders the default.
    #[serde(default)]
    pub relations: Vec<Vec<Option<String>>>,
}

/// Kinds whose body is entirely curated: there is no source to read, so a
/// missing symbol is not an error (curated-diagrams.md §4).
fn is_curated_only(kind: &str) -> bool {
    matches!(kind, "const" | "external")
}

/// Rust keys its declarations by `crate::module` (parse/rust.rs), not by the
/// dotted file path Python and TypeScript use.
fn rust_file(file: &str) -> bool {
    Language::from_path(file) == Some(Language::Rust)
}

/// The graph key of the module a file's declarations live under. One rule,
/// shared with `parse`, so a curated entry and the parser agree on the key.
fn module_box_for(file: &str, manifests: &[(String, String)]) -> String {
    if rust_file(file) {
        rust_qualified_prefix(file, manifests).replace("::", ".")
    } else if Language::from_path(file) == Some(Language::Go) {
        // A Go type belongs to its package, keyed by import path, so an entry
        // finds methods declared in any file of it (docs/languages/go.md §8).
        crate::parse::go::import_path(file, manifests, "main")
    } else {
        module_path(file)
    }
}

/// The graph key an entry points at. A `module` entry addresses the module box
/// itself; everything else addresses a declaration inside it.
fn qualified_for(entry: &Entry, manifests: &[(String, String)]) -> Result<String> {
    let file = entry.file.as_deref().with_context(|| {
        format!(
            "entry `{}` of kind `{}` needs a `file`",
            entry.id, entry.kind
        )
    })?;
    if entry.kind == "module" {
        return Ok(module_box_for(file, manifests));
    }
    let symbol = entry
        .symbol
        .as_deref()
        .with_context(|| format!("entry `{}` needs a `symbol`", entry.id))?;
    // A Rust type is keyed `crate::module::Type`; Python and TypeScript key it
    // `<dotted.module>.<Type>`.
    Ok(if rust_file(file) {
        format!("{}::{symbol}", rust_qualified_prefix(file, manifests))
    } else {
        format!("{}.{symbol}", module_box_for(file, manifests))
    })
}

/// Default stereotype when the manifest does not override it. A module carries
/// its file name, which is how the managed documents already read.
fn stereotype(entry: &Entry) -> Option<String> {
    if let Some(explicit) = &entry.stereotype {
        return Some(explicit.clone());
    }
    match entry.kind.as_str() {
        "module" => {
            let file = entry.file.as_deref().unwrap_or("");
            let base = file.rsplit('/').next().unwrap_or(file);
            Some(format!("module {base}"))
        }
        "external" => None,
        // Match the wording the managed documents already carry, so adopting
        // vizzle does not churn every stereotype line.
        "schema" => Some("Effect Schema struct".to_owned()),
        other => Some(other.to_owned()),
    }
}

/// The members to draw for an entry, taken from the parsed class and narrowed
/// by the manifest where it asks for a subset. `values` are the module's
/// assignments that are not constants by name: only a `consts` entry reaches
/// them, because listing one is the author choosing it (curated-diagrams.md §4).
/// `class` is absent for a module whose only surface is such assignments.
fn members_for<'a>(
    entry: &Entry,
    qualified: &str,
    class: Option<&'a Class>,
    values: &'a [Member],
) -> Result<Vec<&'a Member>> {
    let drawn: Vec<&Member> = class
        .map(|c| c.drawn_members().collect())
        .unwrap_or_default();
    let consts = entry.consts.iter().flatten();
    let functions = entry.functions.iter().flatten();
    if consts.clone().chain(functions.clone()).next().is_none() {
        return Ok(drawn);
    }
    // A name the manifest lists explicitly reaches any member of the box, an
    // unexported one included: the module box hides an unchanged private
    // helper from an exhaustive diagram, but listing it is the author choosing
    // it (docs/languages/go.md §8, the same principle as `consts` and D9).
    let by_name: HashMap<&str, &Member> = class
        .map(|c| c.members.iter().map(|m| (m.name.as_str(), m)).collect())
        .unwrap_or_default();
    let value_by_name: HashMap<&str, &Member> =
        values.iter().map(|m| (m.name.as_str(), m)).collect();
    let missing = |name: &String| {
        anyhow::anyhow!(
            "entry `{}`: `{}` is not a public member of {}",
            entry.id,
            name,
            qualified
        )
    };
    // An entry may name both, and the managed documents list constants first.
    // Chaining the two lists, never `.or()`, keeps every const on an entry that
    // also names functions — which is most of them.
    consts
        .map(|name| {
            by_name
                .get(name.as_str())
                .or_else(|| value_by_name.get(name.as_str()))
                .copied()
                .ok_or_else(|| missing(name))
        })
        .chain(functions.map(|name| {
            by_name
                .get(name.as_str())
                .copied()
                .ok_or_else(|| missing(name))
        }))
        .collect()
}

/// Render a curated diagram from a manifest and a parsed graph.
///
/// Every entry that names source must resolve: a silently dropped entry would
/// let a rename empty a diagram, which is the drift this mode exists to catch
/// (curated-diagrams.md §3). `manifests` is the same Cargo context the graph was
/// parsed with, so a Rust entry resolves to the parser's `crate::module::Type`.
pub fn render(
    manifest: &Manifest,
    graph: &CodeGraph,
    manifests: &[(String, String)],
) -> Result<String> {
    let by_qualified: HashMap<&str, &Class> = graph
        .classes
        .iter()
        .map(|c| (c.qualified.as_str(), c))
        .collect();

    let mut out = String::from("classDiagram\n");
    if let Some(direction) = &manifest.direction {
        let _ = writeln!(out, "  direction {direction}\n");
    }

    for entry in &manifest.classes {
        let _ = writeln!(out, "  class {} {{", entry.id);
        if let Some(stereotype) = stereotype(entry) {
            let _ = writeln!(out, "    <<{stereotype}>>");
        }

        if is_curated_only(&entry.kind) {
            if let Some(note) = &entry.note {
                let _ = writeln!(out, "    {note}");
            }
        } else {
            let qualified = qualified_for(entry, manifests)?;
            let values = module_values_for(entry, graph, manifests);
            let found = by_qualified.get(qualified.as_str()).copied();
            // A module whose public surface is only lowercase assignments has no
            // box; its values still answer a `consts` list.
            let class = match found {
                None if entry.kind == "module" && !values.is_empty() => None,
                _ => Some(found.with_context(|| {
                    let how = if rust_file(entry.file.as_deref().unwrap_or("")) {
                        " (a Rust entry resolves by `crate::module::Type`)"
                    } else {
                        ""
                    };
                    format!(
                        "entry `{}` resolves to `{}`, which is not in the parsed graph \
                     — was it renamed, or is `{}` outside the scanned tree?{}",
                        entry.id,
                        qualified,
                        entry.file.as_deref().unwrap_or("?"),
                        how
                    )
                })?),
            };
            for member in members_for(entry, &qualified, class, values)? {
                let _ = writeln!(out, "    {}", member_row(member, false, Params::NamesOnly));
            }
            if let Some(note) = &entry.note {
                let _ = writeln!(out, "    {note}");
            }
        }
        let _ = writeln!(out, "  }}\n");
    }

    // Realization edges are derived, not curated: an entry declared
    // `const claudeStrategy: AgentStrategy` realizes AgentStrategy, and the edge
    // is only drawn if that type is also in the diagram (curated-diagrams.md §4).
    for entry in &manifest.classes {
        let Some(realizes) = realized_type(entry, &by_qualified, graph, manifests) else {
            continue;
        };
        if let Some(target) = entry_id_for(&realizes, &manifest.classes) {
            let _ = writeln!(out, "  {} ..|> {}", entry.id, target);
        }
    }

    for relation in &manifest.relations {
        let (Some(Some(from)), Some(Some(to))) = (relation.first(), relation.get(1)) else {
            bail!("a relation needs at least [from, to]");
        };
        let arrow = relation.get(2).and_then(|a| a.as_deref()).unwrap_or("-->");
        let label = relation.get(3).and_then(|l| l.as_deref());
        match label {
            Some(label) => {
                let _ = writeln!(out, "  {from} {arrow} {to} : {label}");
            }
            None => {
                let _ = writeln!(out, "  {from} {arrow} {to}");
            }
        }
    }
    Ok(out)
}

/// The declared type of a `const` entry, read from the module box member of the
/// same name. Absent when the const has no annotation — nothing to realize.
fn realized_type(
    entry: &Entry,
    by_qualified: &HashMap<&str, &Class>,
    graph: &CodeGraph,
    manifests: &[(String, String)],
) -> Option<String> {
    if entry.kind != "const" {
        return None;
    }
    let module = module_box_for(entry.file.as_deref()?, manifests);
    let symbol = entry.symbol.as_deref()?;
    let boxed = by_qualified
        .get(module.as_str())
        .into_iter()
        .flat_map(|c| c.members.iter());
    let member = boxed
        .chain(graph.module_values.get(&module).into_iter().flatten())
        .find(|m| m.name == symbol)?;
    (!member.detail.is_empty()).then(|| member.detail.clone())
}

/// The module's assignments that are not constants by name (a lowercase Python
/// name), for a `module` entry's `consts` list. Empty for every other kind.
fn module_values_for<'a>(
    entry: &Entry,
    graph: &'a CodeGraph,
    manifests: &[(String, String)],
) -> &'a [Member] {
    match (entry.kind.as_str(), entry.file.as_deref()) {
        ("module", Some(file)) => graph
            .module_values
            .get(&module_box_for(file, manifests))
            .map(Vec::as_slice)
            .unwrap_or_default(),
        _ => &[],
    }
}

/// The diagram id for a type name, if the diagram contains it. An edge to a
/// box that is not drawn would be mermaid inventing a node nobody curated.
fn entry_id_for(type_name: &str, entries: &[Entry]) -> Option<String> {
    let bare = type_name
        .split(['~', '<'])
        .next()
        .unwrap_or(type_name)
        .trim();
    entries
        .iter()
        .find(|e| e.id == bare || e.symbol.as_deref() == Some(bare))
        .map(|e| e.id.clone())
}

/// Parse a manifest, with the JSON error pointed at the manifest rather than
/// the document that carried it.
pub fn parse_manifest(json: &str) -> Result<Manifest> {
    serde_json::from_str(json).context("the gen:c4-code manifest is not valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two modules each declaring `Config`, plus the manifest context that
    /// keys them `demo::a::Config` and `demo::b::Config`.
    fn rust_repo() -> (CodeGraph, Vec<(String, String)>) {
        let files = vec![
            (
                "src/a.rs".to_owned(),
                "pub struct Config { pub a_only: bool }\n".to_owned(),
            ),
            (
                "src/b.rs".to_owned(),
                "pub struct Config { pub b_only: bool }\n".to_owned(),
            ),
        ];
        let manifests = vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"demo\"\n".to_owned(),
        )];
        let graph = crate::parse::parse_files_with_manifests(&files, &manifests).unwrap();
        (graph, manifests)
    }

    fn manifest(json: &str) -> Manifest {
        parse_manifest(json).unwrap()
    }

    fn python_graph(source: &str) -> CodeGraph {
        let files = vec![("app.py".to_owned(), source.to_owned())];
        crate::parse::parse_files_with_manifests(&files, &[]).unwrap()
    }

    #[test]
    fn consts_names_a_lowercase_module_value_explicitly() {
        let graph = python_graph(
            "registry: dict[str, int] = {}\nWELL_KNOWN = 1\nhelper_value = 2\ndef run() -> None: ...\n",
        );
        let manifest = manifest(
            r#"{"classes":[{"id":"App","kind":"module","file":"app.py","consts":["registry","WELL_KNOWN"],"functions":["run"]}]}"#,
        );
        let rendered = render(&manifest, &graph, &[]).unwrap();
        assert!(rendered.contains("registry"), "{rendered}");
        assert!(rendered.contains("WELL_KNOWN"), "{rendered}");
        assert!(
            rendered.find("registry") < rendered.find("+run("),
            "consts first, in the order listed: {rendered}"
        );
        assert!(
            !rendered.contains("helper_value"),
            "only what is listed: {rendered}"
        );
    }

    #[test]
    fn a_module_with_only_lowercase_values_still_answers_consts() {
        let graph = python_graph("settings = {}\n");
        let manifest = manifest(
            r#"{"classes":[{"id":"Cfg","kind":"module","file":"app.py","consts":["settings"]}]}"#,
        );
        let rendered = render(&manifest, &graph, &[]).unwrap();
        assert!(rendered.contains("settings"), "{rendered}");
    }

    #[test]
    fn a_value_is_reached_only_through_consts() {
        let graph = python_graph("settings = {}\ndef run() -> None: ...\n");
        // `functions` is a list of functions; a value named there is a typo.
        let typo = manifest(
            r#"{"classes":[{"id":"App","kind":"module","file":"app.py","functions":["settings"]}]}"#,
        );
        let err = render(&typo, &graph, &[]).unwrap_err();
        assert!(
            err.to_string()
                .contains("`settings` is not a public member"),
            "{err}"
        );
        // And an entry that lists nothing draws the box, which never holds values.
        let unlisted = manifest(r#"{"classes":[{"id":"App","kind":"module","file":"app.py"}]}"#);
        let rendered = render(&unlisted, &graph, &[]).unwrap();
        assert!(!rendered.contains("settings"), "{rendered}");
    }

    #[test]
    fn a_missing_const_still_fails() {
        let graph = python_graph("settings = {}\n");
        let manifest = manifest(
            r#"{"classes":[{"id":"Cfg","kind":"module","file":"app.py","consts":["gone"]}]}"#,
        );
        let err = render(&manifest, &graph, &[]).unwrap_err();
        assert!(
            err.to_string()
                .contains("`gone` is not a public member of app"),
            "{err}"
        );
    }

    #[test]
    fn a_lowercase_const_entry_realizes_its_declared_type() {
        let graph = python_graph("class Strategy: ...\nclaude_strategy: Strategy = Strategy()\n");
        let manifest = manifest(
            r#"{"classes":[
                {"id":"Strategy","kind":"class","file":"app.py","symbol":"Strategy"},
                {"id":"claude","kind":"const","file":"app.py","symbol":"claude_strategy","note":"claude"}
            ]}"#,
        );
        let rendered = render(&manifest, &graph, &[]).unwrap();
        assert!(rendered.contains("claude ..|> Strategy"), "{rendered}");
    }

    #[test]
    fn a_go_entry_resolves_in_its_package_across_files() {
        let files = vec![
            ("store/store.go".to_owned(), "package store\ntype Store struct{ Name string }\nvar registry = map[string]int{}\nfunc Open() {}\n".to_owned()),
            ("store/get.go".to_owned(), "package store\nfunc (s *Store) Get(key string) string { return \"\" }\n".to_owned()),
        ];
        let manifests = vec![("go.mod".to_owned(), "module example.com/demo\n".to_owned())];
        let graph = crate::parse::parse_files_with_manifests(&files, &manifests).unwrap();
        let manifest = manifest(
            r#"{"classes":[
                {"id":"Store","kind":"class","file":"store/store.go","symbol":"Store"},
                {"id":"Pkg","kind":"module","file":"store/store.go","functions":["Open"],"consts":["registry"]}
            ]}"#,
        );
        let rendered = render(&manifest, &graph, &manifests).unwrap();
        assert!(
            rendered.contains("+Get(key)"),
            "the method from get.go: {rendered}"
        );
        assert!(
            rendered.contains("+Open()") && rendered.contains("registry"),
            "{rendered}"
        );
    }

    #[test]
    fn a_rust_entry_resolves_by_file_and_crate_module() {
        let (graph, manifests) = rust_repo();
        let manifest = manifest(
            r#"{"classes":[{"id":"Config","kind":"class","file":"src/a.rs","symbol":"Config"}]}"#,
        );
        let rendered = render(&manifest, &graph, &manifests).unwrap();
        assert!(rendered.contains("a_only"), "{rendered}");
        assert!(
            !rendered.contains("b_only"),
            "the other module's Config: {rendered}"
        );
    }

    #[test]
    fn a_missing_rust_symbol_names_the_rust_key() {
        let (graph, manifests) = rust_repo();
        let manifest = manifest(
            r#"{"classes":[{"id":"Missing","kind":"class","file":"src/a.rs","symbol":"Missing"}]}"#,
        );
        let err = render(&manifest, &graph, &manifests).unwrap_err();
        assert!(err.to_string().contains("demo::a::Missing"), "{err}");
        assert!(err.to_string().contains("not in the parsed graph"), "{err}");
    }
}
