//! Component graph: build-level modules of a repository and the dependency
//! edges between them, derived from imports.
//!
//! Detection is manifest-driven with a directory fallback (see
//! `docs/diagram-types/component.md`): a directory holding a package manifest
//! is a component; the innermost manifest above a source file owns it; files
//! under no manifest fall back to their top-level directory. Imports become
//! edges only when they resolve to another detected component.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;
use std::hash::{Hash, Hasher};

use serde_json::{json, Value};

use crate::mermaid::{escape_label, sanitize_id};
use crate::model::{ChangeKind, CodeGraph, Import, Language};
use crate::parse;

/// Manifest file names that mark a directory as a component, in priority
/// order when one directory holds several.
pub const MANIFEST_NAMES: [&str; 4] = ["package.json", "pyproject.toml", "Cargo.toml", "go.mod"];

#[derive(Debug, Clone)]
pub struct Component {
    /// Display name: manifest name if declared, else the directory name
    /// (possibly disambiguated with a parent-directory prefix).
    pub name: String,
    /// Name declared by the package manifest, if any. Bare TypeScript import
    /// specifiers match against this, never against the display name.
    pub manifest_name: Option<String>,
    /// Repo-relative directory; `""` for the synthetic root component.
    pub path: String,
    /// Parent path relative to the repo root; `""` for depth-1 components.
    pub group: String,
    pub langs: Vec<&'static str>,
    pub files: usize,
    pub classes: usize,
    /// Hash of the owned files (paths + contents), for diff detection.
    pub fingerprint: u64,
    pub change: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeTarget {
    /// Path of a component in the graph.
    Component(String),
    /// External package name (npm / PyPI / stdlib), kept only for `--externals`.
    External(String),
}

#[derive(Debug, Clone)]
pub struct ComponentEdge {
    /// Path of the importing component.
    pub from: String,
    pub to: EdgeTarget,
    /// Number of distinct importing files.
    pub weight: usize,
    pub change: ChangeKind,
}

#[derive(Debug, Clone, Default)]
pub struct ComponentGraph {
    pub components: Vec<Component>,
    pub edges: Vec<ComponentEdge>,
}

impl ComponentGraph {
    fn normalize(&mut self) {
        self.components.sort_by(|a, b| a.path.cmp(&b.path));
        self.edges
            .sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    }

