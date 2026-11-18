//! Layout: size every node from its content, then place it on the canvas.
//!
//! A node's geometry starts as NaN and is filled in as placement reaches it,
//! the way the original read fields that were not set yet: a coordinate
//! expression or a boundary that names a node placed later sees that node's
//! raw spec value, not a guess.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};

use super::metrics::{max_of, min_of, r, text_width, wrap};
use super::reconcile::{split_ref, Placement};
use super::spec::{Expr, Module, Node, NodeType, RelativeY, YSpec};

pub const SUB_LH: f64 = 19.0;
pub const PAD: f64 = 20.0;
pub const GAP: f64 = 16.0;

/// An axis-aligned box, as lint and routing compare them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub x2: f64,
    pub y2: f64,
}

/// One module box inside a component, relative to the component's origin.
#[derive(Debug)]
pub struct Cell<'a> {
    pub m: &'a Module,
    pub lines: Vec<String>,
    pub dx: f64,
    pub dy: f64,
    pub w: f64,
    pub h: f64,
}

/// A component's links band.
#[derive(Debug)]
pub struct Band {
    pub lines: Vec<String>,
    pub head: String,
    pub dy: f64,
    pub h: f64,
}

/// A spec node with the geometry and wrapped text the layout gave it.
#[derive(Debug)]
pub struct Laid<'a> {
    pub n: &'a Node,
    pub x0: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// A component's stack lines, or a tile's sub lines.
    pub stack: Vec<String>,
    pub head_h: f64,
    pub cells: Vec<Cell<'a>>,
    pub band: Option<Band>,
    /// A box's, store's or band's wrapped sub-text.
    pub lines: Vec<String>,
    pub foot: Vec<String>,
    pub foot_h: f64,
}

impl<'a> Laid<'a> {
    fn new(n: &'a Node) -> Self {
        let y = match &n.y {
            Some(YSpec::At(v)) => *v,
            _ => f64::NAN,
        };
        Laid {
            n,
            x0: f64::NAN,
            y,
            w: f64::NAN,
            h: f64::NAN,
            stack: Vec::new(),
            head_h: 0.0,
            cells: Vec::new(),
            band: None,
            lines: Vec::new(),
            foot: Vec::new(),
            foot_h: 0.0,
        }
    }

    pub fn rect(&self) -> Rect {
        rect(self)
    }
}

pub fn rect(n: &Laid) -> Rect {
    Rect {
        x: n.x0,
        y: n.y,
        x2: n.x0 + n.w,
        y2: n.y + n.h,
    }
}

/// Every node laid out, in spec order, with a lookup by id.
pub struct Layout<'a> {
    pub nodes: Vec<Laid<'a>>,
    pub by_id: HashMap<&'a str, usize>,
}

impl<'a> Layout<'a> {
    pub fn get(&self, id: &str) -> Result<&Laid<'a>> {
        self.by_id
            .get(id)
            .map(|&i| &self.nodes[i])
            .ok_or_else(|| anyhow!("names unknown node {id:?}"))
    }

    /// Resolve a coordinate: a number, `a~b` (the midpoint), or
    /// `<id>.<top|bottom|left|right|cx|cy>[±N]`. `None` stays `None`.
    pub fn val(&self, expr: Option<&Expr>) -> Result<Option<f64>> {
        match expr {
            None => Ok(None),
            Some(Expr::Num(v)) => Ok(Some(*v)),
            Some(Expr::Str(s)) => self.val_str(s).map(Some),
        }
    }

    fn val_str(&self, expr: &str) -> Result<f64> {
        let mid: Vec<&str> = expr.split('~').collect();
        if mid.len() == 2 {
            return Ok((self.val_str(mid[0])? + self.val_str(mid[1])?) / 2.0);
        }
        let bad = || anyhow!("bad coordinate expression {expr:?}");
        let (id, rest) = expr.split_once('.').ok_or_else(bad)?;
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(bad());
        }
        let cut = rest.find(['+', '-']).unwrap_or(rest.len());
        let (anchor, offset) = rest.split_at(cut);
        let offset = if offset.is_empty() {
            0.0
        } else {
            let digits = &offset[1..];
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return Err(bad());
            }
            offset.parse::<f64>().map_err(|_| bad())?
        };
        let n = self
            .get(id)
            .map_err(|_| anyhow!("coordinate expression names unknown node {id:?}"))?;
        let v = match anchor {
            "top" => n.y,
            "bottom" => n.y + n.h,
            "left" => n.x0,
            "right" => n.x0 + n.w,
            "cx" => n.x0 + n.w / 2.0,
            "cy" => n.y + n.h / 2.0,
            _ => return Err(bad()),
        };
        Ok(v + offset)
    }
}

