//! The lens a reader puts on a diagram: light the elements a question is
//! about and dim the rest (`highlight`), or cut the diagram down to the
//! neighbourhood of one element (`around` + `depth`).
//!
//! Why this is vizzle's job and not the consumer's: an agent explaining an
//! incident used to append its own `classDef`/`cssClass` layer to the
//! generated mermaid, which meant knowing vizzle's internal ids, hoping the
//! text stayed appendable, and re-inventing a palette vizzle already owns.
//! Measured 2026-09-22 with mermaid-cli 11.17: both `cssClass "id" name` and
//! the inline `class id[...]:::name` form render, with and without
//! namespaces; the inline form is used here because it cannot be separated
//! from the line it styles.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use anyhow::{bail, Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::component::{ComponentGraph, EdgeTarget};
use crate::model::CodeGraph;
use crate::resolve::{resolve_all_relations, Target};

/// What the reader asked to see. Patterns are names or globs, matched against
/// an element's short name and its full name (a class's qualified name, a
/// component's path), so `Worker`, `pkg.orchestrator.*` and `orchestrator`
/// all work.
#[derive(Debug, Clone, Default)]
pub struct Lens {
    pub highlight: Vec<String>,
    pub around: Vec<String>,
    /// Hops kept around each `around` centre; 1 is the immediate neighbours.
    pub depth: usize,
}

impl Lens {
    pub fn is_empty(&self) -> bool {
        self.highlight.is_empty() && self.around.is_empty()
    }
}

fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid pattern `{pattern}`"))?);
    }
    Ok(builder.build()?)
}

/// The full names among `candidates` (`(short, full)` pairs) that any
/// pattern matches. A pattern that matches nothing is an error naming what
/// was available, so a typo is caught rather than drawn as an empty lens.
pub fn select<'a>(
    what: &str,
    patterns: &[String],
    candidates: impl Iterator<Item = (&'a str, &'a str)>,
) -> Result<BTreeSet<String>> {
    if patterns.is_empty() {
        return Ok(BTreeSet::new());
    }
    let candidates: Vec<(&str, &str)> = candidates.collect();
    let mut chosen = BTreeSet::new();
    for pattern in patterns {
        let set = globs(std::slice::from_ref(pattern))?;
        let mut hit = false;
        for (short, full) in &candidates {
            if set.is_match(short) || set.is_match(full) {
                chosen.insert((*full).to_owned());
                hit = true;
            }
        }
        if !hit {
            let mut names: Vec<&str> = candidates.iter().map(|(short, _)| *short).collect();
            names.sort_unstable();
            names.dedup();
            let shown: Vec<&str> = names.iter().copied().take(40).collect();
            let more = if names.len() > shown.len() {
                format!(" … and {} more", names.len() - shown.len())
            } else {
                String::new()
            };
            bail!(
                "no {what} matches `{pattern}`; available: {}{more}",
                shown.join(", ")
            );
        }
    }
    Ok(chosen)
}

/// Undirected breadth-first walk over `edges` from `centres`, `depth` hops.
fn neighbourhood(
    centres: &BTreeSet<String>,
    edges: &[(String, String)],
    depth: usize,
) -> HashSet<String> {
    let mut adjacent: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in edges {
        adjacent.entry(a.as_str()).or_default().push(b.as_str());
        adjacent.entry(b.as_str()).or_default().push(a.as_str());
    }
    let mut seen: HashSet<String> = centres.iter().cloned().collect();
    let mut queue: VecDeque<(&str, usize)> = centres.iter().map(|c| (c.as_str(), 0)).collect();
    while let Some((node, d)) = queue.pop_front() {
        if d == depth {
            continue;
        }
        for next in adjacent.get(node).into_iter().flatten() {
            if seen.insert((*next).to_owned()) {
                queue.push_back((next, d + 1));
            }
        }
    }
    seen
}

/// The classes within `depth` relations of the centres, plus the centres.
/// Relations to external types do not count as hops.
pub fn around_classes(graph: &CodeGraph, centres: &BTreeSet<String>, depth: usize) -> CodeGraph {
    let edges: Vec<(String, String)> = resolve_all_relations(graph)
        .into_iter()
        .filter_map(|r| match r.to {
            Target::Internal(to) => Some((r.from, to)),
            Target::External(_) => None,
        })
        .collect();
    let keep = neighbourhood(centres, &edges, depth);
    CodeGraph {
        classes: graph
            .classes
            .iter()
            .filter(|c| keep.contains(&c.qualified))
            .cloned()
            .collect(),
        imports: graph.imports.clone(),
    }
}

