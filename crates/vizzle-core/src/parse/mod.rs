//! Source parsing via tree-sitter, one extractor per language.

mod python;
mod rust;
mod typescript;

use anyhow::Result;
use rayon::prelude::*;

use crate::model::{ChangeKind, Class, CodeGraph, Language, Member, MODULE_ANNOTATION};

/// One `<<module>>` box per module holding its exported module-level functions
/// (class.md §2.4). Sorted, because diagram output must be diffable.
pub(super) fn push_module_box(
    module: &str,
    mut members: Vec<Member>,
    lang: Language,
    graph: &mut CodeGraph,
) {
    if members.is_empty() {
        return;
    }
    members.sort_by(|a, b| a.name.cmp(&b.name));
    let name = module.rsplit('.').next().unwrap_or(module).to_owned();
    graph.classes.push(Class {
        qualified: module.to_owned(),
        module: module.to_owned(),
        file: String::new(),
        annotation: Some(MODULE_ANNOTATION.to_owned()),
        bases: Vec::new(),
        members,
        lang,
        change: ChangeKind::Unchanged,
        name,
    });
}

/// Derive a dotted module path from a repo-relative file path.
///
/// `apps/dapr-agent/src/main.py` -> `apps.dapr-agent.src.main`
/// `pkg/__init__.py`             -> `pkg`
/// `web/src/index.ts`            -> `web.src.index`
pub fn module_path(rel_path: &str) -> String {
    let no_ext = rel_path
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(rel_path);
    let dotted = no_ext.replace(['/', '\\'], ".");
    dotted
        .strip_suffix(".__init__")
        .map(str::to_owned)
        .unwrap_or(dotted)
}

/// Parse a single file's contents into graph fragments.
pub fn parse_file(rel_path: &str, source: &str) -> Result<CodeGraph> {
    parse_file_with_manifests(rel_path, source, &[])
}

/// Parse a single file with Cargo manifest context for Rust qualified-name derivation.
pub fn parse_file_with_manifests(
    rel_path: &str,
    source: &str,
    manifests: &[(String, String)],
) -> Result<CodeGraph> {
    let Some(lang) = Language::from_path(rel_path) else {
        return Ok(CodeGraph::default());
    };
    let mut graph = match lang {
        Language::Python => {
            let module = module_path(rel_path);
            python::parse(&module, source)
        }
        Language::Rust => rust::parse(rel_path, source, manifests),
        Language::TypeScript => {
            let module = module_path(rel_path);
            typescript::parse(&module, source)
        }
    }?;
    // Stamp the file path on every import and class — the parsers see a module, not a path.
    for import in &mut graph.imports {
        import.file = rel_path.to_owned();
    }
    for class in &mut graph.classes {
        class.file = rel_path.to_owned();
    }
    Ok(graph)
}

/// Parse many `(relative_path, contents)` pairs in parallel into one graph.
pub fn parse_files(files: &[(String, String)]) -> Result<CodeGraph> {
    parse_files_with_manifests(files, &[])
}

/// Parse many files with Cargo manifest context for Rust qualified-name derivation.
pub fn parse_files_with_manifests(
    files: &[(String, String)],
    manifests: &[(String, String)],
) -> Result<CodeGraph> {
    let fragments: Vec<CodeGraph> = files
        .par_iter()
        .map(|(path, src)| parse_file_with_manifests(path, src, manifests))
        .collect::<Result<_>>()?;
    let mut graph = CodeGraph::default();
    for fragment in fragments {
        graph.merge(fragment);
    }
    graph.normalize();
    Ok(graph)
}

