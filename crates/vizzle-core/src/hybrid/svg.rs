//! SVG emit: one emitter per node type, the edges and their labels, and the
//! legend. Every colour comes from `palette`.

use super::layout::{content_height, rect, Laid, Layout, Rect, PAD, SUB_LH};
use super::lint::{Leaf, Routed};
use super::metrics::{esc, max_of, num, r, rn, text_width, wrap, MONO_FONT};
use super::spec::{family_or_default, EdgeKind, Family, LegendKind, NodeType, Spec};
use crate::palette::{
    HybridFamily, HYBRID_BOUNDARY_FILL, HYBRID_INK, HYBRID_MUTED, HYBRID_RULE, HYBRID_SHARED,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Middle,
    Start,
}

pub fn text_lines(
    lines: &[String],
    x: f64,
    y0: f64,
    cls: &str,
    anchor: Anchor,
    extra: &str,
) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            format!(
                "<text x=\"{}\" y=\"{}\"{} class=\"{cls}\"{extra}>{}</text>",
                rn(x),
                rn(y0 + i as f64 * SUB_LH),
                if anchor == Anchor::Middle {
                    " text-anchor=\"middle\""
                } else {
                    ""
                },
                esc(l)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_arguments)]
fn two_line_box(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    name: &str,
    lines: &[String],
    fam: Family,
    sw: f64,
    rx: f64,
) -> String {
    let f = fam.colors();
    let cx = x + w / 2.0;
    let top = y + (h - content_height(lines.len())) / 2.0;
    [
        format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",
            rn(x),
            rn(y),
            rn(w),
            rn(h),
            num(rx),
            f.inner,
            f.stroke,
            num(sw)
        ),
        format!("<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" class=\"t\">{}</text>", rn(cx), rn(top + 28.0), esc(name)),
        text_lines(lines, cx, top + 52.0, "small", Anchor::Middle, ""),
    ]
    .join("\n")
}

/// One emitter per node type: pushes the node's SVG onto `out` and its lint
/// leaves onto `leaves`.
fn emit_component(n: &Laid, f: &HybridFamily, out: &mut Vec<String>, leaves: &mut Vec<Leaf>) {
    let id = &n.n.id;
    let name = &n.n.name;
    out.push(format!("<g id=\"{id}\">"));
    out.push(format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"12\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2.5\"/>",
        rn(n.x0),
        rn(n.y),
        rn(n.w),
        rn(n.h),
        f.outer,
        f.stroke
    ));
    out.push(format!(
        "<text x=\"{}\" y=\"{}\" class=\"t\" style=\"fill:{}\">{}</text>",
        rn(n.x0 + PAD),
        rn(n.y + 32.0),
        f.stroke,
        esc(name)
    ));
    out.push(text_lines(
        &n.stack,
        n.x0 + PAD,
        n.y + 32.0 + 22.0,
        "small",
        Anchor::Start,
        "",
    ));
    let tw = max_of(
        std::iter::once(text_width(name, 17.0, true, false))
            .chain(n.stack.iter().map(|l| text_width(l, 14.0, false, false))),
    );
    leaves.push(Leaf::of(
        id,
        format!("title of {id}"),
        Rect {
            x: n.x0 + PAD,
            y: n.y + 14.0,
            x2: n.x0 + PAD + tw,
            y2: n.y + n.head_h - 8.0,
        },
        false,
    ));
    for c in &n.cells {
        let fam = c.m.family.or(n.n.family).unwrap_or(Family::Primary);
        out.push(two_line_box(
            n.x0 + c.dx,
            n.y + c.dy,
            c.w,
            c.h,
            &c.m.name,
            &c.lines,
            fam,
            2.0,
            8.0,
        ));
        leaves.push(Leaf::of(
            id,
            format!("{id} › {}", c.m.name),
            Rect {
                x: n.x0 + c.dx,
                y: n.y + c.dy,
                x2: n.x0 + c.dx + c.w,
                y2: n.y + c.dy + c.h,
            },
            false,
        ));
    }
    if let Some(band) = &n.band {
        let s = &HYBRID_SHARED;
        let bx = n.x0 + PAD;
        let by = n.y + band.dy;
        let bw = n.w - 2.0 * PAD;
        out.push(format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.5\"/>",
            rn(bx),
            rn(by),
            rn(bw),
            rn(band.h),
            s.inner,
            s.stroke
        ));
        for (i, l) in band.lines.iter().enumerate() {
            let y = by + 9.0 + 15.0 + i as f64 * SUB_LH;
            match l.strip_prefix(band.head.as_str()).filter(|_| i == 0) {
                Some(rest) => {
                    let label = band.head.strip_suffix(" — ").unwrap_or(&band.head);
                    out.push(format!(
                        "<text x=\"{}\" y=\"{}\" class=\"lbl\"><tspan class=\"t\" style=\"font-size:14px;fill:{}\">{}</tspan><tspan class=\"small\"> — {}</tspan></text>",
                        rn(bx + 14.0),
                        rn(y),
                        s.stroke,
                        esc(label),
                        esc(rest)
                    ));
                }
                None => out.push(format!(
                    "<text x=\"{}\" y=\"{}\" class=\"small\">{}</text>",
                    rn(bx + 14.0),
                    rn(y),
                    esc(l)
                )),
            }
        }
        leaves.push(Leaf::of(
            id,
            format!("{id} links band"),
            Rect {
                x: bx,
                y: by,
                x2: bx + bw,
                y2: by + band.h,
            },
            false,
        ));
    }
    out.push("</g>".to_owned());
}