struct ContentHeight {
    lines: Vec<String>,
    h: f64,
}

fn box_content_height(sub: Option<&str>, w: f64) -> ContentHeight {
    let lines = sub
        .map(|s| wrap(s, w - 24.0, 14.0, false, false))
        .unwrap_or_default();
    let h = content_height(lines.len());
    ContentHeight { lines, h }
}

/// The height of a two-line box's content (title, then its sub lines). The
/// layout sizes boxes by it and the emitter centres their text by it.
pub fn content_height(lines: usize) -> f64 {
    28.0 + if lines == 0 {
        0.0
    } else {
        24.0 + (lines as f64 - 1.0) * SUB_LH
    } + 18.0
}

pub fn measure(l: &mut Laid, lint_err: &mut Vec<String>, links: &[String]) {
    let n = l.n;
    let [x0, x1] = n.x.unwrap_or([f64::NAN, f64::NAN]);
    let w = x1 - x0;
    l.w = w;
    match n.kind {
        NodeType::Component => {
            let inner = w - 2.0 * PAD;
            if text_width(&n.name, 17.0, true, false) > inner {
                lint_err.push(format!(
                    "{}: title \"{}\" is wider than its box",
                    n.id, n.name
                ));
            }
            l.stack = n
                .stack
                .as_deref()
                .map(|s| wrap(s, inner, 14.0, false, false))
                .unwrap_or_default();
            let mut y = 32.0 + l.stack.len() as f64 * SUB_LH + 16.0;
            l.head_h = y;
            let cols = n.cols.unwrap_or_else(|| n.modules.len().max(1));
            let cols_f = cols as f64;
            let cell_w = (inner - (cols_f - 1.0) * GAP) / cols_f;
            // pack modules into grid rows; a module may span several columns
            let mut grid_rows: Vec<Vec<(&Module, usize, usize)>> = Vec::new();
            let mut cur = Vec::new();
            let mut used = 0usize;
            for m in &n.modules {
                let span = m.span.unwrap_or(1).min(cols);
                if used + span > cols {
                    grid_rows.push(std::mem::take(&mut cur));
                    used = 0;
                }
                cur.push((m, used, span));
                used += span;
            }
            if !cur.is_empty() {
                grid_rows.push(cur);
            }
            for gr in grid_rows {
                let row: Vec<(&Module, usize, f64, ContentHeight)> = gr
                    .into_iter()
                    .map(|(m, col, span)| {
                        let span_f = span as f64;
                        let cw = span_f * cell_w + (span_f - 1.0) * GAP;
                        (m, col, cw, box_content_height(m.sub.as_deref(), cw))
                    })
                    .collect();
                let rh = max_of(row.iter().map(|c| c.3.h));
                for (m, col, cw, c) in row {
                    if text_width(&m.name, 17.0, true, false) > cw - 16.0 {
                        lint_err.push(format!(
                            "{} › \"{}\" is wider than its box ({}px)",
                            n.id,
                            m.name,
                            super::metrics::num(r(cw))
                        ));
                    }
                    l.cells.push(Cell {
                        m,
                        lines: c.lines,
                        dx: PAD + col as f64 * (cell_w + GAP),
                        dy: y,
                        w: cw,
                        h: rh,
                    });
                }
                y += rh + GAP;
            }
            if !n.modules.is_empty() {
                y -= GAP;
            }
            l.band = None;
            if !links.is_empty() {
                let names: Vec<&str> = links
                    .iter()
                    .map(|reference| split_ref(reference).1.rsplit('/').next().unwrap_or(""))
                    .collect();
                let head = format!("{} — ", n.links_label.as_deref().unwrap_or("links"));
                let lines = wrap(
                    &format!("{head}{}", names.join(" · ")),
                    inner - 28.0,
                    14.0,
                    false,
                    false,
                );
                let bh = 18.0 + lines.len() as f64 * SUB_LH;
                l.band = Some(Band {
                    lines,
                    head,
                    dy: y + 14.0,
                    h: bh,
                });
                y += 14.0 + bh;
            }
            l.h = y + PAD;
        }
        NodeType::Store => {
            let c = box_content_height(n.sub.as_deref(), w);
            l.lines = c.lines;
            l.h = c.h + 52.0;
            if text_width(&n.name, 17.0, true, false) > w - 16.0 {
                lint_err.push(format!(
                    "{}: \"{}\" is wider than its cylinder",
                    n.id, n.name
                ));
            }
        }
        NodeType::Band => {
            l.lines = wrap(
                &format!("{} — {}", n.name, n.text),
                w - 40.0,
                14.0,
                false,
                false,
            );
            l.h = 22.0 + l.lines.len() as f64 * SUB_LH;
        }
        NodeType::Tile => {
            // a fixed-size frame with a header (bold name, grey sub) and a monospace foot; other nodes sit
            // inside it via `in`, so a gallery of small graphs shares one grammar
            let inner = w - 40.0;
            if text_width(&n.name, 17.0, true, false) > inner {
                lint_err.push(format!(
                    "{}: title \"{}\" is wider than its tile",
                    n.id, n.name
                ));
            }
            l.stack = n
                .sub
                .as_deref()
                .map(|s| wrap(s, inner, 14.0, false, false))
                .unwrap_or_default();
            l.head_h = 32.0 + l.stack.len() as f64 * SUB_LH + 12.0;
            // a foot may break its own lines with \n; a line's leading spaces indent it and its wraps
            l.foot = n
                .foot
                .as_deref()
                .map(|foot| {
                    foot.split('\n')
                        .flat_map(|line| {
                            let rest = line.trim_start_matches(' ');
                            let ind = &line[..line.len() - rest.len()];
                            let max_w = w - 56.0 - text_width(ind, 13.0, false, true);
                            wrap(rest, max_w, 13.0, false, true)
                                .into_iter()
                                .map(move |x| format!("{ind}{x}"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            l.foot_h = if l.foot.is_empty() {
                0.0
            } else {
                16.0 + l.foot.len() as f64 * 18.0
            };
            l.h = n.height.unwrap_or(f64::NAN);
        }
        NodeType::Box => {
            let c = box_content_height(n.sub.as_deref(), w);
            l.lines = c.lines;
            l.h = c.h.max(n.min_h.unwrap_or(0.0));
            if text_width(&n.name, 17.0, true, false) > w - 16.0 {
                lint_err.push(format!("{}: \"{}\" is wider than its box", n.id, n.name));
            }
        }
        NodeType::Boundary => {}
    }
}

/// Size every node, equalize rows, then place each in spec order.
pub fn place<'a>(
    nodes: &'a [Node],
    placement: &HashMap<String, Placement>,
    lint_err: &mut Vec<String>,
) -> Result<Layout<'a>> {
    let mut layout = Layout {
        nodes: nodes.iter().map(Laid::new).collect(),
        by_id: nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect(),
    };
    // `in: <id>` makes a node's x and numeric y offsets from that (earlier-placed) node's origin
    for (i, n) in nodes.iter().enumerate() {
        if n.kind == NodeType::Boundary {
            continue;
        }
        let ox = match &n.inside {
            Some(t) => {
                let host = layout.get(t)?.n;
                host.x
                    .ok_or_else(|| anyhow!("{}: `in` names {t}, which has no `x`", n.id))?[0]
            }
            None => 0.0,
        };
        let links = placement
            .get(&n.id)
            .map(|p| p.links.as_slice())
            .unwrap_or(&[]);
        let l = &mut layout.nodes[i];
        l.x0 = n.x.map_or(f64::NAN, |x| x[0]) + ox;
        measure(l, lint_err, links);
    }
    // equalize heights within a row so siblings line up
    let mut rows: Vec<(&str, Vec<usize>)> = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        if let Some(row) = &n.row {
            match rows.iter_mut().find(|(k, _)| *k == row.as_str()) {
                Some((_, members)) => members.push(i),
                None => rows.push((row.as_str(), vec![i])),
            }
        }
    }
    for (_, group) in &rows {
        let h = max_of(group.iter().map(|&i| layout.nodes[i].h));
        for &i in group {
            let l = &mut layout.nodes[i];
            if l.n.kind == NodeType::Component {
                let grow = h - l.h;
                if let Some(band) = &mut l.band {
                    band.dy += grow;
                }
            }
            l.h = h;
        }
    }
    for (i, n) in nodes.iter().enumerate() {
        if n.kind == NodeType::Boundary {
            let kids: Vec<&Laid> = n
                .contains
                .iter()
                .map(|id| layout.get(id))
                .collect::<Result<_>>()?;
            let [pt, pr, pb, pl] = n.pad.unwrap_or([56.0, 30.0, 30.0, 30.0]);
            let x0 = min_of(kids.iter().map(|k| k.x0)) - pl;
            let y = min_of(kids.iter().map(|k| k.y)) - pt;
            let w = max_of(kids.iter().map(|k| k.x0 + k.w)) + pr - x0;
            let h = max_of(kids.iter().map(|k| k.y + k.h)) + pb - y;
            let l = &mut layout.nodes[i];
            (l.x0, l.y, l.w, l.h) = (x0, y, w, h);
            continue;
        }
        let y = match &n.y {
            Some(YSpec::At(v)) => {
                let base = match &n.inside {
                    Some(t) => layout.get(t)?.y,
                    None => 0.0,
                };
                Some(v + base)
            }
            Some(YSpec::Relative(rel)) => relative_y(&layout, rel)?,
            None => bail!("{}: needs `y`", n.id),
        };
        if let Some(y) = y {
            layout.nodes[i].y = y;
        }
    }
    // The JS drew NaN for a node it never placed, and every lint comparison against NaN is false, so the picture
    // passed. Refuse instead (exit 2), naming the node.
    for l in &layout.nodes {
        if ![l.x0, l.y, l.w, l.h].iter().all(|v| v.is_finite()) {
            bail!(
                "{}: never placed (its x, y, width or height is not a number): give it an `x` and a `y` that \
                 names a node defined before it",
                l.n.id
            );
        }
    }
    Ok(layout)
}

/// `{below, gap}`, `{top, dy}` or `{at}`, tried in that order. A form with none of them is refused, and so is a
/// reference to a node not yet placed (one defined later in the spec): both drew NaN in the JS.
fn relative_y(layout: &Layout, rel: &RelativeY) -> Result<Option<f64>> {
    if let Some(below) = &rel.below {
        let bottoms = below
            .items()
            .into_iter()
            .map(|id| {
                let o = layout.get(id)?;
                if !o.y.is_finite() {
                    bail!("`below` names {id}, which is placed later in the spec: move it first");
                }
                Ok(o.y + o.h)
            })
            .collect::<Result<Vec<f64>>>()?;
        return Ok(Some(max_of(bottoms) + rel.gap.unwrap_or(80.0)));
    }
    if let Some(top) = rel.top.as_deref().filter(|t| !t.is_empty()) {
        let y = layout.val_str(&format!("{top}.top"))?;
        if !y.is_finite() {
            bail!("`top` names {top}, which is placed later in the spec: move it first");
        }
        return Ok(Some(y + rel.dy.unwrap_or(0.0)));
    }
    if rel.at.is_some() {
        return layout.val(rel.at.as_ref());
    }
    bail!("a relative `y` needs one of `below`, `top` or `at`")
}