/// Compute the Rust-specific qualified prefix for a source file.
///
/// Returns `crate_ident::module::path` (double-colon) derived from the nearest
/// `Cargo.toml` ancestor; falls back to the generic dotted module path with
/// dots replaced by `::` when no manifest matches.
pub fn rust_qualified_prefix(rel_path: &str, manifests: &[(String, String)]) -> String {
    // Find the nearest (longest directory path) Cargo.toml that is an ancestor of rel_path.
    let best = manifests
        .iter()
        .filter(|(mp, _)| mp.ends_with("Cargo.toml"))
        .filter_map(|(mp, contents)| {
            let dir = mp.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let is_ancestor = if dir.is_empty() {
                true
            } else {
                rel_path.starts_with(&format!("{dir}/"))
            };
            if is_ancestor {
                Some((dir.to_owned(), contents.as_str()))
            } else {
                None
            }
        })
        .max_by_key(|(dir, _)| dir.len());

    let Some((manifest_dir, contents)) = best else {
        // No manifest found: convert dotted module path to :: notation as fallback.
        return module_path(rel_path).replace('.', "::");
    };

    // Extract `[package] name` and normalize `-` → `_`.
    let crate_ident = toml_package_name(contents)
        .map(|n| n.replace('-', "_"))
        .unwrap_or_else(|| {
            manifest_dir
                .rsplit('/')
                .next()
                .unwrap_or(&manifest_dir)
                .replace('-', "_")
        });

    // Path relative to the crate root, then strip `src/` prefix if present.
    let rel_to_crate = if manifest_dir.is_empty() {
        rel_path.to_owned()
    } else {
        rel_path
            .strip_prefix(&format!("{manifest_dir}/"))
            .unwrap_or(rel_path)
            .to_owned()
    };
    let rel_to_src = rel_to_crate.strip_prefix("src/").unwrap_or(&rel_to_crate);

    // Strip `.rs` extension.
    let stem = rel_to_src
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(rel_to_src);

    // Crate-root files (`lib.rs`, `main.rs`) have no module path segment.
    if stem == "lib" || stem == "main" {
        return crate_ident;
    }

    // `foo/mod.rs` → module `foo` (not `foo::mod`).
    let stem = stem.strip_suffix("/mod").unwrap_or(stem);

    let module_segments = stem.replace('/', "::");
    format!("{crate_ident}::{module_segments}")
}

/// Minimal scan for `name = "..."` inside `[package]` in a Cargo.toml.
fn toml_package_name(contents: &str) -> Option<String> {
    let mut in_package = false;
    for line in contents.lines() {
        let line = line.trim();
        if line == "[package]" {
            in_package = true;
        } else if line.starts_with('[') {
            in_package = false;
        } else if in_package {
            if let Some(rest) = line.strip_prefix("name") {
                let rest = rest.trim_start();
                if let Some(value) = rest.strip_prefix('=') {
                    return Some(value.trim().trim_matches(['"', '\'']).to_owned());
                }
            }
        }
    }
    None
}

/// Shared helper: node text as owned string.
pub(crate) fn text(node: tree_sitter::Node, src: &str) -> String {
    src[node.byte_range()].to_owned()
}

/// Hash of a node's source text, for [`Member::body_hash`]. The text is
/// hashed as written: whitespace is significant in Python, and a reformatted
/// or re-documented member *did* change, which the diff should say rather
/// than guess at intent.
pub(crate) fn text_hash(node: tree_sitter::Node, src: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    src[node.byte_range()].hash(&mut hasher);
    hasher.finish()
}

/// Compact a type expression for display inside a mermaid member row.
pub(crate) fn clean_type(raw: &str) -> String {
    // `dict[str, int]` -> `dict~str, int~` (mermaid generics), but keep
    // literal `[]` array suffixes (`string[]` renders fine as-is).
    //
    // TypeScript's `Foo<Bar>` needs the same treatment: a raw `<` in a
    // classDiagram member is read as markup and kills the whole diagram, which
    // is why every class diagram this tool emitted failed to render in mmdc
    // until 2026-08-17.
    let mut cleaned: String = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("[]", "\u{1}")
        // `=>` in a function type is not a generic bracket; protect the arrow
        // before the angle brackets are rewritten, or `A => B` becomes `A =~ B`.
        .replace("=>", "\u{2}")
        .replace(['[', ']', '<', '>'], "~")
        .replace('\u{2}', "=>")
        .replace('\u{1}', "[]")
        .replace(['"', '\'', '{', '}', '(', ')', '`', ';'], "");
    if cleaned.chars().count() > 40 {
        cleaned = cleaned.chars().take(39).collect();
        cleaned.push('…');
    }
    cleaned
}