fn emit_store(n: &Laid, f: &HybridFamily, out: &mut Vec<String>, leaves: &mut Vec<Leaf>) {
    let rx = n.w / 2.0;
    let ry = 20.0;
    let cx = n.x0 + rx;
    let top = n.y + ry;
    let bot = n.y + n.h - ry;
    out.push(format!("<g id=\"{}\">", n.n.id));
    out.push(format!(
        "<path d=\"M{},{} L{},{} A{},{} 0 0 0 {},{} L{},{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\"/>",
        rn(n.x0),
        rn(top),
        rn(n.x0),
        rn(bot),
        rn(rx),
        num(ry),
        rn(n.x0 + n.w),
        rn(bot),
        rn(n.x0 + n.w),
        rn(top),
        f.inner,
        f.stroke
    ));
    out.push(format!(
        "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\"/>",
        rn(cx),
        rn(top),
        rn(rx),
        num(ry),
        f.outer,
        f.stroke
    ));
    let ch = content_height(n.lines.len());
    let ty = top + ry + 4.0 + (bot - top - ry - 4.0 - ch) / 2.0;
    out.push(format!(
        "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" class=\"t\">{}</text>",
        rn(cx),
        rn(ty + 28.0),
        esc(&n.n.name)
    ));
    out.push(text_lines(
        &n.lines,
        cx,
        ty + 52.0,
        "small",
        Anchor::Middle,
        "",
    ));
    out.push("</g>".to_owned());
    leaves.push(Leaf::of(
        &n.n.id,
        format!("store {}", n.n.id),
        rect(n),
        true,
    ));
}

