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

use crate::export::change_str;
use crate::mermaid::{escape_label, sanitize_id};
use crate::model::{ChangeCounts, ChangeKind, Class, CodeGraph, Import, Language};
use crate::palette;
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
    /// Per-file content hashes, keyed by path relative to the component, so a
    /// diff can say *which* files changed and not only that some did.
    pub file_hashes: BTreeMap<String, u64>,
    /// Files added, removed or changed relative to the base revision, relative
    /// to the component; empty for an unchanged component or outside a diff.
    /// This is how a component that changed without any class changing can
    /// still explain itself (§5.3).
    pub changed_files: Vec<String>,
    pub change: ChangeKind,
    /// True if this component is outside the scope but shares an edge with an in-scope component.
    pub is_boundary: bool,
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

/// A class together with the component that owns it — the drill-down detail
/// behind a component box.
#[derive(Debug, Clone)]
pub struct PlacedClass {
    /// Path of the owning component.
    pub component: String,
    pub class: Class,
}

#[derive(Debug, Clone, Default)]
pub struct ComponentGraph {
    pub components: Vec<Component>,
    pub edges: Vec<ComponentEdge>,
    /// Classes inside each component. Carried so a reader can open a component
    /// and see what it is made of without regenerating a separate diagram.
    pub classes: Vec<PlacedClass>,
    /// The file selection this graph was built under (`include …`,
    /// `exclude …`, `lang …`), empty when nothing was filtered. Carried on the
    /// graph so every renderer can say what the diagram is NOT claiming to
    /// show: a component filtered out here is absent, not unchanged.
    pub selection: Vec<String>,
    /// Unchanged components a `focus` pass left out (§6.3). Zero when the
    /// graph is complete. Carried so the trailer and the JSON can say how much
    /// of the picture the reader is not seeing.
    pub omitted: usize,
}

impl ComponentGraph {
    fn normalize(&mut self) {
        self.components.sort_by(|a, b| a.path.cmp(&b.path));
        self.edges
            .sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
        self.classes.sort_by(|a, b| {
            (&a.component, &a.class.qualified).cmp(&(&b.component, &b.class.qualified))
        });
    }

    /// Change counts over components and edges: both are diff signals (§6),
    /// and a rewiring with no component churn must still read as a change.
    pub fn change_counts(&self) -> ChangeCounts {
        ChangeCounts::tally(
            self.components
                .iter()
                .map(|c| c.change)
                .chain(self.edges.iter().map(|e| e.change)),
        )
    }

    pub fn diff_mode(&self) -> bool {
        self.change_counts().changed()
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

/// `path` relative to `dir` when `dir` is a proper ancestor (`""` is every
/// path's ancestor); `None` otherwise.
fn under<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    if dir.is_empty() {
        return Some(path);
    }
    let rest = path.strip_prefix(dir)?;
    rest.strip_prefix('/')
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
    /// Split directories (§3.4), longest first, each with the import root its
    /// children resolve Python imports against.
    splits: Vec<(String, String)>,
    /// Components a split created, keyed by component path.
    split_components: HashMap<String, usize>,
}

impl Detector {
    /// For a file under a split directory, the path of the component the
    /// split assigns it to: `split/<child>` for a file in a child directory,
    /// the split directory itself for a file sitting directly in it.
    fn split_component_path(&self, path: &str) -> Option<String> {
        for (split, _) in &self.splits {
            if let Some(rel) = under(path, split) {
                return Some(match rel.split_once('/') {
                    Some((child, _)) => format!("{split}/{child}"),
                    None => split.clone(),
                });
            }
        }
        None
    }

    /// The directory a file's Python module path is spelled from: the split's
    /// import root for a split child, else the owning component's directory,
    /// stepping into `src/` for a src-layout package.
    fn import_root(&self, path: &str, component_path: &str) -> String {
        for (split, root) in &self.splits {
            if under(path, split).is_some() {
                return root.clone();
            }
        }
        let src = if component_path.is_empty() {
            "src".to_owned()
        } else {
            format!("{component_path}/src")
        };
        if under(path, &src).is_some() {
            src
        } else {
            component_path.to_owned()
        }
    }