    pub fn diff_mode(&self) -> bool {
        self.components
            .iter()
            .any(|c| c.change != ChangeKind::Unchanged)
            || self.edges.iter().any(|e| e.change != ChangeKind::Unchanged)
    }
}

// ---------------------------------------------------------------- detection

fn manifest_priority(file_name: &str) -> Option<usize> {
    MANIFEST_NAMES.iter().position(|m| *m == file_name)
}

/// Component name declared by a manifest, if any.
fn manifest_name(file_name: &str, contents: &str) -> Option<String> {
    match file_name {
        "package.json" => serde_json::from_str::<Value>(contents)
            .ok()?
            .get("name")?
            .as_str()
            .map(str::to_owned),
        "pyproject.toml" => toml_name(contents, &["project", "tool.poetry"]),
        "Cargo.toml" => toml_name(contents, &["package"]),
        "go.mod" => contents
            .lines()
            .find_map(|l| l.trim().strip_prefix("module "))
            .map(|m| m.trim().rsplit('/').next().unwrap_or(m).to_owned()),
        _ => None,
    }
}

/// Minimal TOML scan for `name = "..."` inside one of `sections`, so we don't
/// pull in a TOML parser for a single key.
fn toml_name(contents: &str, sections: &[&str]) -> Option<String> {
    let mut current = String::new();
    for line in contents.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            current = header.trim_end_matches(']').trim().to_owned();
        } else if sections.contains(&current.as_str()) {
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

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

fn last_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Join `spec` (with `./`/`../` segments) onto `base_dir`; `None` if it
/// escapes the repository root.
fn normalize_join(base_dir: &str, spec: &str) -> Option<String> {
    let mut segments: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for part in spec.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

struct Detector {
    /// Manifest dirs, longest first, with the component index they map to.
    manifest_dirs: Vec<(String, usize)>,
    /// Fallback components keyed by top-level directory ("" = repo root).
    fallback: HashMap<String, usize>,
}

impl Detector {
    /// Component owning a repo-relative path, if any.
    fn owner(&self, path: &str) -> Option<usize> {
        for (dir, idx) in &self.manifest_dirs {
            if path.starts_with(dir.as_str()) && path.as_bytes().get(dir.len()) == Some(&b'/') {
                return Some(*idx);
            }
        }
        let top = if path.contains('/') {
            path.split('/').next().unwrap_or("")
        } else {
            ""
        };
        self.fallback.get(top).copied()
    }
}

/// Build the component graph from source files and manifest files, both as
/// repo-relative `(path, contents)` pairs.
pub fn build(
    files: &[(String, String)],
    manifests: &[(String, String)],
) -> anyhow::Result<ComponentGraph> {
    let graph = parse::parse_files(files)?;
    Ok(build_from_graph(&graph, files, manifests))
}

fn build_from_graph(
    code: &CodeGraph,
    files: &[(String, String)],
    manifests: &[(String, String)],
) -> ComponentGraph {
    // Manifest dir -> declared name, best-priority manifest wins; the repo
    // root ("") declares the workspace, never a component.
    let mut declared: BTreeMap<String, (usize, Option<String>)> = BTreeMap::new();
    for (path, contents) in manifests {
        let file_name = last_segment(path);
        let Some(priority) = manifest_priority(file_name) else {
            continue;
        };
        let dir = dir_of(path).to_owned();
        if dir.is_empty() {
            continue;
        }
        let name = manifest_name(file_name, contents);
        match declared.get(&dir) {
            Some((existing, _)) if *existing <= priority => {}
            _ => {
                declared.insert(dir, (priority, name));
            }
        }
    }

    let mut components: Vec<Component> = Vec::new();
    let mut detector = Detector {
        manifest_dirs: Vec::new(),
        fallback: HashMap::new(),
    };
    for (dir, (_, name)) in &declared {
        let idx = components.len();
        let mut component = new_component(
            name.clone().unwrap_or_else(|| last_segment(dir).to_owned()),
            dir.clone(),
        );
        component.manifest_name = name.clone();
        components.push(component);
        detector.manifest_dirs.push((dir.clone(), idx));
    }
    detector
        .manifest_dirs
        .sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then(a.cmp(b)));

    // Assign files; create fallback components on demand.
    let mut owned: Vec<Vec<usize>> = vec![Vec::new(); components.len()]; // file indices per component
    for (i, (path, _)) in files.iter().enumerate() {
        let idx = match detector.owner(path) {
            Some(idx) => idx,
            None => {
                let top = if path.contains('/') {
                    path.split('/').next().unwrap_or("")
                } else {
                    ""
                };
                let idx = components.len();
                let name = if top.is_empty() { "(root)" } else { top };
                components.push(new_component(name.to_owned(), top.to_owned()));
                owned.push(Vec::new());
                detector.fallback.insert(top.to_owned(), idx);
                idx
            }
        };
        owned[idx].push(i);
    }

    // Drop empty components (manifest dirs with nothing parseable inside).
    let keep: Vec<usize> = (0..components.len())
        .filter(|i| !owned[*i].is_empty())
        .collect();
    let remap: HashMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect();
    components = keep.iter().map(|i| components[*i].clone()).collect();
    owned = keep
        .iter()
        .map(|i| std::mem::take(&mut owned[*i]))
        .collect();
    for (_, idx) in &mut detector.manifest_dirs {
        *idx = remap.get(idx).copied().unwrap_or(usize::MAX);
    }
    detector.manifest_dirs.retain(|(_, idx)| *idx != usize::MAX);
    for idx in detector.fallback.values_mut() {
        *idx = remap[idx]; // fallback components always own at least one file
    }

    disambiguate_names(&mut components);

    // Stats + fingerprints, the module -> component map for class counts, and
    // each component's importable Python names (first path segment below the
    // component, minus a src-layout `src/` prefix, so
    // `packages/py/agent-core/src/agent_core/x.py` yields `agent_core`).
    // An ambiguous name maps to None — a wrong edge is worse than a missing one.
    let mut module_owner: HashMap<String, usize> = HashMap::new();
    let mut py_names: HashMap<String, Option<usize>> = HashMap::new();
    for (idx, file_indices) in owned.iter().enumerate() {
        let mut langs: BTreeSet<&'static str> = BTreeSet::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for &i in file_indices {
            let (path, contents) = &files[i];
            if let Some(lang) = Language::from_path(path) {
                langs.insert(lang.name());
            }
            path.hash(&mut hasher);
            contents.hash(&mut hasher);
            module_owner.insert(parse::module_path(path), idx);
            if Language::from_path(path) == Some(Language::Python) {
                if let Some(name) = importable_py_name(path, &components[idx].path) {
                    match py_names.get(&name) {
                        Some(Some(existing)) if *existing != idx => {
                            py_names.insert(name, None);
                        }
                        Some(_) => {}
                        None => {
                            py_names.insert(name, Some(idx));
                        }
                    }
                }
            }
        }
        let component = &mut components[idx];
        component.files = file_indices.len();
        component.langs = langs.into_iter().collect();
        component.fingerprint = hasher.finish();
    }
    for class in &code.classes {
        if let Some(&idx) = module_owner.get(&class.module) {
            components[idx].classes += 1;
        }
    }

    let edges = build_edges(&code.imports, &components, &detector, &py_names);

    let mut graph = ComponentGraph { components, edges };
    graph.normalize();
    graph
}

fn new_component(name: String, path: String) -> Component {
    let group = dir_of(&path).to_owned();
    Component {
        name,
        manifest_name: None,
        path,
        group,
        langs: Vec::new(),
        files: 0,
        classes: 0,
        fingerprint: 0,
        change: ChangeKind::Unchanged,
    }
}

/// Duplicate display names get their parent directory prefixed
/// (`js/core` vs `py/core`); the full path is the last resort.
fn disambiguate_names(components: &mut [Component]) {
    for _ in 0..2 {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for c in components.iter() {
            *counts.entry(c.name.clone()).or_default() += 1;
        }
        let mut changed = false;
        for c in components.iter_mut() {
            if counts[&c.name] > 1 && !c.group.is_empty() {
                let prefixed = format!("{}/{}", last_segment(&c.group), last_segment(&c.name));
                if prefixed != c.name {
                    c.name = prefixed;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for c in components.iter() {
        *counts.entry(c.name.clone()).or_default() += 1;
    }
    for c in components.iter_mut() {
        if counts[&c.name] > 1 {
            c.name = c.path.clone();
        }
    }
}

// -------------------------------------------------------------------- edges

enum Resolved {
    Component(usize),
    External(String),
    Unknown,
}

/// Importable top-level Python name a file contributes to its component.
fn importable_py_name(path: &str, component_path: &str) -> Option<String> {
    let rel = if component_path.is_empty() {
        path
    } else {
        path.get(component_path.len() + 1..)?
    };
    let rel = rel.strip_prefix("src/").unwrap_or(rel);
    let first = rel.split('/').next()?;
    let name = first.strip_suffix(".py").unwrap_or(first);
    (!name.is_empty()).then(|| name.to_owned())
}

fn build_edges(
    imports: &[Import],
    components: &[Component],
    detector: &Detector,
    py_names: &HashMap<String, Option<usize>>,
) -> Vec<ComponentEdge> {
    // Bare TS specifiers match manifest names; Python absolute imports match
    // importable top-level names derived from each component's own files.
    let ts_names: Vec<(&str, usize)> = components
        .iter()
        .enumerate()
        .filter_map(|(idx, c)| c.manifest_name.as_deref().map(|n| (n, idx)))
        .collect();

    let mut buckets: BTreeMap<(usize, EdgeTarget), BTreeSet<&str>> = BTreeMap::new();
    for import in imports {
        let Some(from) = detector.owner(&import.file) else {
            continue;
        };
        let resolved = match import.lang {
            Language::TypeScript => resolve_ts(import, &ts_names, detector),
            Language::Python => resolve_py(import, py_names, detector),
        };
        let target = match resolved {
            Resolved::Component(to) if to == from => continue,
            Resolved::Component(to) => EdgeTarget::Component(components[to].path.clone()),
            Resolved::External(pkg) => EdgeTarget::External(pkg),
            Resolved::Unknown => continue,
        };
        buckets
            .entry((from, target))
            .or_default()
            .insert(import.file.as_str());
    }

    buckets
        .into_iter()
        .map(|((from, to), files)| ComponentEdge {
            from: components[from].path.clone(),
            to,
            weight: files.len(),
            change: ChangeKind::Unchanged,
        })
        .collect()
}

fn resolve_ts(import: &Import, ts_names: &[(&str, usize)], detector: &Detector) -> Resolved {
    let spec = import.target.as_str();
    if spec.starts_with('.') {
        let base = dir_of(&import.file);
        return match normalize_join(base, spec).and_then(|p| detector.owner(&p)) {
            Some(idx) => Resolved::Component(idx),
            None => Resolved::Unknown,
        };
    }
    if spec.starts_with('/') {
        return Resolved::Unknown;
    }
    // Longest manifest-name match wins (`@h/core` before a hypothetical `@h`).
    let best = ts_names
        .iter()
        .filter(|(name, _)| spec == *name || spec.starts_with(&format!("{name}/")))
        .max_by_key(|(name, _)| name.len());
    if let Some((_, idx)) = best {
        return Resolved::Component(*idx);
    }
    let mut segments = spec.split('/');
    let package = match segments.next() {
        Some(scope) if scope.starts_with('@') => match segments.next() {
            Some(rest) => format!("{scope}/{rest}"),
            None => scope.to_owned(),
        },
        Some(first) => first.to_owned(),
        None => return Resolved::Unknown,
    };
    Resolved::External(package)
}

fn resolve_py(
    import: &Import,
    py_names: &HashMap<String, Option<usize>>,
    detector: &Detector,
) -> Resolved {
    let spec = import.target.as_str();
    let dots = spec.chars().take_while(|c| *c == '.').count();
    if dots > 0 {
        // `from ..mod import x`: one dot = the file's own package.
        let mut dir = dir_of(&import.file).to_owned();
        for _ in 1..dots {
            if dir.is_empty() {
                return Resolved::Unknown;
            }
            dir = dir_of(&dir).to_owned();
        }
        let rest = spec[dots..].replace('.', "/");
        return match normalize_join(&dir, &rest).and_then(|p| detector.owner(&p)) {
            Some(idx) => Resolved::Component(idx),
            None => Resolved::Unknown,
        };
    }
    let first = spec.split('.').next().unwrap_or(spec);
    match py_names.get(first) {
        Some(Some(idx)) => Resolved::Component(*idx),
        Some(None) => Resolved::Unknown, // ambiguous
        None => Resolved::External(first.to_owned()),
    }
}

// --------------------------------------------------------------------- diff

/// Merge base and head component graphs into one annotated with changes.
/// Components match by path; an edge matches by `(from, to)`. A weight change
/// alone is not a diff signal — rewiring is.
pub fn diff(base: &ComponentGraph, head: &ComponentGraph) -> ComponentGraph {
    let base_by_path: HashMap<&str, &Component> = base
        .components
        .iter()
        .map(|c| (c.path.as_str(), c))
        .collect();
    let head_paths: BTreeSet<&str> = head.components.iter().map(|c| c.path.as_str()).collect();

    let mut merged = ComponentGraph::default();
    for component in &head.components {
        let mut component = component.clone();
        component.change = match base_by_path.get(component.path.as_str()) {
            None => ChangeKind::Added,
            Some(old) if old.fingerprint == component.fingerprint => ChangeKind::Unchanged,
            Some(_) => ChangeKind::Modified,
        };
        merged.components.push(component);
    }
    for component in &base.components {
        if !head_paths.contains(component.path.as_str()) {
            let mut component = component.clone();
            component.change = ChangeKind::Removed;
            merged.components.push(component);
        }
    }

    let base_edges: BTreeSet<(&str, &EdgeTarget)> = base
        .edges
        .iter()
        .map(|e| (e.from.as_str(), &e.to))
        .collect();
    let head_edges: BTreeSet<(&str, &EdgeTarget)> = head
        .edges
        .iter()
        .map(|e| (e.from.as_str(), &e.to))
        .collect();
    for edge in &head.edges {
        let mut edge = edge.clone();
        edge.change = if base_edges.contains(&(edge.from.as_str(), &edge.to)) {
            ChangeKind::Unchanged
        } else {
            ChangeKind::Added
        };
        merged.edges.push(edge);
    }
    for edge in &base.edges {
        if !head_edges.contains(&(edge.from.as_str(), &edge.to)) {
            let mut edge = edge.clone();
            edge.change = ChangeKind::Removed;
            merged.edges.push(edge);
        }
    }

    merged.normalize();
    merged
}

// ------------------------------------------------------------------- render

#[derive(Debug, Clone)]
pub struct ComponentRenderOptions {
    /// Wrap sibling components in subgraph blocks per parent directory.
    pub group: bool,
    /// Label edges with their weight (distinct importing files) when > 1.
    pub weights: bool,
    /// Render one node per external package with edges into it.
    pub include_externals: bool,
    pub direction: Option<String>,
    pub title: Option<String>,
}

impl Default for ComponentRenderOptions {
    fn default() -> Self {
        Self {
            group: true,
            weights: false,
            include_externals: false,
            direction: None,
            title: None,
        }
    }
}

fn node_id(path: &str) -> String {
    if path.is_empty() {
        "c_root".to_owned()
    } else {
        format!("c_{}", sanitize_id(path))
    }
}

fn change_glyph(change: ChangeKind) -> &'static str {
    match change {
        ChangeKind::Added => " ✚",
        ChangeKind::Removed => " ✖",
        ChangeKind::Modified => " ✱",
        ChangeKind::Unchanged => "",
    }
}

/// Render the component graph as a Mermaid `flowchart` styled to read as a
/// UML component diagram (mermaid has no native component-diagram syntax).
pub fn render_mermaid(graph: &ComponentGraph, opts: &ComponentRenderOptions) -> String {
    let mut out = String::new();
    if let Some(title) = &opts.title {
        let _ = writeln!(out, "---\ntitle: {}\n---", escape_label(title));
    }
    let direction = opts.direction.as_deref().unwrap_or("LR");
    let _ = writeln!(out, "flowchart {direction}");

    let diff_mode = graph.diff_mode();

    let mut groups: BTreeMap<&str, Vec<&Component>> = BTreeMap::new();
    for component in &graph.components {
        let key = if opts.group {
            component.group.as_str()
        } else {
            ""
        };
        groups.entry(key).or_default().push(component);
    }
    for (group, components) in &groups {
        let indent = if group.is_empty() {
            "    "
        } else {
            let _ = writeln!(
                out,
                "    subgraph sg_{}[\"{}\"]",
                sanitize_id(group),
                escape_label(group)
            );
            "        "
        };
        for component in components {
            let glyph = if diff_mode {
                change_glyph(component.change)
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "{indent}{}[\"«component»<br/><b>{}{glyph}</b>\"]",
                node_id(&component.path),
                escape_label(&component.name),
            );
        }
        if !group.is_empty() {
            out.push_str("    end\n");
        }
    }

    // Edges; changed ones are recolored via linkStyle by emission index.
    let mut externals: BTreeMap<String, String> = BTreeMap::new(); // id -> label
    let mut added_links: Vec<usize> = Vec::new();
    let mut removed_links: Vec<usize> = Vec::new();
    let mut link_index = 0usize;
    let mut edge_count = 0usize;
    for edge in &graph.edges {
        let to_id = match &edge.to {
            EdgeTarget::Component(path) => node_id(path),
            EdgeTarget::External(package) if opts.include_externals => {
                let id = format!("ext_{}", sanitize_id(package));
                externals.insert(id.clone(), package.clone());
                id
            }
            EdgeTarget::External(_) => continue,
        };
        let mut label_parts: Vec<String> = Vec::new();
        if opts.weights && edge.weight > 1 {
            label_parts.push(edge.weight.to_string());
        }
        if diff_mode && edge.change != ChangeKind::Unchanged {
            label_parts.push(change_glyph(edge.change).trim().to_owned());
        }
        let from_id = node_id(&edge.from);
        if label_parts.is_empty() {
            let _ = writeln!(out, "    {from_id} -.-> {to_id}");
        } else {
            let _ = writeln!(
                out,
                "    {from_id} -. \"{}\" .-> {to_id}",
                label_parts.join(" ")
            );
        }
        match edge.change {
            ChangeKind::Added => added_links.push(link_index),
            ChangeKind::Removed => removed_links.push(link_index),
            _ => {}
        }
        link_index += 1;
        edge_count += 1;
    }

    for (id, label) in &externals {
        let _ = writeln!(
            out,
            "    {id}([\"{}\"]):::vizzyExternal",
            escape_label(label)
        );
    }
    if !externals.is_empty() {
        out.push_str("    classDef vizzyExternal fill:#ffffff,stroke:#656d76,stroke-dasharray:4 3,color:#656d76\n");
    }

    if diff_mode {
        for (change, css) in [
            (ChangeKind::Added, "vizzyAdded"),
            (ChangeKind::Removed, "vizzyRemoved"),
            (ChangeKind::Modified, "vizzyModified"),
        ] {
            let members: Vec<String> = graph
                .components
                .iter()
                .filter(|c| c.change == change)
                .map(|c| node_id(&c.path))
                .collect();
            if !members.is_empty() {
                let _ = writeln!(out, "    class {} {css}", members.join(","));
            }
        }
        out.push_str(concat!(
            "    classDef vizzyAdded fill:#e6ffec,stroke:#1a7f37,stroke-width:2px,color:#1a7f37\n",
            "    classDef vizzyRemoved fill:#ffebe9,stroke:#cf222e,stroke-width:2px,stroke-dasharray:6 4,color:#cf222e\n",
            "    classDef vizzyModified fill:#fff8c5,stroke:#9a6700,stroke-width:2px,color:#9a6700\n",
        ));
        if !added_links.is_empty() {
            let indices: Vec<String> = added_links.iter().map(usize::to_string).collect();
            let _ = writeln!(
                out,
                "    linkStyle {} stroke:#1a7f37,stroke-width:2.5px",
                indices.join(",")
            );
        }
        if !removed_links.is_empty() {
            let indices: Vec<String> = removed_links.iter().map(usize::to_string).collect();
            let _ = writeln!(
                out,
                "    linkStyle {} stroke:#cf222e,stroke-width:2.5px",
                indices.join(",")
            );
        }
    }

    let _ = writeln!(
        out,
        "%% vizzy: {} components, {} dependencies",
        graph.components.len(),
        edge_count
    );
    out
}

// --------------------------------------------------------------------- json

fn change_str(change: ChangeKind) -> &'static str {
    match change {
        ChangeKind::Unchanged => "unchanged",
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Modified => "modified",
    }
}

/// Serialize for external renderers (the d3 HTML view). Shape:
///
/// ```json
/// {
///   "components": [{"name", "path", "group", "langs", "files", "classes", "change"}],
///   "edges": [{"from", "to", "external", "weight", "change"}],
///   "stats": {"components", "edges", "diff"}
/// }
/// ```
///
/// `from`/`to` reference components by path; external edges carry the
/// package name with `"external": true`.
pub fn to_json(graph: &ComponentGraph) -> String {
    let components: Vec<Value> = graph
        .components
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "path": c.path,
                "group": c.group,
                "langs": c.langs,
                "files": c.files,
                "classes": c.classes,
                "change": change_str(c.change),
            })
        })
        .collect();
    let edges: Vec<Value> = graph
        .edges
        .iter()
        .map(|e| {
            let (to, external) = match &e.to {
                EdgeTarget::Component(path) => (path.clone(), false),
                EdgeTarget::External(package) => (package.clone(), true),
            };
            json!({
                "from": e.from,
                "to": to,
                "external": external,
                "weight": e.weight,
                "change": change_str(e.change),
            })
        })
        .collect();
    json!({
        "components": components,
        "edges": edges,
        "stats": {
            "components": graph.components.len(),
            "edges": graph.edges.len(),
            "diff": graph.diff_mode(),
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    type Pairs = Vec<(String, String)>;

    fn pairs(items: &[(&str, &str)]) -> Pairs {
        items
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn workspace() -> (Pairs, Pairs) {
        let manifests = pairs(&[
            (
                "package.json",
                r#"{"name": "root", "workspaces": ["packages/*"]}"#,
            ),
            ("packages/core/package.json", r#"{"name": "@x/core"}"#),
            ("apps/svc/package.json", r#"{"name": "svc"}"#),
            (
                "libs/pylib/pyproject.toml",
                "[project]\nname = \"py-lib\"\n",
            ),
        ]);
        let files = pairs(&[
            ("packages/core/src/index.ts", "export class Core {}\n"),
            (
                "apps/svc/src/main.ts",
                "import { Core } from \"@x/core\";\nimport { h } from \"./helper\";\nimport fastify from \"fastify\";\nclass Svc {}\n",
            ),
            ("apps/svc/src/helper.ts", "export const h = 1;\n"),
            (
                "libs/pylib/src/py_lib/core.py",
                "class PyCore:\n    pass\n",
            ),
            (
                "scripts/run.py",
                "import py_lib\nfrom py_lib.core import PyCore\nimport os\n",
            ),
        ]);
        (files, manifests)
    }

    #[test]
    fn detects_components_and_edges() {
        let (files, manifests) = workspace();
        let graph = build(&files, &manifests).unwrap();

        let names: Vec<&str> = graph.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["svc", "py-lib", "@x/core", "scripts"]); // sorted by path
        let by_name = |n: &str| graph.components.iter().find(|c| c.name == n).unwrap();
        assert_eq!(by_name("svc").group, "apps");
        assert_eq!(by_name("svc").files, 2);
        assert_eq!(by_name("svc").classes, 1);
        assert_eq!(by_name("@x/core").langs, ["typescript"]);
        assert_eq!(by_name("scripts").group, "");

        // svc -> @x/core (bare specifier); scripts -> py-lib (absolute import,
        // counted once despite two import statements). The relative and
        // stdlib/npm imports produce no internal edges.
        let internal: Vec<(&str, &str)> = graph
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTarget::Component(p) => Some((e.from.as_str(), p.as_str())),
                EdgeTarget::External(_) => None,
            })
            .collect();
        assert_eq!(
            internal,
            [("apps/svc", "packages/core"), ("scripts", "libs/pylib")]
        );
        assert!(graph.edges.iter().all(|e| e.weight == 1));

        let externals: Vec<&str> = graph
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTarget::External(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(externals, ["fastify", "os"]);
    }

    #[test]
    fn relative_ts_import_crossing_a_boundary_resolves() {
        let manifests = pairs(&[
            ("a/package.json", r#"{"name": "a"}"#),
            ("b/package.json", r#"{"name": "b"}"#),
        ]);
        let files = pairs(&[
            ("a/src/x.ts", "import { y } from \"../../b/src/y\";\n"),
            ("b/src/y.ts", "export const y = 1;\n"),
        ]);
        let graph = build(&files, &manifests).unwrap();
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "a");
        assert_eq!(graph.edges[0].to, EdgeTarget::Component("b".into()));
    }

    #[test]
    fn diff_marks_rewiring_and_component_churn() {
        let (base_files, manifests) = workspace();
        let mut head_files = base_files.clone();
        // Rewire scripts -> @x/core via a new file, and touch a file in svc.
        head_files.push((
            "scripts/tool.ts".into(),
            "import { Core } from \"@x/core\";\n".into(),
        ));
        for (path, contents) in &mut head_files {
            if path == "apps/svc/src/helper.ts" {
                *contents = "export const h = 2;\n".into();
            }
        }
        let base = build(&base_files, &manifests).unwrap();
        let head = build(&head_files, &manifests).unwrap();
        let merged = diff(&base, &head);

        let change = |n: &str| {
            merged
                .components
                .iter()
                .find(|c| c.name == n)
                .unwrap()
                .change
        };
        assert_eq!(change("svc"), ChangeKind::Modified);
        assert_eq!(change("scripts"), ChangeKind::Modified);
        assert_eq!(change("@x/core"), ChangeKind::Unchanged);

        let added: Vec<(&str, &EdgeTarget)> = merged
            .edges
            .iter()
            .filter(|e| e.change == ChangeKind::Added)
            .map(|e| (e.from.as_str(), &e.to))
            .collect();
        assert_eq!(
            added,
            [("scripts", &EdgeTarget::Component("packages/core".into()))]
        );
        assert!(merged
            .edges
            .iter()
            .filter(|e| e.from == "apps/svc")
            .all(|e| e.change == ChangeKind::Unchanged));
    }

    #[test]
    fn renders_mermaid_flowchart() {
        let (files, manifests) = workspace();
        let graph = build(&files, &manifests).unwrap();
        let out = render_mermaid(&graph, &ComponentRenderOptions::default());
        assert!(out.starts_with("flowchart LR"));
        assert!(out.contains("subgraph sg_apps[\"apps\"]"));
        assert!(out.contains("c_apps_svc[\"«component»<br/><b>svc</b>\"]"));
        assert!(out.contains("c_apps_svc -.-> c_packages_core"));
        assert!(!out.contains("fastify")); // externals off by default
        assert!(out.contains("%% vizzy: 4 components, 2 dependencies"));
    }

    #[test]
    fn manifest_names_parse() {
        assert_eq!(
            manifest_name(
                "pyproject.toml",
                "[build-system]\nrequires=[]\n[project]\nname = \"x\"\n"
            ),
            Some("x".into())
        );
        assert_eq!(
            manifest_name("Cargo.toml", "[package]\nname = \"y\"\nversion = \"0\"\n"),
            Some("y".into())
        );
        assert_eq!(
            manifest_name("Cargo.toml", "[workspace]\nmembers = []\n"),
            None
        );
        assert_eq!(
            manifest_name("go.mod", "module github.com/acme/thing\n\ngo 1.22\n"),
            Some("thing".into())
        );
    }
}