fn emit_tile(n: &Laid, f: &HybridFamily, out: &mut Vec<String>, leaves: &mut Vec<Leaf>) {
    let id = &n.n.id;
    out.push(format!("<g id=\"{id}\">"));
    out.push(format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"12\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.5\"/>",
        rn(n.x0),
        rn(n.y),
        rn(n.w),
        rn(n.h),
        f.outer,
        f.stroke
    ));
    out.push(format!(
        "<text x=\"{}\" y=\"{}\" class=\"t\">{}</text>",
        rn(n.x0 + PAD),
        rn(n.y + 32.0),
        esc(&n.n.name)
    ));
    out.push(text_lines(
        &n.stack,
        n.x0 + PAD,
        n.y + 32.0 + 22.0,
        "small",
        Anchor::Start,
        "",
    ));
    let tw = max_of(
        std::iter::once(text_width(&n.n.name, 17.0, true, false))
            .chain(n.stack.iter().map(|l| text_width(l, 14.0, false, false))),
    );
    leaves.push(Leaf::of(
        id,
        format!("header of {id}"),
        Rect {
            x: n.x0 + PAD,
            y: n.y + 14.0,
            x2: n.x0 + PAD + tw,
            y2: n.y + n.head_h - 4.0,
        },
        false,
    ));
    if !n.foot.is_empty() {
        let fx = n.x0 + 16.0;
        let fy = n.y + n.h - 16.0 - n.foot_h;
        let fw = n.w - 32.0;
        out.push(format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" fill=\"#ffffff\" stroke=\"{HYBRID_RULE}\" stroke-width=\"1\"/>",
            rn(fx),
            rn(fy),
            rn(fw),
            rn(n.foot_h)
        ));
        for (i, l) in n.foot.iter().enumerate() {
            out.push(format!(
                "<text x=\"{}\" y=\"{}\" style=\"font-family:{MONO_FONT};font-size:13px;white-space:pre\">{}</text>",
                rn(fx + 12.0),
                rn(fy + 8.0 + 13.0 + i as f64 * 18.0),
                esc(l)
            ));
        }
        leaves.push(Leaf::of(
            id,
            format!("foot of {id}"),
            Rect {
                x: fx,
                y: fy,
                x2: fx + fw,
                y2: fy + n.foot_h,
            },
            false,
        ));
    }
    out.push("</g>".to_owned());
}

fn emit_box(n: &Laid, _f: &HybridFamily, out: &mut Vec<String>, leaves: &mut Vec<Leaf>) {
    out.push(format!("<g id=\"{}\">", n.n.id));
    out.push(two_line_box(
        n.x0,
        n.y,
        n.w,
        n.h,
        &n.n.name,
        &n.lines,
        family_or_default(n.n.family),
        2.0,
        10.0,
    ));
    out.push("</g>".to_owned());
    leaves.push(Leaf::of(&n.n.id, format!("box {}", n.n.id), rect(n), true));
}

fn emit_band(n: &Laid, f: &HybridFamily, out: &mut Vec<String>, leaves: &mut Vec<Leaf>) {
    let name = &n.n.name;
    out.push(format!("<g id=\"{}\">", n.n.id));
    out.push(format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.5\"/>",
        rn(n.x0),
        rn(n.y),
        rn(n.w),
        rn(n.h),
        f.inner,
        f.stroke
    ));
    let head = format!("{name} — ");
    for (i, l) in n.lines.iter().enumerate() {
        let y = n.y + 11.0 + 15.0 + i as f64 * SUB_LH;
        let x = n.x0 + 20.0;
        match l.strip_prefix(head.as_str()).filter(|_| i == 0) {
            Some(rest) => out.push(format!(
                "<text x=\"{}\" y=\"{}\"><tspan class=\"t\">{}</tspan><tspan class=\"small\"> — {}</tspan></text>",
                rn(x),
                rn(y),
                esc(name),
                esc(rest)
            )),
            None => out.push(format!("<text x=\"{}\" y=\"{}\" class=\"small\">{}</text>", rn(x), rn(y), esc(l))),
        }
    }
    out.push("</g>".to_owned());
    leaves.push(Leaf::of(&n.n.id, format!("band {}", n.n.id), rect(n), true));
}

type Emitter = fn(&Laid, &HybridFamily, &mut Vec<String>, &mut Vec<Leaf>);

fn emitter(kind: NodeType) -> Option<Emitter> {
    match kind {
        NodeType::Component => Some(emit_component),
        NodeType::Store => Some(emit_store),
        NodeType::Tile => Some(emit_tile),
        NodeType::Box => Some(emit_box),
        NodeType::Band => Some(emit_band),
        NodeType::Boundary => None,
    }
}