    /// Component owning a repo-relative path, if any.
    fn owner(&self, path: &str) -> Option<usize> {
        if let Some(component_path) = self.split_component_path(path) {
            return self.split_components.get(&component_path).copied();
        }
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
/// repo-relative `(path, contents)` pairs. `splits` are directories whose
/// direct children become components of their own (§3.4).
pub fn build(
    files: &[(String, String)],
    manifests: &[(String, String)],
    splits: &[String],
) -> anyhow::Result<ComponentGraph> {
    for split in splits {
        if split.trim_matches('/').is_empty() || split == "." {
            anyhow::bail!(
                "split needs a directory below the root; the root's own top-level \
                 directories are already components (component.md §3.1 rule 3)"
            );
        }
    }
    let graph = parse::parse_files(files)?;
    Ok(build_from_graph(&graph, files, manifests, splits))
}

fn build_from_graph(
    code: &CodeGraph,
    files: &[(String, String)],
    manifests: &[(String, String)],
    splits: &[String],
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
        splits: Vec::new(),
        split_components: HashMap::new(),
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

    // A split refines whatever would have owned it, so its children spell
    // their Python imports from that owner's import root: the enclosing
    // manifest's directory (its `src/` for a src layout), or the top-level
    // directory for a fallback component.
    for split in splits {
        let split = split.trim_matches('/').to_owned();
        let enclosing = detector
            .manifest_dirs
            .iter()
            .find(|(dir, _)| split == *dir || under(&split, dir).is_some())
            .map(|(dir, _)| dir.clone());
        let root = match enclosing {
            Some(dir) => {
                let src = format!("{dir}/src");
                if split == src || under(&split, &src).is_some() {
                    src
                } else {
                    dir
                }
            }
            None => split.split('/').next().unwrap_or("").to_owned(),
        };
        detector.splits.push((split, root));
    }
    detector
        .splits
        .sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then(a.cmp(b)));

    // Assign files; create split and fallback components on demand.
    let mut owned: Vec<Vec<usize>> = vec![Vec::new(); components.len()]; // file indices per component
    for (i, (path, _)) in files.iter().enumerate() {
        if let Some(component_path) = detector.split_component_path(path) {
            let idx = match detector.split_components.get(&component_path) {
                Some(idx) => *idx,
                None => {
                    let idx = components.len();
                    components.push(new_component(
                        last_segment(&component_path).to_owned(),
                        component_path.clone(),
                    ));
                    owned.push(Vec::new());
                    detector.split_components.insert(component_path, idx);
                    idx
                }
            };
            owned[idx].push(i);
            continue;
        }
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
    for idx in detector.split_components.values_mut() {
        *idx = remap[idx]; // split components are created only for a file
    }

    disambiguate_names(&mut components);

    // Stats + fingerprints, the module -> component map for class counts, and
    // the importable Python names each component answers to: every dotted
    // prefix of each file's module path, spelled from its import root, so
    // `packages/py/agent-core/src/agent_core/x.py` registers `agent_core` and
    // `agent_core.x`. A prefix two components share maps to None — a wrong
    // edge is worse than a missing one — which is what lets a split package
    // resolve `pkg.sub.mod` to `sub` while `pkg` alone resolves to nothing.
    let mut module_owner: HashMap<String, usize> = HashMap::new();
    let mut py_names: HashMap<String, Option<usize>> = HashMap::new();
    for (idx, file_indices) in owned.iter().enumerate() {
        let mut langs: BTreeSet<&'static str> = BTreeSet::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut file_hashes: BTreeMap<String, u64> = BTreeMap::new();
        for &i in file_indices {
            let (path, contents) = &files[i];
            if let Some(lang) = Language::from_path(path) {
                langs.insert(lang.name());
            }
            path.hash(&mut hasher);
            contents.hash(&mut hasher);
            let mut file_hasher = std::collections::hash_map::DefaultHasher::new();
            contents.hash(&mut file_hasher);
            let rel = under(path, &components[idx].path)
                .unwrap_or(path)
                .to_owned();
            file_hashes.insert(rel, file_hasher.finish());
            module_owner.insert(parse::module_path(path), idx);
            if Language::from_path(path) == Some(Language::Python) {
                let root = detector.import_root(path, &components[idx].path);
                for name in importable_py_prefixes(path, &root) {
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
        component.file_hashes = file_hashes;
    }
    let mut placed: Vec<PlacedClass> = Vec::new();
    for class in &code.classes {
        if let Some(&idx) = module_owner.get(&class.module) {
            components[idx].classes += 1;
            placed.push(PlacedClass {
                component: components[idx].path.clone(),
                class: class.clone(),
            });
        }
    }

    let edges = build_edges(&code.imports, &components, &detector, &py_names);

    let mut graph = ComponentGraph {
        components,
        edges,
        classes: placed,
        selection: Vec::new(),
        omitted: 0,
    };
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
        file_hashes: BTreeMap::new(),
        changed_files: Vec::new(),
        change: ChangeKind::Unchanged,
        is_boundary: false,
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

/// Every dotted prefix of a Python file's module path, spelled from
/// `import_root`: `pkg/sub/mod.py` yields `pkg`, `pkg.sub`, `pkg.sub.mod`.
/// Empty when the file is not under the root.
fn importable_py_prefixes(path: &str, import_root: &str) -> Vec<String> {
    let Some(rel) = under(path, import_root) else {
        return Vec::new();
    };
    let dotted = parse::module_path(rel);
    let mut prefixes = Vec::new();
    let mut prefix = String::new();
    for segment in dotted.split('.').filter(|s| !s.is_empty()) {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(segment);
        prefixes.push(prefix.clone());
    }
    prefixes
}

fn build_edges(
    imports: &[Import],
    components: &[Component],
    detector: &Detector,
    py_names: &HashMap<String, Option<usize>>,
) -> Vec<ComponentEdge> {
    // Bare TS specifiers match manifest names; Python absolute imports match
    // the longest registered dotted prefix of the specifier.
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
    // Longest known prefix wins, as for TS bare specifiers. Stopping at an
    // ambiguous prefix rather than trying a shorter one is deliberate: the
    // shorter prefix is shared by even more components.
    let segments: Vec<&str> = spec.split('.').collect();
    for n in (1..=segments.len()).rev() {
        match py_names.get(&segments[..n].join(".")) {
            Some(Some(idx)) => return Resolved::Component(*idx),
            Some(None) => return Resolved::Unknown,
            None => continue,
        }
    }
    Resolved::External(segments[0].to_owned())
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
        component.changed_files = match base_by_path.get(component.path.as_str()) {
            Some(old) => changed_files(&old.file_hashes, &component.file_hashes),
            None => component.file_hashes.keys().cloned().collect(),
        };
        merged.components.push(component);
    }
    for component in &base.components {
        if !head_paths.contains(component.path.as_str()) {
            let mut component = component.clone();
            component.change = ChangeKind::Removed;
            component.changed_files = component.file_hashes.keys().cloned().collect();
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

    merged.classes = diff_classes(&base.classes, &head.classes);
    merged.selection = head.selection.clone();
    merged.normalize();
    merged
}

/// Files present on one side only, or present on both with different
/// contents. Sorted, so the list is stable.
fn changed_files(base: &BTreeMap<String, u64>, head: &BTreeMap<String, u64>) -> Vec<String> {
    let mut out: BTreeSet<&str> = BTreeSet::new();
    for (path, hash) in head {
        if base.get(path) != Some(hash) {
            out.insert(path);
        }
    }
    for path in base.keys() {
        if !head.contains_key(path) {
            out.insert(path);
        }
    }
    out.into_iter().map(str::to_owned).collect()
}

/// Annotate the drill-down classes with their own change status, reusing the
/// class diagram's diff engine so both views agree on what changed.
fn diff_classes(base: &[PlacedClass], head: &[PlacedClass]) -> Vec<PlacedClass> {
    let as_graph = |placed: &[PlacedClass]| CodeGraph {
        classes: placed.iter().map(|p| p.class.clone()).collect(),
        imports: Vec::new(),
    };
    // Base first so a surviving class is attributed to its head component
    // (a moved class follows the move); removed classes keep their base home.
    let mut owner: HashMap<&str, &str> = HashMap::new();
    for placed in base.iter().chain(head) {
        owner.insert(placed.class.qualified.as_str(), placed.component.as_str());
    }

    let merged = crate::diff::diff_graphs(&as_graph(base), &as_graph(head));
    merged
        .classes
        .into_iter()
        .filter_map(|class| {
            owner
                .get(class.qualified.as_str())
                .map(|component| PlacedClass {
                    component: (*component).to_owned(),
                    class,
                })
        })
        .collect()
}

/// Filter graph to components under scope_path and out-of-scope neighbours that share edges.
/// Empty string or "." returns the graph unchanged. Boundary neighbours are marked with is_boundary=true.
pub fn scope(graph: &ComponentGraph, scope_path: &str) -> ComponentGraph {
    if scope_path.is_empty() || scope_path == "." {
        return graph.clone();
    }

    let is_in_scope = |path: &str| -> bool {
        path == scope_path || path.starts_with(&format!("{}/", scope_path))
    };

    let in_scope: BTreeSet<&str> = graph
        .components
        .iter()
        .filter(|c| is_in_scope(&c.path))
        .map(|c| c.path.as_str())
        .collect();

    if in_scope.is_empty() {
        return ComponentGraph {
            selection: graph.selection.clone(),
            ..ComponentGraph::default()
        };
    }

    let mut boundary: BTreeSet<String> = BTreeSet::new();
    for edge in &graph.edges {
        let from_in = in_scope.contains(edge.from.as_str());
        let to_in = match &edge.to {
            EdgeTarget::Component(path) => in_scope.contains(path.as_str()),
            EdgeTarget::External(_) => false,
        };

        if from_in && !to_in {
            if let EdgeTarget::Component(path) = &edge.to {
                boundary.insert(path.clone());
            }
        }
        if !from_in && to_in {
            boundary.insert(edge.from.clone());
        }
    }

    let kept_paths: BTreeSet<&str> = in_scope
        .iter()
        .copied()
        .chain(boundary.iter().map(|s| s.as_str()))
        .collect();

    let mut result = ComponentGraph::default();
    for component in &graph.components {
        if kept_paths.contains(component.path.as_str()) {
            let mut c = component.clone();
            c.is_boundary = !in_scope.contains(component.path.as_str());
            result.components.push(c);
        }
    }

    for edge in &graph.edges {
        let from_kept = kept_paths.contains(edge.from.as_str());
        let to_kept = match &edge.to {
            EdgeTarget::Component(path) => kept_paths.contains(path.as_str()),
            EdgeTarget::External(_) => true,
        };

        if from_kept && to_kept {
            let from_in_scope = in_scope.contains(edge.from.as_str());
            let to_in_scope = match &edge.to {
                EdgeTarget::Component(path) => in_scope.contains(path.as_str()),
                EdgeTarget::External(_) => false,
            };

            if from_in_scope || to_in_scope {
                result.edges.push(edge.clone());
            }
        }
    }

    result.classes = graph
        .classes
        .iter()
        .filter(|pc| kept_paths.contains(pc.component.as_str()))
        .cloned()
        .collect();
    result.selection = graph.selection.clone();

    result.normalize();
    result
}

/// Keep only what a reader of a change needs (§6.3): every changed
/// component, every added or removed edge with both its endpoints, and the
/// unchanged neighbours an existing edge ties to a changed component. Every
/// other component is left out and counted in `omitted`, so the trailer and
/// the JSON can say how much of the picture is not shown. Boundary nodes
/// carry no change of their own and survive only as neighbours.
pub fn focus(graph: &ComponentGraph) -> ComponentGraph {
    let changed: BTreeSet<&str> = graph
        .components
        .iter()
        .filter(|c| c.change != ChangeKind::Unchanged && !c.is_boundary)
        .map(|c| c.path.as_str())
        .collect();

    let mut keep: BTreeSet<&str> = changed.clone();
    let mut edges: Vec<ComponentEdge> = Vec::new();
    for edge in &graph.edges {
        let to = match &edge.to {
            EdgeTarget::Component(path) => Some(path.as_str()),
            EdgeTarget::External(_) => None,
        };
        let touches_changed =
            changed.contains(edge.from.as_str()) || to.is_some_and(|t| changed.contains(t));
        if edge.change != ChangeKind::Unchanged || touches_changed {
            keep.insert(edge.from.as_str());
            if let Some(t) = to {
                keep.insert(t);
            }
            edges.push(edge.clone());
        }
    }

    let components: Vec<Component> = graph
        .components
        .iter()
        .filter(|c| keep.contains(c.path.as_str()))
        .cloned()
        .collect();
    let mut result = ComponentGraph {
        omitted: graph.omitted + (graph.components.len() - components.len()),
        components,
        edges,
        classes: graph
            .classes
            .iter()
            .filter(|pc| keep.contains(pc.component.as_str()))
            .cloned()
            .collect(),
        selection: graph.selection.clone(),
    };
    result.normalize();
    result
}

/// The class-level view inside a diff's changed components (§6.4): every
/// changed class that lives in a changed, non-boundary component, together
/// with the map from class to component name that lets the class renderer
/// group them into one namespace per component. Unchanged classes are left
/// out; the renderer's `changed_members_only` does the same for members.
pub fn zoom(graph: &ComponentGraph) -> (CodeGraph, HashMap<String, String>) {
    let changed_components: HashMap<&str, &str> = graph
        .components
        .iter()
        .filter(|c| c.change != ChangeKind::Unchanged && !c.is_boundary)
        .map(|c| (c.path.as_str(), c.name.as_str()))
        .collect();
    let mut classes = Vec::new();
    let mut component_of = HashMap::new();
    for placed in &graph.classes {
        if placed.class.change == ChangeKind::Unchanged {
            continue;
        }
        if let Some(name) = changed_components.get(placed.component.as_str()) {
            component_of.insert(placed.class.qualified.clone(), (*name).to_owned());
            classes.push(placed.class.clone());
        }
    }
    (
        CodeGraph {
            classes,
            imports: Vec::new(),
        },
        component_of,
    )
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
            let glyph = if diff_mode && !component.is_boundary {
                component.change.glyph()
            } else {
                ""
            };
            let stereotype = if component.is_boundary {
                "«boundary»"
            } else {
                "«component»"
            };
            let _ = writeln!(
                out,
                "{indent}{}[\"{}<br/><b>{}{glyph}</b>\"]",
                node_id(&component.path),
                stereotype,
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
            label_parts.push(edge.change.glyph().trim().to_owned());
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
            "    {id}([\"{}\"]):::vizzleExternal",
            escape_label(label)
        );
    }
    if !externals.is_empty() {
        out.push_str("    classDef vizzleExternal fill:#ffffff,stroke:#656d76,stroke-dasharray:4 3,color:#656d76\n");
    }

    let has_boundary = graph.components.iter().any(|c| c.is_boundary);
    if has_boundary {
        out.push_str(&palette::mermaid_boundary_classdef());
        let boundary_nodes: Vec<String> = graph
            .components
            .iter()
            .filter(|c| c.is_boundary)
            .map(|c| node_id(&c.path))
            .collect();
        if !boundary_nodes.is_empty() {
            let _ = writeln!(out, "    class {} diffBoundary", boundary_nodes.join(","));
        }
    }

    if diff_mode {
        for change in [ChangeKind::Added, ChangeKind::Removed, ChangeKind::Modified] {
            let members: Vec<String> = graph
                .components
                .iter()
                .filter(|c| c.change == change && !c.is_boundary)
                .map(|c| node_id(&c.path))
                .collect();
            if let (false, Some(css)) = (members.is_empty(), palette::mermaid_class(change)) {
                let _ = writeln!(out, "    class {} {css}", members.join(","));
            }
        }
        out.push_str(&palette::mermaid_classdefs());
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
        "%% vizzle: {} components, {} dependencies",
        graph.components.len(),
        edge_count
    );
    // Mermaid has no legend, so the selection rides the trailer: a reader of
    // the source sees why a component they expected is not drawn.
    if !graph.selection.is_empty() {
        let _ = writeln!(out, "%% vizzle: selection: {}", graph.selection.join("; "));
    }
    if graph.omitted > 0 {
        let _ = writeln!(
            out,
            "%% vizzle: focus: {} unchanged component(s) not drawn",
            graph.omitted
        );
    }
    out
}

// --------------------------------------------------------------------- json

/// Serialize for external renderers (the d3 HTML view). Shape:
///
/// ```json
/// {
///   "components": [{"name", "path", "group", "langs", "files", "classes", "change", "changedFiles"}],
///   "edges": [{"from", "to", "external", "weight", "change"}],
///   "classes": [{"component", ...class fields}],
///   "stats": {"components", "edges", "diff", "changes": {"added", "removed", "modified"}, "omitted"}
/// }
/// ```
///
/// `from`/`to` reference components by path; external edges carry the
/// package name with `"external": true`. `classes` is the drill-down detail
/// (same shape as the class diagram's export); `include_classes = false`
/// omits it for a leaner page.
pub fn to_json(graph: &ComponentGraph, include_classes: bool) -> String {
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
                "changedFiles": c.changed_files,
                "boundary": c.is_boundary,
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
    let classes: Vec<Value> = if include_classes {
        graph
            .classes
            .iter()
            .map(|placed| {
                let mut value = crate::export::class_json(&placed.class);
                value["component"] = json!(placed.component);
                value
            })
            .collect()
    } else {
        Vec::new()
    };

    // Relations between those classes, so an exploded component can render a
    // real class diagram rather than a list of names. Derived from the same
    // resolver the class diagram uses.
    let class_relations: Vec<Value> = if include_classes {
        let code = CodeGraph {
            classes: graph.classes.iter().map(|p| p.class.clone()).collect(),
            imports: Vec::new(),
        };
        crate::resolve::resolve_all_relations(&code)
            .iter()
            .filter(|r| matches!(r.to, crate::resolve::Target::Internal(_)))
            .map(crate::export::relation_json)
            .collect()
    } else {
        Vec::new()
    };

    json!({
        "components": components,
        "edges": edges,
        "classes": classes,
        "classRelations": class_relations,
        "stats": {
            "components": graph.components.len(),
            "edges": graph.edges.len(),
            "classes": classes.len(),
            "classRelations": class_relations.len(),
            "diff": graph.diff_mode(),
            "changes": crate::export::change_counts_json(&graph.change_counts()),
            "omitted": graph.omitted,
            "selection": graph.selection,
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
        let graph = build(&files, &manifests, &[]).unwrap();

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
        let graph = build(&files, &manifests, &[]).unwrap();
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
        let base = build(&base_files, &manifests, &[]).unwrap();
        let head = build(&head_files, &manifests, &[]).unwrap();
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
    fn attaches_classes_to_their_component() {
        let (files, manifests) = workspace();
        let graph = build(&files, &manifests, &[]).unwrap();

        let owner = |name: &str| {
            graph
                .classes
                .iter()
                .find(|p| p.class.name == name)
                .map(|p| p.component.as_str())
        };
        assert_eq!(owner("Core"), Some("packages/core"));
        assert_eq!(owner("Svc"), Some("apps/svc"));
        assert_eq!(owner("PyCore"), Some("libs/pylib"));

        let value: serde_json::Value = serde_json::from_str(&to_json(&graph, true)).unwrap();
        assert_eq!(value["stats"]["classes"], 3);
        let core = value["classes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "Core")
            .unwrap();
        assert_eq!(core["component"], "packages/core");
        assert!(core["members"].is_array()); // full class detail for drill-down

        // The lean page omits the detail but keeps components and edges.
        let lean: serde_json::Value = serde_json::from_str(&to_json(&graph, false)).unwrap();
        assert_eq!(lean["classes"].as_array().unwrap().len(), 0);
        assert_eq!(lean["components"], value["components"]);
    }

    #[test]
    fn diff_annotates_classes_inside_components() {
        let (base_files, manifests) = workspace();
        let mut head_files = base_files.clone();
        for (path, contents) in &mut head_files {
            if path == "packages/core/src/index.ts" {
                *contents = "export class Core {}\nexport class Extra {}\n".into();
            }
        }
        let merged = diff(
            &build(&base_files, &manifests, &[]).unwrap(),
            &build(&head_files, &manifests, &[]).unwrap(),
        );

        let class = |name: &str| {
            merged
                .classes
                .iter()
                .find(|p| p.class.name == name)
                .unwrap()
        };
        assert_eq!(class("Extra").class.change, ChangeKind::Added);
        assert_eq!(class("Extra").component, "packages/core");
        assert_eq!(class("Core").class.change, ChangeKind::Unchanged);
        assert_eq!(class("Svc").class.change, ChangeKind::Unchanged);
    }

    #[test]
    fn renders_mermaid_flowchart() {
        let (files, manifests) = workspace();
        let graph = build(&files, &manifests, &[]).unwrap();
        let out = render_mermaid(&graph, &ComponentRenderOptions::default());
        assert!(out.starts_with("flowchart LR"));
        assert!(out.contains("subgraph sg_apps[\"apps\"]"));
        assert!(out.contains("c_apps_svc[\"«component»<br/><b>svc</b>\"]"));
        assert!(out.contains("c_apps_svc -.-> c_packages_core"));
        assert!(!out.contains("fastify")); // externals off by default
        assert!(out.contains("%% vizzle: 4 components, 2 dependencies"));
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

    #[test]
    fn scope_empty_path_is_noop() {
        let (files, manifests) = workspace();
        let graph = build(&files, &manifests, &[]).unwrap();
        let scoped = scope(&graph, "");
        assert_eq!(scoped.components.len(), graph.components.len());
        assert_eq!(scoped.edges.len(), graph.edges.len());
        assert!(!scoped.components.iter().any(|c| c.is_boundary));
    }

    #[test]
    fn scope_root_path_is_noop() {
        let (files, manifests) = workspace();
        let graph = build(&files, &manifests, &[]).unwrap();
        let scoped = scope(&graph, ".");
        assert_eq!(scoped.components.len(), graph.components.len());
        assert_eq!(scoped.edges.len(), graph.edges.len());
        assert!(!scoped.components.iter().any(|c| c.is_boundary));
    }

    // Workspace with boundary-to-boundary edges, used by scope tests.
    //
    // - `pkgs/core`: package, imported by svc and bridge
    // - `apps/svc`: imports core (boundary when scoping to core)
    // - `apps/bridge`: imports core (boundary when scoping to core) AND imports svc
    //   → the bridge→svc edge is boundary-to-boundary and must be dropped.
    fn workspace_with_cross_boundary_edge() -> (Pairs, Pairs) {
        let manifests = pairs(&[
            ("pkgs/core/package.json", r#"{"name": "@x/core"}"#),
            ("apps/svc/package.json", r#"{"name": "svc"}"#),
            ("apps/bridge/package.json", r#"{"name": "bridge"}"#),
        ]);
        let files = pairs(&[
            ("pkgs/core/src/index.ts", "export class Core {}\n"),
            (
                "apps/svc/src/main.ts",
                "import { Core } from \"@x/core\";\nclass Svc {}\n",
            ),
            (
                "apps/bridge/src/main.ts",
                "import { Core } from \"@x/core\";\nimport { Svc } from \"../svc/src/main\";\n",
            ),
        ]);
        (files, manifests)
    }

    #[test]
    fn scope_filters_to_path_and_boundary_neighbours() {
        let (files, manifests) = workspace();
        let graph = build(&files, &manifests, &[]).unwrap();
        // Workspace: apps/svc → packages/core (internal), scripts → libs/pylib (internal).
        // Scope to "apps/svc": keeps svc (in-scope) + packages/core (boundary, svc imports it).
        // scripts and libs/pylib have no edge into svc's scope → excluded.
        let scoped = scope(&graph, "apps/svc");

        let paths: BTreeSet<&str> = scoped.components.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(
            paths,
            BTreeSet::from(["apps/svc", "packages/core"]),
            "exact component set for apps/svc scope"
        );

        let svc = scoped
            .components
            .iter()
            .find(|c| c.path == "apps/svc")
            .unwrap();
        assert!(!svc.is_boundary, "apps/svc is in scope, not a boundary");

        let core = scoped
            .components
            .iter()
            .find(|c| c.path == "packages/core")
            .unwrap();
        assert!(
            core.is_boundary,
            "packages/core is boundary (svc imports it)"
        );

        // Exact edge set: svc→core (internal, kept because svc is in-scope)
        // and svc→fastify (external, kept because svc is in-scope).
        // scripts→libs/pylib is dropped (neither endpoint in scope).
        let internal_edges: Vec<(&str, &str)> = scoped
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTarget::Component(to) => Some((e.from.as_str(), to.as_str())),
                EdgeTarget::External(_) => None,
            })
            .collect();
        assert_eq!(
            internal_edges,
            [("apps/svc", "packages/core")],
            "only the apps/svc→packages/core edge survives"
        );

        let external_edges: Vec<(&str, &str)> = scoped
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTarget::External(pkg) => Some((e.from.as_str(), pkg.as_str())),
                EdgeTarget::Component(_) => None,
            })
            .collect();
        assert_eq!(
            external_edges,
            [("apps/svc", "fastify")],
            "external edge from in-scope component is kept; scripts→os is dropped"
        );
    }

    #[test]
    fn scope_keeps_only_edges_where_at_least_one_endpoint_is_in_scope() {
        let (files, manifests) = workspace();
        let graph = build(&files, &manifests, &[]).unwrap();
        // Scope to packages/core: svc imports core → svc is boundary.
        // scripts imports libs/pylib — neither is in scope → both excluded.
        let scoped = scope(&graph, "packages/core");

        let paths: BTreeSet<&str> = scoped.components.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(
            paths,
            BTreeSet::from(["packages/core", "apps/svc"]),
            "exact component set: core (in-scope) + svc (boundary)"
        );

        assert!(
            !scoped
                .components
                .iter()
                .find(|c| c.path == "packages/core")
                .unwrap()
                .is_boundary,
            "core is in-scope"
        );
        assert!(
            scoped
                .components
                .iter()
                .find(|c| c.path == "apps/svc")
                .unwrap()
                .is_boundary,
            "svc is boundary"
        );

        // Exact edge set: only apps/svc→packages/core (to-endpoint is in-scope).
        // apps/svc→fastify is dropped (external, but from is boundary, to is external → neither in scope).
        let internal_edges: Vec<(&str, &str)> = scoped
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTarget::Component(to) => Some((e.from.as_str(), to.as_str())),
                EdgeTarget::External(_) => None,
            })
            .collect();
        assert_eq!(
            internal_edges,
            [("apps/svc", "packages/core")],
            "only the svc→core edge survives"
        );

        let external_edges: Vec<(&str, &str)> = scoped
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTarget::External(pkg) => Some((e.from.as_str(), pkg.as_str())),
                EdgeTarget::Component(_) => None,
            })
            .collect();
        assert!(
            external_edges.is_empty(),
            "no external edges survive: svc→fastify dropped (svc is boundary, fastify external, neither in scope)"
        );
    }

    /// One manifest, src layout, a package with two subpackages that import
    /// each other absolutely, plus a module directly in the package.
    fn monolith() -> (Pairs, Pairs) {
        let files = pairs(&[
            (
                "svc/src/svc/main.py",
                "from svc.api.routes import router\nclass App: ...\n",
            ),
            (
                "svc/src/svc/api/routes.py",
                "from svc.store.db import Db\nfrom typing import Any\nclass Router: ...\n",
            ),
            ("svc/src/svc/store/db.py", "class Db: ...\n"),
            ("svc/src/svc/store/__init__.py", ""),
        ]);
        let manifests = pairs(&[("svc/pyproject.toml", "[project]\nname = \"svc\"\n")]);
        (files, manifests)
    }

    #[test]
    fn without_a_split_a_manifest_is_one_component_with_no_internal_edges() {
        let (files, manifests) = monolith();
        let graph = build(&files, &manifests, &[]).unwrap();
        assert_eq!(graph.components.len(), 1);
        assert!(
            !graph
                .edges
                .iter()
                .any(|e| matches!(e.to, EdgeTarget::Component(_))),
            "self-imports are not edges"
        );
        // The package's own name is not an external package either: every
        // prefix of `svc.*` is known, so it resolves to the one component.
        assert!(
            !graph
                .edges
                .iter()
                .any(|e| e.to == EdgeTarget::External("svc".to_owned())),
            "{:?}",
            graph.edges
        );
    }

    #[test]
    fn split_makes_subpackages_components_and_resolves_dotted_imports() {
        let (files, manifests) = monolith();
        let graph = build(&files, &manifests, &["svc/src/svc".to_owned()]).unwrap();
        let paths: Vec<&str> = graph.components.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["svc/src/svc", "svc/src/svc/api", "svc/src/svc/store"]
        );
        let names: Vec<&str> = graph.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["svc", "api", "store"]);
        assert!(graph
            .components
            .iter()
            .all(|c| c.group == "svc/src/svc" || c.path == "svc/src/svc"));

        let internal: Vec<(&str, &str)> = graph
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTarget::Component(to) => Some((e.from.as_str(), to.as_str())),
                EdgeTarget::External(_) => None,
            })
            .collect();
        assert_eq!(
            internal,
            vec![
                ("svc/src/svc", "svc/src/svc/api"),
                ("svc/src/svc/api", "svc/src/svc/store")
            ]
        );
        // `typing` is still external; the package's own name never is.
        assert!(graph
            .edges
            .iter()
            .any(|e| e.to == EdgeTarget::External("typing".to_owned())));
        assert!(!graph
            .edges
            .iter()
            .any(|e| e.to == EdgeTarget::External("svc".to_owned())));
    }

    #[test]
    fn split_at_the_root_is_refused() {
        let (files, manifests) = monolith();
        for bad in ["", "/", "."] {
            assert!(
                build(&files, &manifests, &[bad.to_owned()]).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn focus_keeps_changed_components_their_neighbours_and_counts_the_rest() {
        let (base_files, manifests) = monolith();
        let mut head_files = base_files.clone();
        // store changes; api depends on it (neighbour); the package root does
        // not touch store and is unrelated to the change.
        head_files[2].1 = "class Db:\n    def ping(self): ...\n".to_owned();
        let split = ["svc/src/svc".to_owned()];
        let base = build(&base_files, &manifests, &split).unwrap();
        let head = build(&head_files, &manifests, &split).unwrap();
        let merged = diff(&base, &head);
        assert_eq!(merged.components.len(), 3);

        let focused = focus(&merged);
        let paths: Vec<&str> = focused.components.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, vec!["svc/src/svc/api", "svc/src/svc/store"]);
        assert_eq!(focused.omitted, 1);
        assert_eq!(
            focused.edges.len(),
            1,
            "only the edge into the changed component: {:?}",
            focused.edges
        );
        assert!(focused.change_counts().changed());
        assert!(render_mermaid(&focused, &ComponentRenderOptions::default())
            .contains("%% vizzle: focus: 1 unchanged component(s) not drawn"));
    }

    #[test]
    fn focus_on_an_unchanged_graph_draws_nothing_and_counts_everything() {
        let (files, manifests) = monolith();
        let split = ["svc/src/svc".to_owned()];
        let graph = build(&files, &manifests, &split).unwrap();
        let merged = diff(&graph, &graph);
        let focused = focus(&merged);
        assert!(focused.components.is_empty());
        assert_eq!(focused.omitted, 3);
    }

    #[test]
    fn zoom_draws_only_changed_classes_and_members_inside_changed_components() {
        let (base_files, manifests) = monolith();
        let mut head_files = base_files.clone();
        // Db gains ping(); Router (in api, unchanged component) is untouched.
        head_files[2].1 =
            "class Db:\n    def ping(self): ...\n    def close(self): ...\n".to_owned();
        let split = ["svc/src/svc".to_owned()];
        let base = build(&base_files, &manifests, &split).unwrap();
        let head = build(&head_files, &manifests, &split).unwrap();
        let merged = diff(&base, &head);

        let (classes, component_of) = zoom(&merged);
        let names: Vec<&str> = classes
            .classes
            .iter()
            .map(|c| c.qualified.as_str())
            .collect();
        assert_eq!(names, vec!["svc.src.svc.store.db.Db"], "{names:?}");
        assert_eq!(component_of["svc.src.svc.store.db.Db"], "store");

        let out = crate::mermaid::render(
            &classes,
            &crate::mermaid::RenderOptions {
                grouping: crate::mermaid::Grouping::Component,
                component_of,
                changed_members_only: true,
                ..Default::default()
            },
        );
        assert!(out.contains("namespace store {"), "{out}");
        assert!(out.contains("+ping() ✚"), "{out}");
        assert!(out.contains("+close() ✚"), "{out}");
        assert!(!out.contains("Router"), "{out}");
    }

    #[test]
    fn zoom_hides_unchanged_members_behind_a_count() {
        let files_base = pairs(&[(
            "svc/src/svc/store/db.py",
            "class Db:\n    def a(self): ...\n    def b(self): ...\n    def c(self): ...\n",
        )]);
        let files_head = pairs(&[(
            "svc/src/svc/store/db.py",
            "class Db:\n    def a(self): ...\n    def b(self): ...\n    def c(self): ...\n    def d(self): ...\n",
        )]);
        let manifests = pairs(&[("svc/pyproject.toml", "[project]\nname = \"svc\"\n")]);
        let split = ["svc/src/svc".to_owned()];
        let merged = diff(
            &build(&files_base, &manifests, &split).unwrap(),
            &build(&files_head, &manifests, &split).unwrap(),
        );
        let (classes, component_of) = zoom(&merged);
        let out = crate::mermaid::render(
            &classes,
            &crate::mermaid::RenderOptions {
                grouping: crate::mermaid::Grouping::Component,
                component_of,
                changed_members_only: true,
                ..Default::default()
            },
        );
        assert!(out.contains("+d() ✚"), "{out}");
        assert!(out.contains("… 3 unchanged members"), "{out}");
        assert!(!out.contains("+a()"), "{out}");
    }

    #[test]
    fn zoom_of_an_unchanged_diff_is_empty() {
        let (files, manifests) = monolith();
        let graph = build(&files, &manifests, &["svc/src/svc".to_owned()]).unwrap();
        let (classes, _) = zoom(&diff(&graph, &graph));
        assert!(classes.classes.is_empty());
    }

    #[test]
    fn diff_names_the_files_that_changed_in_each_component() {
        let (base_files, manifests) = monolith();
        let mut head_files = base_files.clone();
        head_files[2].1 = "class Db:\n    def ping(self): ...\n".to_owned(); // store/db.py changed
        head_files.push((
            "svc/src/svc/store/cache.py".to_owned(),
            "class Cache: ...\n".to_owned(),
        ));
        head_files.push((
            "svc/src/svc/audit/log.py".to_owned(),
            "class Log: ...\n".to_owned(),
        )); // new component
        let split = ["svc/src/svc".to_owned()];
        let merged = diff(
            &build(&base_files, &manifests, &split).unwrap(),
            &build(&head_files, &manifests, &split).unwrap(),
        );
        let by_path: HashMap<&str, &Component> = merged
            .components
            .iter()
            .map(|c| (c.path.as_str(), c))
            .collect();
        assert_eq!(
            by_path["svc/src/svc/store"].changed_files,
            vec!["cache.py", "db.py"]
        );
        assert_eq!(by_path["svc/src/svc/store"].change, ChangeKind::Modified);
        assert!(by_path["svc/src/svc/api"].changed_files.is_empty());
        assert_eq!(by_path["svc/src/svc/audit"].change, ChangeKind::Added);
        assert_eq!(by_path["svc/src/svc/audit"].changed_files, vec!["log.py"]);
        let json: serde_json::Value = serde_json::from_str(&to_json(&merged, false)).unwrap();
        let store = json["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["path"] == "svc/src/svc/store")
            .unwrap();
        assert_eq!(
            store["changedFiles"],
            serde_json::json!(["cache.py", "db.py"])
        );
    }

    #[test]
    fn scope_drops_boundary_to_boundary_edges() {
        // bridge imports both core and svc; svc imports core.
        // When scoped to pkgs/core, both svc and bridge are boundary.
        // The bridge→svc edge is boundary-to-boundary and must be dropped.
        let (files, manifests) = workspace_with_cross_boundary_edge();
        let graph = build(&files, &manifests, &[]).unwrap();
        let scoped = scope(&graph, "pkgs/core");

        let paths: BTreeSet<&str> = scoped.components.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(
            paths,
            BTreeSet::from(["pkgs/core", "apps/svc", "apps/bridge"]),
            "in-scope core + boundary svc and bridge"
        );

        assert!(
            !scoped
                .components
                .iter()
                .find(|c| c.path == "pkgs/core")
                .unwrap()
                .is_boundary
        );
        assert!(
            scoped
                .components
                .iter()
                .find(|c| c.path == "apps/svc")
                .unwrap()
                .is_boundary
        );
        assert!(
            scoped
                .components
                .iter()
                .find(|c| c.path == "apps/bridge")
                .unwrap()
                .is_boundary
        );

        // Edges to/from pkgs/core survive; the bridge→svc boundary-to-boundary edge is dropped.
        let internal_edges: BTreeSet<(&str, &str)> = scoped
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTarget::Component(to) => Some((e.from.as_str(), to.as_str())),
                EdgeTarget::External(_) => None,
            })
            .collect();
        assert_eq!(
            internal_edges,
            BTreeSet::from([("apps/svc", "pkgs/core"), ("apps/bridge", "pkgs/core")]),
            "both in-boundary→in-scope edges kept; boundary→boundary edge dropped"
        );
    }
}
