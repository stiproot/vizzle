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

use crate::mermaid::member_row;
use crate::model::{Class, CodeGraph, Member};
use crate::parse::module_path;

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

/// The graph key an entry points at. A `module` entry addresses the module box
/// itself; everything else addresses a declaration inside it.
fn qualified_for(entry: &Entry) -> Result<String> {
    let file = entry.file.as_deref().with_context(|| {
        format!(
            "entry `{}` of kind `{}` needs a `file`",
            entry.id, entry.kind
        )
    })?;
    let module = module_path(file);
    if entry.kind == "module" {
        return Ok(module);
    }
    let symbol = entry
        .symbol
        .as_deref()
        .with_context(|| format!("entry `{}` needs a `symbol`", entry.id))?;
    Ok(format!("{module}.{symbol}"))
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
/// by the manifest where it asks for a subset.
fn members_for<'a>(entry: &Entry, class: &'a Class) -> Result<Vec<&'a Member>> {
    let Some(wanted) = entry.functions.as_ref().or(entry.consts.as_ref()) else {
        return Ok(class.members.iter().collect());
    };
    let by_name: HashMap<&str, &Member> =
        class.members.iter().map(|m| (m.name.as_str(), m)).collect();
    wanted
        .iter()
        .map(|name| {
            by_name.get(name.as_str()).copied().with_context(|| {
                format!(
                    "entry `{}`: `{}` is not a public member of {}",
                    entry.id, name, class.qualified
                )
            })
        })
        .collect()
}

/// Render a curated diagram from a manifest and a parsed graph.
///
/// Every entry that names source must resolve: a silently dropped entry would
/// let a rename empty a diagram, which is the drift this mode exists to catch
/// (curated-diagrams.md §3).
pub fn render(manifest: &Manifest, graph: &CodeGraph) -> Result<String> {
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
            let qualified = qualified_for(entry)?;
            let class = by_qualified.get(qualified.as_str()).with_context(|| {
                format!(
                    "entry `{}` resolves to `{}`, which is not in the parsed graph \
                     — was it renamed, or is `{}` outside the scanned tree?",
                    entry.id,
                    qualified,
                    entry.file.as_deref().unwrap_or("?")
                )
            })?;
            for member in members_for(entry, class)? {
                let _ = writeln!(out, "    {}", member_row(member, false));
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
        let Some(realizes) = realized_type(entry, &by_qualified) else {
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
fn realized_type(entry: &Entry, by_qualified: &HashMap<&str, &Class>) -> Option<String> {
    if entry.kind != "const" {
        return None;
    }
    let module = module_path(entry.file.as_deref()?);
    let symbol = entry.symbol.as_deref()?;
    let member = by_qualified
        .get(module.as_str())?
        .members
        .iter()
        .find(|m| m.name == symbol)?;
    (!member.detail.is_empty()).then(|| member.detail.clone())
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