fn emit_boundary(n: &Laid, out: &mut Vec<String>, leaves: &mut Vec<Leaf>) {
    out.push(format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"14\" fill=\"{HYBRID_BOUNDARY_FILL}\" stroke=\"{HYBRID_MUTED}\" stroke-width=\"2\" stroke-dasharray=\"10 6\"/>",
        rn(n.x0),
        rn(n.y),
        rn(n.w),
        rn(n.h)
    ));
    out.push(format!(
        "<text x=\"{}\" y=\"{}\" class=\"t\" style=\"fill:{HYBRID_MUTED}\">{}</text>",
        rn(n.x0 + 24.0),
        rn(n.y + 32.0),
        esc(&n.n.label)
    ));
    leaves.push(Leaf::free(
        format!("boundary label {}", n.n.id),
        Rect {
            x: n.x0 + 24.0,
            y: n.y + 14.0,
            x2: n.x0 + 24.0 + text_width(&n.n.label, 17.0, true, false),
            y2: n.y + 38.0,
        },
    ));
}

/// The picture's body: boundaries, nodes, edges, then every label on a white
/// backing (so a label always sits above every line), then the legend.
pub fn emit(layout: &Layout, edges: &[Routed], legend: &str, leaves: &mut Vec<Leaf>) -> String {
    let mut out = Vec::new();
    // boundaries first, outermost first
    for n in layout
        .nodes
        .iter()
        .filter(|l| l.n.kind == NodeType::Boundary)
    {
        emit_boundary(n, &mut out, leaves);
    }
    for n in &layout.nodes {
        if let Some(emit) = emitter(n.n.kind) {
            emit(n, family_or_default(n.n.family).colors(), &mut out, leaves);
        }
    }
    let mut lab = Vec::new();
    for e in edges {
        let weak = e.e.kind == EdgeKind::Weak;
        let d = e
            .pts
            .iter()
            .enumerate()
            .map(|(i, [x, y])| format!("{}{},{}", if i == 0 { "M" } else { "L" }, num(*x), num(*y)))
            .collect::<Vec<_>>()
            .join(" ");
        out.push(if weak {
            format!(
                "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2\" stroke-dasharray=\"6 5\"/>",
                HYBRID_SHARED.stroke
            )
        } else {
            format!("<path d=\"{d}\" fill=\"none\" stroke=\"{HYBRID_INK}\" stroke-width=\"2\" marker-end=\"url(#a)\"/>")
        });
        if let Some(l) = &e.lab {
            lab.push(format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>",
                rn(l.x - 4.0),
                rn(l.y - 3.0),
                rn(l.w + 8.0),
                rn(l.h + 6.0)
            ));
            let extra = if weak {
                format!(" style=\"fill:{}\"", HYBRID_SHARED.stroke)
            } else {
                String::new()
            };
            lab.push(text_lines(
                &l.lines,
                l.x,
                l.y + 15.0,
                "lbl",
                Anchor::Start,
                &extra,
            ));
        }
    }
    out.extend(lab);
    out.push(legend.to_owned());
    out.join("\n")
}

