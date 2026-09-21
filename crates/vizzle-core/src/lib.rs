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

/// A file set as the entry points take it: repo-relative `(path, contents)` pairs.
pub type Files = Vec<(String, String)>;

/// File-selection options shared by the high-level entry points.
#[derive(Debug, Clone, Default)]
pub struct SelectOptions {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Language names ("python", "typescript"); empty means all supported.
    pub langs: Vec<String>,
}

impl SelectOptions {
    fn selector(&self) -> Result<walk::Selector> {
        walk::Selector::new(&self.include, &self.exclude, &self.languages()?)
    }

    /// The selection as a reader sees it in a legend or trailer, one entry per
    /// rule; empty when nothing is filtered. Speaks in rule names, not CLI flag
    /// spellings — the core does not know how the CLI spells `-E`.
    pub fn describe(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.extend(self.include.iter().map(|g| format!("include {g}")));
        out.extend(self.exclude.iter().map(|g| format!("exclude {g}")));
        out.extend(self.langs.iter().map(|l| format!("lang {l}")));
        out
    }

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
/// `select` applies to both sides, so the same globs mean the same thing here
/// as on `diagram_from_dir`.
pub fn diff_diagram(
    base_files: &[(String, String)],
    head_files: &[(String, String)],
    select: &SelectOptions,
    render: &RenderOptions,
) -> Result<String> {
    let (base_files, head_files) = select_both(base_files, head_files, select)?;
    let base = parse::parse_files(&base_files)?;
    let head = parse::parse_files(&head_files)?;
    let merged = diff::diff_graphs(&base, &head);
    Ok(mermaid::render(&merged, render))
}

/// Apply one selection to both revisions of a changed-file set. Filtering to
/// nothing is an error rather than an empty diagram: the caller already
/// established there were changed files, so "nothing left" is the selection's
/// doing and the reader should hear that, not see a blank page.
fn select_both(
    base_files: &[(String, String)],
    head_files: &[(String, String)],
    select: &SelectOptions,
) -> Result<(Files, Files)> {
    let selector = select.selector()?;
    let base = selector.filter(base_files);
    let head = selector.filter(head_files);
    if base.is_empty() && head.is_empty() && !(base_files.is_empty() && head_files.is_empty()) {
        anyhow::bail!("no changed files match the include/exclude/lang selection");
    }
    Ok((base, head))
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
    let mut graph = component::build(&files, &manifests)?;
    graph.selection = select.describe();
    Ok(graph)
}

/// Render a change-highlighted component diagram from two full revisions of a
/// repo's sources and manifests. Unlike the class diff, both sides must be the
/// complete file set — an edge's existence depends on files a change didn't touch.
/// `select` applies to BOTH revisions before either graph is built, so a
/// filtered file can never read as added or removed.
pub fn component_diff_diagram(
    base_files: &[(String, String)],
    base_manifests: &[(String, String)],
    head_files: &[(String, String)],
    head_manifests: &[(String, String)],
    select: &SelectOptions,
    scope_path: &str,
    render: &ComponentRenderOptions,
) -> Result<String> {
    let merged = component_diff_graph(
        base_files,
        base_manifests,
        head_files,
        head_manifests,
        select,
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
    select: &SelectOptions,
    scope_path: &str,
    include_classes: bool,
) -> Result<String> {
    let merged = component_diff_graph(
        base_files,
        base_manifests,
        head_files,
        head_manifests,
        select,
        scope_path,
    )?;
    Ok(component::to_json(&merged, include_classes))
}

/// Selection runs BEFORE build and scope runs AFTER diff, deliberately in that
/// order. Selection is what the reader asked not to see, so a filtered
/// component must not survive as a `«boundary»` neighbour — it owns no files,
/// so `build` never creates it and no edge can reach it. Scope, by contrast,
/// needs the full graph on both sides to find the boundary at all
/// (`docs/diagram-types/component.md` §6.1–6.2).
fn component_diff_graph(
    base_files: &[(String, String)],
    base_manifests: &[(String, String)],
    head_files: &[(String, String)],
    head_manifests: &[(String, String)],
    select: &SelectOptions,
    scope_path: &str,
) -> Result<component::ComponentGraph> {
    let selector = select.selector()?.within_scope(scope_path);
    let base = component::build(&selector.filter(base_files), base_manifests)?;
    let mut head = component::build(&selector.filter(head_files), head_manifests)?;
    head.selection = select.describe();
    let merged = component::diff(&base, &head);
    Ok(component::scope(&merged, scope_path))
}

/// Export a change-annotated class graph from two revisions of a file set as JSON.
pub fn json_diff(
    base_files: &[(String, String)],
    head_files: &[(String, String)],
    select: &SelectOptions,
) -> Result<String> {
    let (base_files, head_files) = select_both(base_files, head_files, select)?;
    let base = parse::parse_files(&base_files)?;
    let head = parse::parse_files(&head_files)?;
    let merged = diff::diff_graphs(&base, &head);
    Ok(export::to_json(&merged))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> Files {
        items
            .iter()
            .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
            .collect()
    }

    /// `pkgs/core` imported by `apps/svc`; the fixture under core's tests is a
    /// package of its own and exists at both revisions.
    fn revision(core_body: &str) -> (Files, Files) {
        let manifests = pairs(&[
            ("pkgs/core/package.json", r#"{"name": "@x/core"}"#),
            (
                "pkgs/core/tests/fixtures/repo/package.json",
                r#"{"name": "fixture"}"#,
            ),
            ("apps/svc/package.json", r#"{"name": "svc"}"#),
        ]);
        let files = pairs(&[
            ("pkgs/core/src/index.ts", core_body),
            (
                "pkgs/core/tests/fixtures/repo/index.ts",
                "export class Fixture {}\n",
            ),
            (
                "apps/svc/src/main.ts",
                "import { Core } from \"@x/core\";\nclass Svc {}\n",
            ),
        ]);
        (files, manifests)
    }

    fn select(exclude: &[&str]) -> SelectOptions {
        SelectOptions {
            exclude: exclude.iter().map(|g| (*g).to_owned()).collect(),
            ..SelectOptions::default()
        }
    }

    fn component_paths(json: &str) -> Vec<String> {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        value["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["path"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn selection_drops_a_component_from_both_revisions() {
        let (base_files, base_manifests) = revision("export class Core {}\n");
        let (head_files, head_manifests) = revision("export class Core { x = 1 }\n");
        let json = component_json_diff(
            &base_files,
            &base_manifests,
            &head_files,
            &head_manifests,
            &select(&["**/tests/fixtures/**"]),
            "",
            false,
        )
        .unwrap();
        assert_eq!(component_paths(&json), ["apps/svc", "pkgs/core"]);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Absent on both sides: nothing may read as added or removed.
        for c in value["components"].as_array().unwrap() {
            assert_ne!(c["change"], "added");
            assert_ne!(c["change"], "removed");
        }
        assert_eq!(
            value["stats"]["selection"],
            serde_json::json!(["exclude **/tests/fixtures/**"])
        );
    }

    #[test]
    fn selection_runs_before_scope_so_an_excluded_neighbour_is_not_a_boundary() {
        let (base_files, base_manifests) = revision("export class Core {}\n");
        let (head_files, head_manifests) = revision("export class Core { x = 1 }\n");
        let scoped_only = component_json_diff(
            &base_files,
            &base_manifests,
            &head_files,
            &head_manifests,
            &SelectOptions::default(),
            "pkgs/core",
            false,
        )
        .unwrap();
        assert!(
            component_paths(&scoped_only).contains(&"apps/svc".to_owned()),
            "without a selection svc is kept as core's boundary neighbour"
        );

        let json = component_json_diff(
            &base_files,
            &base_manifests,
            &head_files,
            &head_manifests,
            &select(&["apps/**"]),
            "pkgs/core",
            false,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            component_paths(&json),
            ["pkgs/core", "pkgs/core/tests/fixtures/repo"]
        );
        assert!(value["components"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["boundary"] == false));
        assert_eq!(
            value["edges"].as_array().unwrap().len(),
            0,
            "the edge goes with it"
        );
    }

    #[test]
    fn class_diff_selection_that_filters_everything_is_an_error() {
        let base = pairs(&[("app.py", "class A: ...\n")]);
        let head = pairs(&[("app.py", "class B: ...\n")]);
        let err = json_diff(&base, &head, &select(&["app.py"])).unwrap_err();
        assert!(err.to_string().contains("selection"), "{err}");
        // Two genuinely empty sides are the caller's case to report, not ours.
        assert!(json_diff(&[], &[], &select(&["app.py"])).is_ok());
    }
}
