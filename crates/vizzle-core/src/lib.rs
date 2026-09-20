//! vizzle-core: parse source code into a class graph and render Mermaid
//! class diagrams, with git-diff-aware change highlighting.
//!
//! The typical flow:
//! 1. [`walk::collect_files`] or the caller supplies `(path, contents)` pairs.
//! 2. [`parse::parse_files`] turns them into a [`model::CodeGraph`].
//! 3. Optionally [`diff::diff_graphs`] annotates a base/head pair with changes.
//! 4. [`mermaid::render`] emits the diagram text.

pub mod component;
pub mod curated;
pub mod diff;
pub mod export;
pub mod mermaid;
pub mod model;
pub mod palette;
pub mod parse;
pub mod resolve;
pub mod walk;

use std::path::Path;

use anyhow::Result;

pub use component::ComponentRenderOptions;
pub use mermaid::{Grouping, RenderOptions};

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
///
/// Under [`Grouping::Component`] this also detects components, because
/// ownership comes from package manifests on disk rather than from the class
/// graph. That is why component grouping is available here and not from
/// [`diagram_from_files`], which has no tree to look at.
pub fn diagram_from_dir(
    root: &Path,
    select: &SelectOptions,
    render: &RenderOptions,
) -> Result<String> {
    let files = walk::collect_files(root, &select.include, &select.exclude, &select.languages()?)?;
    if render.grouping != Grouping::Component {
        return diagram_from_files(&files, render);
    }
    let manifests = walk::collect_manifests(root)?;
    let components = component::build(&files, &manifests)?;
    let names: std::collections::HashMap<&str, &str> = components
        .components
        .iter()
        .map(|c| (c.path.as_str(), c.name.as_str()))
        .collect();
    let mut render = render.clone();
    render.component_of = components
        .classes
        .iter()
        .map(|placed| {
            let name = names
                .get(placed.component.as_str())
                .copied()
                .unwrap_or(placed.component.as_str());
            (placed.class.qualified.clone(), name.to_owned())
        })
        .collect();
    diagram_from_files(&files, &render)
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

/// Render a curated diagram (docs/curated-diagrams.md) for the repo at `root`.
pub fn curated_from_dir(root: &Path, select: &SelectOptions, manifest: &str) -> Result<String> {
    let files = walk::collect_files(root, &select.include, &select.exclude, &select.languages()?)?;
    let graph = parse::parse_files(&files)?;
    let manifest = curated::parse_manifest(manifest)?;
    curated::render(&manifest, &graph)
}

/// Export the class graph for every supported source file under `root` as JSON.
pub fn json_from_dir(root: &Path, select: &SelectOptions) -> Result<String> {
    let files = walk::collect_files(root, &select.include, &select.exclude, &select.languages()?)?;
    let graph = parse::parse_files(&files)?;
    Ok(export::to_json(&graph))
}

/// Render a component diagram (modules + dependency edges) for the repo at `root`.
pub fn component_diagram_from_dir(
    root: &Path,
    select: &SelectOptions,
    render: &ComponentRenderOptions,
) -> Result<String> {
    let graph = component_graph_from_dir(root, select)?;
    Ok(component::render_mermaid(&graph, render))
}

/// Export the component graph for the repo at `root` as JSON.
/// `include_classes` carries the per-component class detail for drill-down.
pub fn component_json_from_dir(
    root: &Path,
    select: &SelectOptions,
    include_classes: bool,
) -> Result<String> {
    let graph = component_graph_from_dir(root, select)?;
    Ok(component::to_json(&graph, include_classes))
}

fn component_graph_from_dir(
    root: &Path,
    select: &SelectOptions,
) -> Result<component::ComponentGraph> {
    let files = walk::collect_files(root, &select.include, &select.exclude, &select.languages()?)?;
    let manifests = walk::collect_manifests(root)?;
    component::build(&files, &manifests)
}

/// Render a change-highlighted component diagram from two full revisions of a
/// repo's sources and manifests. Unlike the class diff, both sides must be the
/// complete file set — an edge's existence depends on files a change didn't touch.
pub fn component_diff_diagram(
    base_files: &[(String, String)],
    base_manifests: &[(String, String)],
    head_files: &[(String, String)],
    head_manifests: &[(String, String)],
    scope_path: &str,
    render: &ComponentRenderOptions,
) -> Result<String> {
    let merged = component_diff_graph(
        base_files,
        base_manifests,
        head_files,
        head_manifests,
        scope_path,
    )?;
    Ok(component::render_mermaid(&merged, render))
}

/// Export a change-annotated component graph from two full revisions as JSON.
pub fn component_json_diff(
    base_files: &[(String, String)],
    base_manifests: &[(String, String)],
    head_files: &[(String, String)],
    head_manifests: &[(String, String)],
    scope_path: &str,
    include_classes: bool,
) -> Result<String> {
    let merged = component_diff_graph(
        base_files,
        base_manifests,
        head_files,
        head_manifests,
        scope_path,
    )?;
    Ok(component::to_json(&merged, include_classes))
}

fn component_diff_graph(
    base_files: &[(String, String)],
    base_manifests: &[(String, String)],
    head_files: &[(String, String)],
    head_manifests: &[(String, String)],
    scope_path: &str,
) -> Result<component::ComponentGraph> {
    let base = component::build(base_files, base_manifests)?;
    let head = component::build(head_files, head_manifests)?;
    let merged = component::diff(&base, &head);
    Ok(component::scope(&merged, scope_path))
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
