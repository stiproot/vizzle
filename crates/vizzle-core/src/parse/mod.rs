//! Source parsing via tree-sitter, one extractor per language.

mod python;
mod typescript;

use anyhow::Result;
use rayon::prelude::*;

use crate::model::{CodeGraph, Language};

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
    let Some(lang) = Language::from_path(rel_path) else {
        return Ok(CodeGraph::default());
    };
    let module = module_path(rel_path);
    let mut graph = match lang {
        Language::Python => python::parse(&module, source),
        Language::TypeScript => typescript::parse(&module, source),
    }?;
    for import in &mut graph.imports {
        import.file = rel_path.to_owned();
    }
    Ok(graph)
}

/// Parse many `(relative_path, contents)` pairs in parallel into one graph.
pub fn parse_files(files: &[(String, String)]) -> Result<CodeGraph> {
    let fragments: Vec<CodeGraph> = files
        .par_iter()
        .map(|(path, src)| parse_file(path, src))
        .collect::<Result<_>>()?;
    let mut graph = CodeGraph::default();
    for fragment in fragments {
        graph.merge(fragment);
    }
    graph.normalize();
    Ok(graph)
}

/// Shared helper: node text as owned string.
pub(crate) fn text(node: tree_sitter::Node, src: &str) -> String {
    src[node.byte_range()].to_owned()
}

/// Compact a type expression for display inside a mermaid member row.
pub(crate) fn clean_type(raw: &str) -> String {
    // `dict[str, int]` -> `dict~str, int~` (mermaid generics), but keep
    // literal `[]` array suffixes (`string[]` renders fine as-is).
    let mut cleaned: String = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("[]", "\u{1}")
        .replace(['[', ']'], "~")
        .replace('\u{1}', "[]")
        .replace(['"', '\'', '{', '}', '(', ')', '`', ';'], "");
    if cleaned.chars().count() > 40 {
        cleaned = cleaned.chars().take(39).collect();
        cleaned.push('…');
    }
    cleaned
}