/// The components within `depth` dependency edges of the centres, plus the
/// centres, with the edges among them. External targets do not count as hops.
pub fn around_components(
    graph: &ComponentGraph,
    centres: &BTreeSet<String>,
    depth: usize,
) -> ComponentGraph {
    let edges: Vec<(String, String)> = graph
        .edges
        .iter()
        .filter_map(|e| match &e.to {
            EdgeTarget::Component(to) => Some((e.from.clone(), to.clone())),
            EdgeTarget::External(_) => None,
        })
        .collect();
    let keep = neighbourhood(centres, &edges, depth);
    let mut out = ComponentGraph {
        components: graph
            .components
            .iter()
            .filter(|c| keep.contains(&c.path))
            .cloned()
            .collect(),
        edges: graph
            .edges
            .iter()
            .filter(|e| {
                keep.contains(&e.from)
                    && match &e.to {
                        EdgeTarget::Component(to) => keep.contains(to),
                        EdgeTarget::External(_) => true,
                    }
            })
            .cloned()
            .collect(),
        classes: graph
            .classes
            .iter()
            .filter(|pc| keep.contains(&pc.component))
            .cloned()
            .collect(),
        selection: graph.selection.clone(),
        omitted: graph.omitted + (graph.components.len() - keep.len().min(graph.components.len())),
    };
    out.omitted = graph.omitted + graph.components.len() - out.components.len();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_files;

    fn graph() -> CodeGraph {
        parse_files(&[(
            "pkg/mod.py".to_owned(),
            "class Base: ...\nclass Mid(Base): ...\nclass Leaf(Mid): ...\nclass Alone: ...\n"
                .to_owned(),
        )])
        .unwrap()
    }

    #[test]
    fn select_matches_short_or_full_names_and_globs() {
        let g = graph();
        let cands = || {
            g.classes
                .iter()
                .map(|c| (c.name.as_str(), c.qualified.as_str()))
        };
        let one = select("class", &["Mid".to_owned()], cands()).unwrap();
        assert_eq!(one.into_iter().collect::<Vec<_>>(), vec!["pkg.mod.Mid"]);
        let by_full = select("class", &["pkg.mod.Leaf".to_owned()], cands()).unwrap();
        assert_eq!(by_full.len(), 1);
        let glob = select("class", &["*a*".to_owned()], cands()).unwrap();
        assert_eq!(glob.len(), 2, "{glob:?}"); // Base, Leaf (Alone has no lowercase a)
        let prefix = select("class", &["pkg.mod.*".to_owned()], cands()).unwrap();
        assert_eq!(prefix.len(), 4, "{prefix:?}");
    }

    #[test]
    fn select_names_what_is_available_on_a_miss() {
        let g = graph();
        let err = select(
            "class",
            &["Nope".to_owned()],
            g.classes
                .iter()
                .map(|c| (c.name.as_str(), c.qualified.as_str())),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("no class matches `Nope`"), "{err}");
        assert!(err.contains("Alone, Base, Leaf, Mid"), "{err}");
    }

    #[test]
    fn around_keeps_the_centre_and_its_neighbours_within_depth() {
        let g = graph();
        let centre: BTreeSet<String> = ["pkg.mod.Mid".to_owned()].into_iter().collect();
        fn names(g: &CodeGraph) -> Vec<String> {
            let mut v: Vec<String> = g.classes.iter().map(|c| c.name.clone()).collect();
            v.sort_unstable();
            v
        }
        assert_eq!(names(&around_classes(&g, &centre, 0)), vec!["Mid"]);
        assert_eq!(
            names(&around_classes(&g, &centre, 1)),
            vec!["Base", "Leaf", "Mid"]
        );
        // Alone has no relation to anything; no depth reaches it.
        assert_eq!(
            names(&around_classes(&g, &centre, 5)),
            vec!["Base", "Leaf", "Mid"]
        );
    }
}
