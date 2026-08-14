//! vizzy-core: parse source code into a class graph and render Mermaid
//! class diagrams, with git-diff-aware change highlighting.
//!
//! The typical flow:
//! 1. [`walk::collect_files`] or the caller supplies `(path, contents)` pairs.
//! 2. [`parse::parse_files`] turns them into a [`model::CodeGraph`].
//! 3. Optionally [`diff::diff_graphs`] annotates a base/head pair with changes.
//! 4. [`mermaid::render`] emits the diagram text.

pub mod diff;
pub mod export;
pub mod mermaid;
pub mod model;
pub mod parse;
pub mod resolve;
pub mod walk;

use std::path::Path;

use anyhow::Result;

pub use mermaid::RenderOptions;

/// File-selection options shared by the high-level entry points.
#[derive(Debug, Clone, Default)]
pub struct SelectOptions {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Language names ("python", "typescript"); empty means all supported.
    pub langs: Vec<String>,
}

impl SelectOptions {
    fn languages(&self) -> Result<Vec<model::Language>> {
        self.langs
            .iter()
            .map(|l| match l.to_ascii_lowercase().as_str() {
                "python" | "py" => Ok(model::Language::Python),
                "typescript" | "ts" | "tsx" => Ok(model::Language::TypeScript),
                other => anyhow::bail!("unsupported language: {other}"),
            })
            .collect()
    }
}

/// Render a class diagram for every supported source file under `root`.
pub fn diagram_from_dir(
    root: &Path,
    select: &SelectOptions,
    render: &RenderOptions,
) -> Result<String> {
    let files = walk::collect_files(root, &select.include, &select.exclude, &select.languages()?)?;
    diagram_from_files(&files, render)
}

/// Render a class diagram from in-memory `(relative_path, contents)` pairs.
pub fn diagram_from_files(files: &[(String, String)], render: &RenderOptions) -> Result<String> {
    let graph = parse::parse_files(files)?;
    Ok(mermaid::render(&graph, render))
}

/// Render a change-highlighted class diagram from two revisions of a file set.
pub fn diff_diagram(
    base_files: &[(String, String)],
    head_files: &[(String, String)],
    render: &RenderOptions,
) -> Result<String> {
    let base = parse::parse_files(base_files)?;
    let head = parse::parse_files(head_files)?;
    let merged = diff::diff_graphs(&base, &head);
    Ok(mermaid::render(&merged, render))
}

/// Export the class graph for every supported source file under `root` as JSON.
pub fn json_from_dir(root: &Path, select: &SelectOptions) -> Result<String> {
    let files = walk::collect_files(root, &select.include, &select.exclude, &select.languages()?)?;
    let graph = parse::parse_files(&files)?;
    Ok(export::to_json(&graph))
}

/// Export a change-annotated class graph from two revisions of a file set as JSON.
pub fn json_diff(
    base_files: &[(String, String)],
    head_files: &[(String, String)],
) -> Result<String> {
    let base = parse::parse_files(base_files)?;
    let head = parse::parse_files(head_files)?;
    let merged = diff::diff_graphs(&base, &head);
    Ok(export::to_json(&merged))
}
