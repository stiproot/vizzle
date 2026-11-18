//! Drift checks: spec ⇄ vizzle ⇄ tree.
//!
//! The core never touches the filesystem: whether a cited path is in the tree
//! is answered by the caller's `exists`.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use super::passes::ModuleGraph;
use super::spec::{Links, NodeType, Spec};

/// `<pass>:<module id>` split at its first colon.
pub fn split_ref(reference: &str) -> (&str, &str) {
    match reference.find(':') {
        Some(i) => (&reference[..i], &reference[i + 1..]),
        // The original sliced at index -1 here: everything but the last
        // character as the pass, the whole ref as the id. Kept, so a ref with
        // no pass fails the same way it always did (no pass matches).
        None => {
            let cut = reference.char_indices().last().map_or(0, |(i, _)| i);
            (&reference[..cut], reference)
        }
    }
}

/// What a component draws: the modules it runs (`own`) and the shared modules
/// its links band shows (`links`). The layout reads this.
#[derive(Debug, Clone, Default)]
pub struct Placement {
    pub own: Vec<String>,
    pub links: Vec<String>,
}

pub struct Reconciled {
    pub errors: Vec<String>,
    pub placement: HashMap<String, Placement>,
}

/// What reconcile works against: the module graph per pass, and whether a
/// tree path exists.
struct View<'a> {
    graphs: &'a BTreeMap<String, ModuleGraph>,
    exists: &'a dyn Fn(&str) -> bool,
}

impl View<'_> {
    fn has(&self, reference: &str) -> bool {
        let (p, id) = split_ref(reference);
        self.graphs
            .get(p)
            .is_some_and(|g| g.modules.iter().any(|m| m == id))
    }

    /// Every module reachable through vizzle's import edges from `refs`,
    /// excluding `refs` themselves.
    fn closure(&self, refs: &[&str]) -> HashSet<String> {
        let mut seen: HashSet<String> = refs.iter().map(|r| (*r).to_owned()).collect();
        let mut q: VecDeque<String> = refs.iter().map(|r| (*r).to_owned()).collect();
        while let Some(next) = q.pop_front() {
            let (p, id) = split_ref(&next);
            if let Some(g) = self.graphs.get(p) {
                for (a, b) in &g.edges {
                    let nb = format!("{p}:{b}");
                    if a == id && !seen.contains(&nb) {
                        seen.insert(nb.clone());
                        q.push_back(nb);
                    }
                }
            }
        }
        for r in refs {
            seen.remove(*r);
        }
        seen
    }

    fn reach(&self, own: &[String]) -> HashSet<String> {
        let live: Vec<&str> = own
            .iter()
            .map(String::as_str)
            .filter(|r| self.has(r))
            .collect();
        self.closure(&live)
    }
}

// rule: a shared or omitted module the spec names must still be one vizzle reports
fn check_stale_refs(spec: &Spec, view: &View, errors: &mut Vec<String>) {
    for r in &spec.shared {
        if !view.has(r) {
            errors.push(format!(
                "spec.shared names {r}, which vizzle no longer reports"
            ));
        }
    }
    for (r, why) in &spec.omit {
        if !view.has(r) {
            errors.push(format!(
                "spec.omit names {r} ({why}), which vizzle no longer reports — drop the omission"
            ));
        }
    }
}

// rule: a non-component node (box, store, tile …) that stands for a real thing may cite a source path
fn check_node_paths(spec: &Spec, view: &View, errors: &mut Vec<String>) {
    for n in &spec.nodes {
        if n.kind == NodeType::Component {
            continue;
        }
        for p in n.path.iter().flat_map(|p| p.items()) {
            if !(view.exists)(p) {
                errors.push(format!("{} cites {p}, which is gone from the tree", n.id));
            }
        }
    }
}

// rule: each module a component runs must still be reported, and any path it cites must exist
fn place_modules(
    n: &super::spec::Node,
    view: &View,
    errors: &mut Vec<String>,
    placed: &mut HashSet<String>,
) -> Vec<String> {
    let mut own = Vec::new();
    for m in &n.modules {
        for r in &m.refs {
            if !view.has(r) {
                errors.push(format!(
                    "{} › \"{}\" names {r}, which vizzle no longer reports",
                    n.id, m.name
                ));
            }
            own.push(r.clone());
            placed.insert(r.clone());
        }
        for p in m.path.iter().flat_map(|p| p.items()) {
            if !(view.exists)(p) {
                errors.push(format!(
                    "{} › \"{}\" cites {p}, which is gone from the tree",
                    n.id, m.name
                ));
            }
        }
    }
    own
}

