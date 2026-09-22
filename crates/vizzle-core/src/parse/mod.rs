//! Source parsing via tree-sitter, one extractor per language.

mod python;
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
    let Some(lang) = Language::from_path(rel_path) else {
        return Ok(CodeGraph::default());
    };
    let module = module_path(rel_path);
    let mut graph = match lang {
        Language::Python => python::parse(&module, source),
        Language::TypeScript => typescript::parse(&module, source),
    }?;
    // The parsers see a module, not a path; the file is stamped on here, once,
    // so neither language has to carry it through every extractor.
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
