//! Hybrid architecture pictures (`docs/diagram-types/hybrid.md`): a person
//! writes the runtime layer and a coarse layout as JSON, vizzle supplies the
//! module layer, and the build refuses any disagreement and lints its own
//! picture.
//!
//! A port, module for module, of the generator h drew its pictures with
//! (`docs/plans/impl/hybrid.md`): `spec` (new: the typed format), `passes`
//! (was `vizzle.mjs`), `reconcile`, `layout`, `route`, `lint`, `svg`,
//! `metrics`; [`build`] is the old `build()`. The function names match so the
//! two read side by side.
//!
//! Two kinds of trouble, kept apart as `overlay` keeps them: a spec that
//! cannot be read (bad JSON, an unknown field, an id that names nothing) is
//! an error — the command could not run; a spec that reads but disagrees with
//! the code, or whose picture fails its lint, is a list of findings.

pub mod layout;
pub mod lint;
pub mod metrics;
pub mod passes;
pub mod reconcile;
pub mod route;
pub mod spec;
pub mod svg;

use std::collections::BTreeMap;

use anyhow::Result;

use self::lint::{Leaf, Routed};
use self::metrics::{esc, max_of, num, r, rn, text_width, wrap};
use self::passes::ModuleGraph;
use self::svg::{emit, legend_svg};
use crate::palette::HYBRID_MUTED;

pub use self::passes::{module_graph, pass_table, spec_passes, PassTable};

/// What a build produced: the SVG, or the findings that stopped it.
#[derive(Debug, Clone, Default)]
pub struct Built {
    /// `None` whenever `findings` is not empty: a picture that drifts or
    /// fails its lint is never written.
    pub svg: Option<String>,
    /// `drift: …` and `layout: …` lines, in the order found, each once.
    pub findings: Vec<String>,
    pub width: f64,
    pub height: f64,
}

/// Build one picture. `graphs` holds the module graph of every pass the spec
/// names; `exists` answers whether a repo-relative path is in the tree;
/// `spec_path` is how the GENERATED comment names the spec.
pub fn build(
    spec_json: &str,
    graphs: &BTreeMap<String, ModuleGraph>,
    exists: &dyn Fn(&str) -> bool,
    spec_path: &str,
) -> Result<Built> {
    let spec = spec::parse(spec_json)?;
    if let Some(missing) = spec.vizzle.iter().find(|p| !graphs.contains_key(*p)) {
        anyhow::bail!("{}", passes::undefined_pass(missing));
    }
    let rec = reconcile::reconcile(&spec, graphs, exists);
    if !rec.errors.is_empty() {
        return Ok(Built {
            findings: rec.errors.iter().map(|e| format!("drift: {e}")).collect(),
            ..Built::default()
        });
    }
    let mut lint_err = Vec::new();
    let layout = layout::place(&spec.nodes, &rec.placement, &mut lint_err)?;
    let mut leaves: Vec<Leaf> = Vec::new();
    let (legend, legend_h) = legend_svg(&spec, &mut leaves, &mut lint_err);
    let edges = spec
        .edges
        .iter()
        .map(|e| {
            let pts = route::route(e, &layout)?;
            let lab = route::place_label(e, &pts, &layout)?;
            Ok(Routed { e, pts, lab })
        })
        .collect::<Result<Vec<_>>>()?;
    // title block
    leaves.push(Leaf::free(
        "title",
        layout::Rect {
            x: 40.0,
            y: 22.0,
            x2: 40.0 + text_width(&spec.title, 30.0, true, false),
            y2: 60.0,
        },
    ));
    let sub_lines = wrap(
        &spec.subtitle,
        spec.subtitle_w.unwrap_or(1000.0),
        17.0,
        false,
        false,
    );
    leaves.push(Leaf::free(
        "subtitle",
        layout::Rect {
            x: 40.0,
            y: 70.0,
            x2: 40.0 + max_of(sub_lines.iter().map(|l| text_width(l, 17.0, false, false))),
            y2: 70.0 + sub_lines.len() as f64 * 24.0,
        },
    ));
    let body = emit(&layout, &edges, &legend, &mut leaves);
    // the legend counts too, so a legend placed low can never run into the footnote or off the canvas
    let bottom = max_of(
        layout
            .nodes
            .iter()
            .map(|n| n.y + n.h)
            .chain(std::iter::once(spec.legend.y + legend_h)),
    );
    let wc = spec.canvas.width;
    let foot = wrap(&spec.footnote, wc - 200.0, 15.0, false, false);
    let h = r(bottom + 40.0 + foot.len() as f64 * 22.0 + 24.0);
    lint::lint(wc, &layout, &leaves, &edges, &mut lint_err);
    if !lint_err.is_empty() {
        let mut seen = std::collections::HashSet::new();
        let findings = lint_err
            .into_iter()
            .filter(|e| seen.insert(e.clone()))
            .map(|e| format!("layout: {e}"))
            .collect();
        return Ok(Built {
            findings,
            ..Built::default()
        });
    }
    let passes = if spec.vizzle.is_empty() {
        "none".to_owned()
    } else {
        spec.vizzle.join(", ")
    };
    let svg = [
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
            num(wc),
            num(h),
            num(wc),
            num(h)
        ),
        generated_comment(spec_path, &passes),
        "<style>text{font-family:-apple-system,\"Segoe UI\",Helvetica,Arial,sans-serif;font-size:17px;fill:#24292f} .t{font-weight:600} .small{font-size:14px;fill:#57606a} .lbl{font-size:14px;fill:#24292f}</style>".to_owned(),
        "<defs><marker id=\"a\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"8\" markerHeight=\"8\" orient=\"auto-start-reverse\"><path d=\"M0,0 L10,5 L0,10 z\" fill=\"#24292f\"/></marker></defs>".to_owned(),
        format!("<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>", num(wc), num(h)),
        format!("<text x=\"40\" y=\"52\" class=\"t\" style=\"font-size:30px\">{}</text>", esc(&spec.title)),
        sub_lines
            .iter()
            .enumerate()
            .map(|(i, l)| format!("<text x=\"40\" y=\"{}\" style=\"fill:{HYBRID_MUTED}\">{}</text>", num(88.0 + i as f64 * 24.0), esc(l)))
            .collect::<Vec<_>>()
            .join("\n"),
        body,
        foot.iter()
            .enumerate()
            .map(|(i, l)| {
                format!(
                    "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" class=\"small\" style=\"font-size:15px\">{}</text>",
                    num(wc / 2.0),
                    rn(bottom + 44.0 + i as f64 * 22.0),
                    esc(l)
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "</svg>".to_owned(),
        String::new(),
    ]
    .join("\n");
    Ok(Built {
        svg: Some(svg),
        findings: Vec::new(),
        width: wc,
        height: h,
    })
}

/// The SVG's second line: what made it and how to remake it (§9 D9). The
/// version makes a stale build visible in a diff.
pub fn generated_comment(spec_path: &str, passes: &str) -> String {
    format!(
        "<!-- GENERATED by vizzle hybrid {} from {spec_path} (passes: {passes}). Do not edit by hand: change the spec and regenerate. -->",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests;
