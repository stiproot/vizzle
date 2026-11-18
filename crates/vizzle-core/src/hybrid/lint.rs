//! Lint: the picture must keep its own promises.

use super::layout::{rect, Laid, Layout, Rect};
use super::route::{Label, Point};
use super::spec::{Edge, Node, NodeType};

/// A drawn element lint checks for collisions: a title, a module box, a band.
#[derive(Debug, Clone)]
pub struct Leaf {
    pub what: String,
    pub r: Rect,
    /// The node it belongs to; `None` for the title, legend and boundary labels.
    pub node: Option<String>,
    pub owner: Option<String>,
    /// The whole node is this one leaf (a box, a store, a band).
    pub is_node: bool,
}

impl Leaf {
    /// A leaf belonging to no node.
    pub fn free(what: impl Into<String>, r: Rect) -> Self {
        Leaf {
            what: what.into(),
            r,
            node: None,
            owner: None,
            is_node: false,
        }
    }

    /// A leaf belonging to node `id`; `is_node` when it is the whole node.
    pub fn of(id: &str, what: impl Into<String>, r: Rect, is_node: bool) -> Self {
        Leaf {
            what: what.into(),
            r,
            node: Some(id.to_owned()),
            owner: Some(id.to_owned()),
            is_node,
        }
    }
}

/// An edge after routing: its spec, its points and its placed label.
pub struct Routed<'a> {
    pub e: &'a Edge,
    pub pts: Vec<Point>,
    pub lab: Option<Label>,
}

fn overlap(a: &Rect, b: &Rect, m: f64) -> bool {
    a.x < b.x2 - m && b.x < a.x2 - m && a.y < b.y2 - m && b.y < a.y2 - m
}

fn inside(a: &Rect, b: &Rect) -> bool {
    a.x >= b.x && a.x2 <= b.x2 && a.y >= b.y && a.y2 <= b.y2
}

fn seg_rect(a: Point, b: Point) -> Rect {
    Rect {
        x: a[0].min(b[0]) - 0.5,
        y: a[1].min(b[1]) - 0.5,
        x2: a[0].max(b[0]) + 0.5,
        y2: a[1].max(b[1]) + 0.5,
    }
}

fn seg_rects(pts: &[Point]) -> Vec<Rect> {
    pts.windows(2).map(|w| seg_rect(w[0], w[1])).collect()
}

fn is_container(n: &Node) -> bool {
    matches!(
        n.kind,
        NodeType::Component | NodeType::Boundary | NodeType::Tile
    )
}

pub fn lint(
    canvas_width: f64,
    layout: &Layout,
    leaves: &[Leaf],
    edges: &[Routed],
    lint_err: &mut Vec<String>,
) {
    let laid = |id: &str| -> &Laid { &layout.nodes[layout.by_id[id]] };
    let containers: Vec<&Laid> = layout.nodes.iter().filter(|l| is_container(l.n)).collect();
    let ancestors = |id: &str| -> Vec<&str> {
        let r = rect(laid(id));
        containers
            .iter()
            .filter(|c| c.n.id != id && inside(&r, &rect(c)))
            .map(|c| c.n.id.as_str())
            .collect()
    };
    for i in 0..leaves.len() {
        for j in i + 1..leaves.len() {
            if overlap(&leaves[i].r, &leaves[j].r, 0.0) {
                lint_err.push(format!("overlap: {} × {}", leaves[i].what, leaves[j].what));
            }
        }
    }
    let comps: Vec<&Laid> = layout
        .nodes
        .iter()
        .filter(|l| l.n.kind != NodeType::Boundary)
        .collect();
    let nests = |a: &Laid, b: &Laid| {
        (a.n.kind == NodeType::Tile && inside(&rect(b), &rect(a)))
            || (b.n.kind == NodeType::Tile && inside(&rect(a), &rect(b)))
    };
    for i in 0..comps.len() {
        for j in i + 1..comps.len() {
            if nests(comps[i], comps[j]) {
                continue;
            }
            if overlap(&rect(comps[i]), &rect(comps[j]), 0.0) {
                lint_err.push(format!(
                    "overlap: node {} × node {}",
                    comps[i].n.id, comps[j].n.id
                ));
            }
        }
    }
    for l in &layout.nodes {
        let n = l.n;
        if let Some(host) = &n.inside {
            if !inside(&rect(l), &rect(laid(host))) {
                lint_err.push(format!("{} spills out of {host}", n.id));
            }
        }
    }
    let framed: Vec<&Laid> = layout
        .nodes
        .iter()
        .filter(|l| matches!(l.n.kind, NodeType::Component | NodeType::Tile))
        .collect();
    for e in edges {
        let (from, to) = (e.e.from.as_str(), e.e.to.as_str());
        let ends = [from, to];
        let mut ok_containers: Vec<&str> = vec![from, to];
        ok_containers.extend(ancestors(from));
        ok_containers.extend(ancestors(to));
        for s in seg_rects(&e.pts) {
            for l in leaves {
                if let Some(node) = l.node.as_deref() {
                    if ends.contains(&node) || (ok_containers.contains(&node) && l.is_node) {
                        continue;
                    }
                    if l.owner.as_deref().is_some_and(|o| ends.contains(&o)) {
                        continue;
                    }
                }
                if overlap(&s, &l.r, 1.0) {
                    lint_err.push(format!("edge {from}→{to} runs through {}", l.what));
                }
            }
            for c in &framed {
                if ok_containers.contains(&c.n.id.as_str()) {
                    continue;
                }
                if overlap(&s, &rect(c), 1.0) {
                    lint_err.push(format!(
                        "edge {from}→{to} runs through component {}",
                        c.n.id
                    ));
                }
            }
        }
        let Some(lab) = &e.lab else { continue };
        let lb = &lab.bx;
        let label = &e.e.label;
        for l in leaves {
            if overlap(lb, &l.r, 0.0) {
                lint_err.push(format!("label \"{label}\" overlaps {}", l.what));
            }
        }
        for c in &containers {
            let rc = rect(c);
            if overlap(lb, &rc, 0.0) && !inside(lb, &rc) {
                lint_err.push(format!(
                    "label \"{label}\" straddles the border of {}",
                    c.n.id
                ));
            }
        }
        for o in edges {
            for s in seg_rects(&o.pts) {
                if overlap(lb, &s, 0.0) {
                    lint_err.push(format!(
                        "label \"{label}\" is crossed by edge {}→{}",
                        o.e.from, o.e.to
                    ));
                }
            }
            if !std::ptr::eq(o, e) {
                if let Some(other) = &o.lab {
                    if overlap(lb, &other.bx, 0.0) {
                        lint_err.push(format!(
                            "label \"{label}\" overlaps label \"{}\"",
                            o.e.label
                        ));
                    }
                }
            }
        }
        if lb.x < 0.0 || lb.x2 > canvas_width {
            lint_err.push(format!("label \"{label}\" leaves the canvas"));
        }
    }
}