// rule: a shared module a component claims to link needs an import path behind the claim
fn resolve_links(
    n: &super::spec::Node,
    own: &[String],
    shared: &[String],
    view: &View,
    errors: &mut Vec<String>,
) -> Vec<String> {
    let reach = view.reach(own);
    let claimed = match &n.links {
        Some(Links::Word(_)) => {
            return shared
                .iter()
                .filter(|s| reach.contains(*s))
                .cloned()
                .collect()
        }
        Some(Links::List(list)) => list,
        None => return Vec::new(),
    };
    for r in claimed {
        if !reach.contains(r) {
            errors.push(format!(
                "{} claims to link {r}, but vizzle reports no import path from its modules to it",
                n.id
            ));
        }
    }
    for s in claimed.iter().filter(|x| !shared.contains(x)) {
        errors.push(format!("{} links {s}, which is not in spec.shared", n.id));
    }
    shared
        .iter()
        .filter(|s| claimed.contains(s))
        .cloned()
        .collect()
}

fn check_components(
    spec: &Spec,
    view: &View,
    errors: &mut Vec<String>,
) -> (HashSet<String>, HashMap<String, Placement>) {
    let mut placed = HashSet::new();
    let mut placement = HashMap::new();
    for n in &spec.nodes {
        if n.kind != NodeType::Component {
            continue;
        }
        let own = place_modules(n, view, errors, &mut placed);
        let links = resolve_links(n, &own, &spec.shared, view, errors);
        placed.extend(links.iter().cloned());
        placement.insert(n.id.clone(), Placement { own, links });
    }
    (placed, placement)
}

// rule: an edge may claim imports (`"imports": [refs]`): each must be reachable through vizzle's
// import edges from the modules of the component the edge starts at — the same check as explicit links
fn check_edge_imports(
    spec: &Spec,
    view: &View,
    placement: &HashMap<String, Placement>,
    errors: &mut Vec<String>,
) {
    for e in &spec.edges {
        let Some(imports) = &e.imports else { continue };
        let own = placement
            .get(&e.from)
            .map(|p| p.own.as_slice())
            .unwrap_or(&[]);
        let reach = view.reach(own);
        for r in imports {
            if !view.has(r) {
                errors.push(format!(
                    "edge {}→{} claims an import of {r}, which vizzle no longer reports",
                    e.from, e.to
                ));
            } else if !reach.contains(r) {
                errors.push(format!(
                    "edge {}→{} claims {} imports {r}, but vizzle reports no import path from its modules to it",
                    e.from, e.to, e.from
                ));
            }
        }
    }
}

// rule: every module vizzle reports in scope is placed in a component or omitted with a reason
fn check_unplaced(spec: &Spec, view: &View, placed: &HashSet<String>, errors: &mut Vec<String>) {
    for p in &spec.vizzle {
        let Some(g) = view.graphs.get(p) else {
            continue;
        };
        for id in &g.modules {
            let r = format!("{p}:{id}");
            if !placed.contains(&r) && !spec.omits(&r) {
                errors.push(format!(
                    "vizzle reports {r} in scope, but the spec places it nowhere (place it in a component, or omit it with a reason)"
                ));
            }
        }
    }
}

/// The drift checks, spec ⇄ vizzle ⇄ tree. `graphs` maps each of
/// `spec.vizzle`'s passes to its module graph; `exists` answers whether a
/// repo-relative path is in the tree. Returns every drift found and the
/// placement per component id, which the layout reads.
pub fn reconcile(
    spec: &Spec,
    graphs: &BTreeMap<String, ModuleGraph>,
    exists: &dyn Fn(&str) -> bool,
) -> Reconciled {
    let mut errors = Vec::new();
    let view = View { graphs, exists };
    check_stale_refs(spec, &view, &mut errors);
    check_node_paths(spec, &view, &mut errors);
    let (placed, placement) = check_components(spec, &view, &mut errors);
    check_edge_imports(spec, &view, &placement, &mut errors);
    check_unplaced(spec, &view, &placed, &mut errors);
    Reconciled { errors, placement }
}
