//! Edge routing and label placement.

use anyhow::Result;

use super::layout::{Layout, Rect, SUB_LH};
use super::metrics::{max_of, r, text_width, wrap};
use super::spec::{Edge, LabelSide, Side};

pub type Point = [f64; 2];

/// An edge's label: its wrapped lines at a rounded origin, and the backing
/// box lint checks (left unrounded, as it always was).
#[derive(Debug, Clone)]
pub struct Label {
    pub lines: Vec<String>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub bx: Rect,
}

/// An orthogonal route from `e.from` to `e.to`, every point rounded.
pub fn route(e: &Edge, layout: &Layout) -> Result<Vec<Point>> {
    let a = layout.get(&e.from)?.rect();
    let b = layout.get(&e.to)?.rect();
    let (fs, ts) = match (e.from_side, e.to_side) {
        (Some(f), Some(t)) => (f, t),
        _ => {
            if b.y >= a.y2 {
                (Side::Bottom, Side::Top)
            } else if b.y2 <= a.y {
                (Side::Top, Side::Bottom)
            } else if b.x >= a.x2 {
                (Side::Right, Side::Left)
            } else {
                (Side::Left, Side::Right)
            }
        }
    };
    let ox = [a.x.max(b.x), a.x2.min(b.x2)];
    let oy = [a.y.max(b.y), a.y2.min(b.y2)];
    let mid_x = (ox[0] < ox[1]).then(|| (ox[0] + ox[1]) / 2.0);
    let mid_y = (oy[0] < oy[1]).then(|| (oy[0] + oy[1]) / 2.0);
    let fx = layout
        .val(e.fx.as_ref().or(e.x.as_ref()))?
        .or(mid_x)
        .unwrap_or((a.x + a.x2) / 2.0);
    let tx = layout
        .val(e.tx.as_ref().or(e.x.as_ref()))?
        .or(mid_x)
        .unwrap_or((b.x + b.x2) / 2.0);
    let fy = layout
        .val(e.fy.as_ref().or(e.y.as_ref()))?
        .or(mid_y)
        .unwrap_or((a.y + a.y2) / 2.0);
    let ty = layout
        .val(e.ty.as_ref().or(e.y.as_ref()))?
        .or(mid_y)
        .unwrap_or((b.y + b.y2) / 2.0);
    let at = |rc: Rect, side: Side, x: f64, y: f64| -> Point {
        match side {
            Side::Top => [x, rc.y],
            Side::Bottom => [x, rc.y2],
            Side::Left => [rc.x, y],
            Side::Right => [rc.x2, y],
        }
    };
    let s = at(a, fs, fx, fy);
    let t = at(b, ts, tx, ty);
    let pts: Vec<Point> = if let Some(via) = &e.via {
        let mut pts = vec![s];
        for [x, y] in via {
            pts.push([
                layout.val(Some(x))?.unwrap_or(f64::NAN),
                layout.val(Some(y))?.unwrap_or(f64::NAN),
            ]);
        }
        pts.push(t);
        pts
    } else if fs.vertical() && ts.vertical() {
        let lane = layout.val(e.lane.as_ref())?.unwrap_or((s[1] + t[1]) / 2.0);
        if s[0] == t[0] {
            vec![s, t]
        } else {
            vec![s, [s[0], lane], [t[0], lane], t]
        }
    } else if !fs.vertical() && !ts.vertical() {
        let lane = layout.val(e.lane.as_ref())?.unwrap_or((s[0] + t[0]) / 2.0);
        if s[1] == t[1] {
            vec![s, t]
        } else {
            vec![s, [lane, s[1]], [lane, t[1]], t]
        }
    } else if fs.vertical() {
        vec![s, [s[0], t[1]], t]
    } else {
        vec![s, [t[0], s[1]], t]
    };
    Ok(pts.into_iter().map(|[x, y]| [r(x), r(y)]).collect())
}

/// Where an edge's label sits: on its longest segment unless told otherwise,
/// right of a vertical one, above a horizontal one. `None` without a label.
pub fn place_label(e: &Edge, pts: &[Point], layout: &Layout) -> Result<Option<Label>> {
    if e.label.is_empty() {
        return Ok(None);
    }
    let segs: Vec<(Point, Point)> = pts.windows(2).map(|w| (w[0], w[1])).collect();
    let len = |(a, b): (Point, Point)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs();
    let si = match e.label_seg {
        // negative counts from the end; out of range is refused below
        Some(i) => {
            usize::try_from(if i < 0 { i + segs.len() as i64 } else { i }).unwrap_or(usize::MAX)
        }
        None => {
            let mut si = 0;
            for (i, sg) in segs.iter().enumerate() {
                if len(*sg) > len(segs[si]) {
                    si = i;
                }
            }
            si
        }
    };
    let Some(&(a, b)) = segs.get(si) else {
        anyhow::bail!(
            "edge {}→{}: labelSeg {si} is not one of its {} segments",
            e.from,
            e.to,
            segs.len()
        );
    };
    let lines = wrap(&e.label, e.label_w.unwrap_or(300.0), 14.0, false, false);
    let lw = max_of(lines.iter().map(|l| text_width(l, 14.0, false, false)));
    let lh = lines.len() as f64 * SUB_LH;
    let tt = e.label_t.unwrap_or(0.5);
    let px = match &e.label_x {
        Some(x) => layout.val(Some(x))?.unwrap_or(f64::NAN),
        None => a[0] + (b[0] - a[0]) * tt,
    };
    let py = match &e.label_y {
        Some(y) => layout.val(Some(y))?.unwrap_or(f64::NAN),
        None => a[1] + (b[1] - a[1]) * tt,
    };
    let vertical = a[0] == b[0];
    let side = e.label_side.unwrap_or(if vertical {
        LabelSide::Right
    } else {
        LabelSide::Above
    });
    let (mut x, mut y) = match side {
        LabelSide::Right => (px + 12.0, py - lh / 2.0),
        LabelSide::Left => (px - 12.0 - lw, py - lh / 2.0),
        LabelSide::Above => (px - lw / 2.0, py - 8.0 - lh),
        LabelSide::Below => (px - lw / 2.0, py + 8.0),
    };
    x += e.label_dx.unwrap_or(0.0);
    y += e.label_dy.unwrap_or(0.0);
    Ok(Some(Label {
        lines,
        x: r(x),
        y: r(y),
        w: r(lw),
        h: lh,
        bx: Rect {
            x: x - 4.0,
            y: y - 3.0,
            x2: x + lw + 4.0,
            y2: y + lh + 3.0,
        },
    }))
}