/// The legend, and its height (the canvas grows to hold it).
pub fn legend_svg(
    spec: &Spec,
    leaves: &mut Vec<Leaf>,
    lint_err: &mut Vec<String>,
) -> (String, f64) {
    let l = &spec.legend;
    // a spec may supply its own legend rows ([kind, family | null, text]) and note; the default is
    // the substrate legend the single-node and distributed diagrams share
    let rows: Vec<(LegendKind, Option<Family>, String)> = match &l.rows {
        Some(rows) => rows.clone(),
        None => {
            let primary = spec.family == Some(Family::Primary);
            vec![
                (
                    LegendKind::Swatch,
                    spec.family,
                    if primary {
                        "single-node runtime component · the module boxes inside it"
                    } else {
                        "distributed runtime component · the module boxes inside it"
                    }
                    .to_owned(),
                ),
                (
                    LegendKind::Swatch,
                    Some(Family::Shared),
                    "shared core — repeated inside every component that imports it".to_owned(),
                ),
                (
                    LegendKind::Swatch,
                    Some(Family::Entry),
                    "the operator (entry point)".to_owned(),
                ),
                (
                    LegendKind::Swatch,
                    Some(Family::External),
                    "external, or infrastructure h does not build".to_owned(),
                ),
                (
                    LegendKind::Edge,
                    None,
                    "runtime edge, labelled with protocol / verb".to_owned(),
                ),
                (
                    LegendKind::Weak,
                    None,
                    "weaker relation (works in · may call)".to_owned(),
                ),
            ]
        }
    };
    let note = match &l.note {
        Some(None) => Vec::new(),
        Some(Some(text)) => wrap(text, l.w - 32.0, 14.0, false, false),
        None => wrap(
            "Import edges are not drawn: each component's purple \"links\" band is its import closure into the shared core, computed from vizzle on every build.",
            l.w - 32.0,
            14.0,
            false,
            false,
        ),
    };
    // `cols` lays the rows out in a grid, row-major (default one column)
    let cols = l.cols.unwrap_or(1);
    let nrows = rows.len().div_ceil(cols);
    let col_w = (l.w - 16.0) / cols as f64;
    let h = 44.0 + nrows as f64 * 28.0 + note.len() as f64 * SUB_LH + 10.0;
    let mut o = vec![
        format!(
            "<g id=\"legend\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\" fill=\"#ffffff\" stroke=\"{HYBRID_RULE}\" stroke-width=\"1\"/>",
            num(l.x),
            num(l.y),
            num(l.w),
            num(h)
        ),
        format!("<text x=\"{}\" y=\"{}\" class=\"t\">Legend</text>", num(l.x + 16.0), num(l.y + 28.0)),
    ];
    for (i, (kind, fam, text)) in rows.iter().enumerate() {
        let y = l.y + 44.0 + (i / cols) as f64 * 28.0;
        let x = l.x + r((i % cols) as f64 * col_w);
        match kind {
            LegendKind::Swatch => {
                let f = family_or_default(*fam).colors();
                o.push(format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"34\" height=\"20\" rx=\"4\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\"/>",
                    num(x + 16.0),
                    num(y),
                    f.inner,
                    f.stroke
                ));
            }
            LegendKind::Edge => o.push(format!(
                "<path d=\"M{},{} L{},{}\" stroke=\"{HYBRID_INK}\" stroke-width=\"2\" marker-end=\"url(#a)\"/>",
                num(x + 16.0),
                num(y + 10.0),
                num(x + 48.0),
                num(y + 10.0)
            )),
            LegendKind::Weak => o.push(format!(
                "<path d=\"M{},{} L{},{}\" stroke=\"{}\" stroke-width=\"2\" stroke-dasharray=\"6 5\"/>",
                num(x + 16.0),
                num(y + 10.0),
                num(x + 50.0),
                num(y + 10.0),
                HYBRID_SHARED.stroke
            )),
        }
        if text_width(text, 14.0, false, false) > col_w - 62.0 {
            lint_err.push(format!("legend row \"{text}\" is wider than its column"));
        }
        o.push(format!(
            "<text x=\"{}\" y=\"{}\" class=\"lbl\">{}</text>",
            num(x + 62.0),
            num(y + 15.0),
            esc(text)
        ));
    }
    o.push(text_lines(
        &note,
        l.x + 16.0,
        l.y + 44.0 + nrows as f64 * 28.0 + 12.0,
        "small",
        Anchor::Start,
        "",
    ));
    o.push("</g>".to_owned());
    leaves.push(Leaf::free(
        "legend",
        Rect {
            x: l.x,
            y: l.y,
            x2: l.x + l.w,
            y2: l.y + h,
        },
    ));
    (o.join("\n"), h)
}
