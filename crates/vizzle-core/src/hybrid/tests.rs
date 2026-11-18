//! The original `picture.test.mjs` cases (routing, labels, lint, single-node
//! SVG), ported one for one, then the drift and lint acceptance
//! (`docs/plans/impl/hybrid.md` §5.2–§5.3) against whole specs.

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use super::layout::{place, Layout};
use super::lint::{lint, Leaf, Routed};
use super::passes::ModuleGraph;
use super::route::{place_label, route, Point};
use super::spec::{Edge, Node};
use super::svg::emit;
use super::{build, Built};

/// A box at (x, y), 200 wide, as the layout would have placed it.
fn bx(id: &str, x: f64, y: f64, extra: Value) -> Value {
    let mut v = json!({ "id": id, "type": "box", "name": id, "x": [x, x + 200.0], "y": y });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    v
}

fn nodes(values: Vec<Value>) -> Vec<Node> {
    serde_json::from_value(Value::Array(values)).unwrap()
}

fn edge(v: Value) -> Edge {
    serde_json::from_value(v).unwrap()
}

fn laid_out(nodes: &[Node]) -> Layout<'_> {
    let mut lint_err = Vec::new();
    place(nodes, &HashMap::new(), &mut lint_err).unwrap()
}

/// The route from a fixed box `a` at the origin to `b`.
fn route_to(b: Value, e: Value) -> Vec<Point> {
    let to = b["id"].as_str().unwrap().to_owned();
    let ns = nodes(vec![bx("a", 0.0, 0.0, json!({ "minH": 100 })), b]);
    let layout = laid_out(&ns);
    let mut ev = json!({ "from": "a", "to": to });
    ev.as_object_mut()
        .unwrap()
        .extend(e.as_object().unwrap().clone());
    route(&edge(ev), &layout).unwrap()
}

#[test]
fn routes_downwards_from_the_bottom_of_the_source_to_the_top_of_the_target_when_the_target_sits_below(
) {
    assert_eq!(
        route_to(bx("b", 0.0, 300.0, json!({})), json!({})),
        vec![[100.0, 100.0], [100.0, 300.0]]
    );
}

#[test]
fn routes_sideways_when_the_two_boxes_share_rows_leaving_by_the_right_edge() {
    let pts = route_to(bx("b", 400.0, 0.0, json!({ "minH": 100 })), json!({}));
    assert_eq!(
        [pts[0], *pts.last().unwrap()],
        [[200.0, 50.0], [400.0, 50.0]]
    );
}

#[test]
fn elbows_through_the_explicit_lane_when_the_two_ends_are_offset() {
    let pts = route_to(
        bx("b", 400.0, 300.0, json!({})),
        json!({ "tx": 500, "lane": 200 }),
    );
    assert_eq!(
        pts,
        vec![
            [100.0, 100.0],
            [100.0, 200.0],
            [500.0, 200.0],
            [500.0, 300.0]
        ]
    );
}

fn label_for(e: Value, pts: &[Point]) -> Option<super::route::Label> {
    let mut ev = json!({ "from": "a", "to": "b", "label": "go" });
    ev.as_object_mut()
        .unwrap()
        .extend(e.as_object().unwrap().clone());
    let empty: Vec<Node> = Vec::new();
    place_label(&edge(ev), pts, &laid_out(&empty)).unwrap()
}

#[test]
fn puts_the_label_of_a_vertical_segment_to_its_right_and_a_horizontal_one_above() {
    let vertical = label_for(json!({}), &[[100.0, 0.0], [100.0, 200.0]]).unwrap();
    let horizontal = label_for(json!({}), &[[0.0, 100.0], [200.0, 100.0]]).unwrap();
    assert!(vertical.x > 100.0, "right of the vertical line");
    assert!(
        horizontal.y + horizontal.h < 100.0,
        "above the horizontal line"
    );
}

#[test]
fn honours_label_side_over_the_default_side() {
    let left = label_for(
        json!({ "labelSide": "left" }),
        &[[100.0, 0.0], [100.0, 200.0]],
    )
    .unwrap();
    assert!(left.x + left.w < 100.0, "left of the vertical line");
}

#[test]
fn reports_a_label_that_has_no_text_as_no_label() {
    assert!(label_for(json!({ "label": "" }), &[[0.0, 0.0], [10.0, 0.0]]).is_none());
}

/// lint over hand-made nodes and routed edges, returning the messages.
fn lint_of(ns: Vec<Value>, es: Vec<Value>) -> Vec<String> {
    let ns = nodes(ns);
    let es: Vec<Edge> = es.into_iter().map(edge).collect();
    let layout = laid_out(&ns);
    let routed: Vec<Routed> = es
        .iter()
        .map(|e| {
            let pts = route(e, &layout).unwrap();
            let lab = place_label(e, &pts, &layout).unwrap();
            Routed { e, pts, lab }
        })
        .collect();
    let mut leaves: Vec<Leaf> = Vec::new();
    let mut errs = Vec::new();
    emit(&layout, &routed, "", &mut leaves);
    lint(1000.0, &layout, &leaves, &routed, &mut errs);
    errs
}

#[test]
fn lint_reports_two_boxes_that_overlap() {
    let errs = lint_of(
        vec![
            bx("a", 0.0, 0.0, json!({ "minH": 100 })),
            bx("b", 100.0, 0.0, json!({ "minH": 100 })),
        ],
        vec![],
    );
    assert!(
        errs.contains(&"overlap: node a × node b".to_owned()),
        "{errs:?}"
    );
}

#[test]
fn lint_reports_an_edge_that_runs_through_a_component_it_neither_starts_nor_ends_at() {
    let errs = lint_of(
        vec![
            bx("a", 0.0, 0.0, json!({ "minH": 100 })),
            json!({ "id": "mid", "type": "component", "name": "mid", "x": [0, 200], "y": 200, "modules": [] }),
            bx("c", 0.0, 500.0, json!({ "minH": 100 })),
        ],
        vec![json!({ "from": "a", "to": "c" })],
    );
    assert!(
        errs.contains(&"edge a→c runs through component mid".to_owned()),
        "{errs:?}"
    );
}

#[test]
fn lint_reports_a_label_that_straddles_the_border_of_a_component() {
    let label = "x".repeat(40);
    let errs = lint_of(
        vec![
            bx("a", 0.0, 0.0, json!({ "minH": 100 })),
            json!({ "id": "frame", "type": "component", "name": "frame", "x": [300, 700], "y": 0, "modules": [] }),
            bx("c", 0.0, 300.0, json!({ "minH": 100 })),
        ],
        vec![
            json!({ "from": "a", "to": "c", "label": label, "labelX": 280, "labelY": 30, "labelSide": "right" }),
        ],
    );
    assert!(
        errs.contains(&format!("label \"{label}\" straddles the border of frame")),
        "{errs:?}"
    );
}

#[test]
fn a_clean_two_box_picture_lints_clean() {
    let errs = lint_of(
        vec![
            bx("a", 0.0, 0.0, json!({ "minH": 100 })),
            bx("b", 0.0, 300.0, json!({ "minH": 100 })),
        ],
        vec![json!({ "from": "a", "to": "b" })],
    );
    assert_eq!(errs, Vec::<String>::new());
}

/// emit hand-made nodes and return their SVG.
fn svg_of(ns: Vec<Value>) -> String {
    let ns = nodes(ns);
    let layout = laid_out(&ns);
    emit(&layout, &[], "", &mut Vec::new())
}

#[test]
fn emits_a_box_as_a_group_holding_a_rounded_rect_and_its_title() {
    let svg = svg_of(vec![bx("b", 10.0, 20.0, json!({ "family": "ext" }))]);
    assert!(
        svg.starts_with("<g id=\"b\">\n<rect x=\"10\" y=\"20\" width=\"200\" height=\"46\" rx=\"10\" fill=\"#eaeef2\" stroke=\"#57606a\""),
        "{svg}"
    );
    assert!(
        svg.trim_end().ends_with("class=\"t\">b</text>\n\n</g>"),
        "{svg}"
    );
}

#[test]
fn emits_a_store_as_a_cylinder_a_body_path_and_a_top_ellipse() {
    let svg = svg_of(vec![
        json!({ "id": "s", "type": "store", "name": "s", "x": [0, 200], "y": 0 }),
    ]);
    assert!(
        svg.starts_with("<g id=\"s\">\n<path d=\"M0,20 L0,78 A100,20 0 0 0 200,78 L200,20\""),
        "{svg}"
    );
    assert!(
        svg.contains("<ellipse cx=\"100\" cy=\"20\" rx=\"100\" ry=\"20\""),
        "{svg}"
    );
}

#[test]
fn emits_a_band_with_its_name_in_bold_and_the_rest_of_its_text_small() {
    let svg = svg_of(vec![
        json!({ "id": "bd", "type": "band", "name": "Name", "text": "details", "x": [0, 400], "y": 0 }),
    ]);
    assert!(
        svg.contains("<tspan class=\"t\">Name</tspan><tspan class=\"small\"> — details</tspan>"),
        "{svg}"
    );
}

#[test]
fn emits_a_tile_with_its_header_and_a_component_with_its_module_boxes() {
    let tile = svg_of(vec![
        json!({ "id": "t", "type": "tile", "name": "Tile", "x": [0, 400], "y": 0, "height": 200 }),
    ]);
    assert!(
        tile.starts_with(
            "<g id=\"t\">\n<rect x=\"0\" y=\"0\" width=\"400\" height=\"200\" rx=\"12\""
        ),
        "{tile}"
    );
    let comp = svg_of(vec![
        json!({ "id": "c", "type": "component", "name": "Comp", "x": [0, 400], "y": 0, "modules": [{ "name": "mod" }] }),
    ]);
    assert!(
        comp.contains("<g id=\"c\">") && comp.contains("class=\"t\">mod</text>"),
        "{comp}"
    );
}

#[test]
fn emits_a_boundary_dashed_behind_the_nodes_it_contains() {
    let svg = svg_of(vec![
        bx("a", 100.0, 100.0, json!({ "minH": 100 })),
        json!({ "id": "bd", "type": "boundary", "label": "Zone", "contains": ["a"] }),
    ]);
    assert!(svg.starts_with("<rect x=\"70\""), "{}", &svg[..40]);
    let dash = svg.find("stroke-dasharray=\"10 6\"").unwrap();
    let zone = svg.find("Zone").unwrap();
    let group = svg.find("<g id=\"a\">").unwrap();
    assert!(dash < zone && zone < group, "{svg}");
}

// ─── whole specs: drift and lint name the box and the rule ───────────────────

/// A runtime spec: one component running `p:pkg/a`, over a pass that reports `modules`.
fn runtime_spec(refs: &[&str]) -> Value {
    json!({
        "title": "T", "subtitle": "s", "footnote": "f", "family": "primary",
        "canvas": { "width": 800 }, "vizzle": ["p"],
        "nodes": [{ "id": "proc", "type": "component", "name": "Proc", "x": [40, 440], "y": 140,
                    "modules": [{ "name": "a", "refs": refs }] }],
        "edges": [],
        "legend": { "x": 480, "y": 140, "w": 300, "rows": [["swatch", "primary", "process"]], "note": null }
    })
}

fn graphs(modules: &[&str]) -> BTreeMap<String, ModuleGraph> {
    BTreeMap::from([(
        "p".to_owned(),
        ModuleGraph {
            modules: modules.iter().map(|m| (*m).to_owned()).collect(),
            edges: Vec::new(),
        },
    )])
}

fn build_of(
    spec: &Value,
    g: &BTreeMap<String, ModuleGraph>,
    exists: &dyn Fn(&str) -> bool,
) -> Built {
    build(&spec.to_string(), g, exists, "spec.json").unwrap()
}

#[test]
fn a_matching_spec_builds_an_svg_naming_vizzle_and_the_spec() {
    let built = build_of(&runtime_spec(&["p:pkg/a"]), &graphs(&["pkg/a"]), &|_| true);
    assert_eq!(built.findings, Vec::<String>::new());
    let svg = built.svg.unwrap();
    assert!(
        svg.lines()
            .nth(1)
            .unwrap()
            .starts_with("<!-- GENERATED by vizzle hybrid "),
        "{svg}"
    );
    assert!(svg.contains("from spec.json (passes: p)"), "{svg}");
}

#[test]
fn drift_a_renamed_module_names_the_component_and_the_ref() {
    let built = build_of(
        &runtime_spec(&["p:pkg/a"]),
        &graphs(&["pkg/renamed"]),
        &|_| true,
    );
    assert!(built.svg.is_none());
    assert!(
        built.findings.contains(
            &"drift: proc › \"a\" names p:pkg/a, which vizzle no longer reports".to_owned()
        ),
        "{:?}",
        built.findings
    );
}

#[test]
fn drift_a_new_module_in_scope_is_named_as_unplaced() {
    let built = build_of(
        &runtime_spec(&["p:pkg/a"]),
        &graphs(&["pkg/a", "pkg/new"]),
        &|_| true,
    );
    assert_eq!(
        built.findings,
        vec!["drift: vizzle reports p:pkg/new in scope, but the spec places it nowhere (place it in a component, or omit it with a reason)"]
    );
}

#[test]
fn drift_an_omission_whose_module_has_gone_fails() {
    let mut spec = runtime_spec(&["p:pkg/a"]);
    spec["omit"] = json!({ "p:pkg/old": "retired" });
    let built = build_of(&spec, &graphs(&["pkg/a"]), &|_| true);
    assert_eq!(
        built.findings,
        vec!["drift: spec.omit names p:pkg/old (retired), which vizzle no longer reports — drop the omission"]
    );
}

#[test]
fn drift_a_cited_path_that_is_gone_names_the_node() {
    let spec = json!({
        "title": "T", "subtitle": "s", "footnote": "f",
        "canvas": { "width": 800 }, "vizzle": [],
        "nodes": [{ "id": "thing", "type": "box", "name": "Thing", "x": [40, 240], "y": 140, "path": "src/thing.rs" }],
        "legend": { "x": 480, "y": 140, "w": 300, "rows": [["edge", null, "flow"]], "note": null }
    });
    let gone = build_of(&spec, &BTreeMap::new(), &|p| p != "src/thing.rs");
    assert_eq!(
        gone.findings,
        vec!["drift: thing cites src/thing.rs, which is gone from the tree"]
    );
    assert!(build_of(&spec, &BTreeMap::new(), &|_| true).svg.is_some());
}

#[test]
fn drift_a_claimed_link_with_no_import_path_fails() {
    let mut spec = runtime_spec(&["p:pkg/a"]);
    spec["shared"] = json!(["p:pkg/core"]);
    spec["nodes"][0]["links"] = json!(["p:pkg/core"]);
    let built = build_of(&spec, &graphs(&["pkg/a", "pkg/core"]), &|_| true);
    assert_eq!(
        built.findings,
        vec!["drift: proc claims to link p:pkg/core, but vizzle reports no import path from its modules to it"]
    );
    let mut wired = graphs(&["pkg/a", "pkg/core"]);
    wired
        .get_mut("p")
        .unwrap()
        .edges
        .push(("pkg/a".to_owned(), "pkg/core".to_owned()));
    let built = build_of(&spec, &wired, &|_| true);
    assert_eq!(built.findings, Vec::<String>::new());
    assert!(built
        .svg
        .unwrap()
        .contains("links</tspan><tspan class=\"small\"> — core</tspan>"));
}

#[test]
fn lint_two_overlapping_boxes_name_the_rule_and_both_ids() {
    let spec = json!({
        "title": "T", "subtitle": "s", "footnote": "f",
        "canvas": { "width": 800 }, "vizzle": [],
        "nodes": [{ "id": "left", "type": "box", "name": "L", "x": [40, 240], "y": 140 },
                  { "id": "right", "type": "box", "name": "R", "x": [200, 400], "y": 150 }],
        "legend": { "x": 480, "y": 140, "w": 300, "rows": [["edge", null, "flow"]], "note": null }
    });
    let built = build_of(&spec, &BTreeMap::new(), &|_| true);
    assert!(built.svg.is_none());
    assert!(
        built
            .findings
            .contains(&"layout: overlap: node left × node right".to_owned()),
        "{:?}",
        built.findings
    );
}

#[test]
fn lint_text_wider_than_its_box_names_the_box() {
    let spec = json!({
        "title": "T", "subtitle": "s", "footnote": "f",
        "canvas": { "width": 800 }, "vizzle": [],
        "nodes": [{ "id": "narrow", "type": "box", "name": "A name far too long for this box", "x": [40, 140], "y": 140 }],
        "legend": { "x": 480, "y": 140, "w": 300, "rows": [["edge", null, "flow"]], "note": null }
    });
    let built = build_of(&spec, &BTreeMap::new(), &|_| true);
    assert_eq!(
        built.findings,
        vec!["layout: narrow: \"A name far too long for this box\" is wider than its box"]
    );
}

#[test]
fn an_unknown_field_is_refused_not_ignored() {
    let mut spec = runtime_spec(&["p:pkg/a"]);
    let nodes = spec.as_object_mut().unwrap().remove("nodes").unwrap();
    spec["nodez"] = nodes;
    let err = build(
        &spec.to_string(),
        &graphs(&["pkg/a"]),
        &|_| true,
        "spec.json",
    )
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("unknown field `nodez`"),
        "{err:#}"
    );
}

#[test]
fn an_undefined_pass_and_an_unknown_node_id_are_refused() {
    let err = build(
        &runtime_spec(&["p:pkg/a"]).to_string(),
        &BTreeMap::new(),
        &|_| true,
        "s",
    )
    .unwrap_err();
    assert!(err.to_string().contains("pass \"p\""), "{err}");
    let mut spec = runtime_spec(&["p:pkg/a"]);
    spec["edges"] = json!([{ "from": "proc", "to": "ghost" }]);
    let err = build(&spec.to_string(), &graphs(&["pkg/a"]), &|_| true, "s").unwrap_err();
    assert!(err.to_string().contains("\"ghost\""), "{err}");
}

#[test]
fn h_family_names_are_accepted_as_aliases_and_draw_the_same() {
    let generic = build_of(&runtime_spec(&["p:pkg/a"]), &graphs(&["pkg/a"]), &|_| true);
    let mut spec = runtime_spec(&["p:pkg/a"]);
    spec["family"] = json!("sn");
    spec["legend"]["rows"] = json!([["swatch", "sn", "process"]]);
    let aliased = build_of(&spec, &graphs(&["pkg/a"]), &|_| true);
    assert_eq!(generic.svg, aliased.svg);
}

// ─── a real pass: the component graph JSON the CLI hands over ────────────────

/// Two workspace packages, `apps/svc` importing `@x/core`, read the way the
/// CLI reads a pass: built in-process, exported with `component::to_json`.
fn real_pass() -> BTreeMap<String, ModuleGraph> {
    let pairs = |items: &[(&str, &str)]| -> Vec<(String, String)> {
        items
            .iter()
            .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
            .collect()
    };
    let manifests = pairs(&[
        ("pkgs/core/package.json", r#"{"name": "@x/core"}"#),
        ("apps/svc/package.json", r#"{"name": "svc"}"#),
    ]);
    let files = pairs(&[
        ("pkgs/core/src/index.ts", "export class Core {}\n"),
        (
            "apps/svc/src/main.ts",
            "import { Core } from \"@x/core\";\nclass Svc {}\n",
        ),
    ]);
    let graph = crate::component::build(&files, &manifests, &[]).unwrap();
    let json = crate::component::to_json(&graph, false);
    BTreeMap::from([("p".to_owned(), super::module_graph(&json).unwrap())])
}

#[test]
fn a_real_pass_reports_module_ids_as_spec_refs_name_them() {
    let g = real_pass();
    assert_eq!(g["p"].modules, vec!["apps/svc", "pkgs/core"]);
    assert_eq!(
        g["p"].edges,
        vec![("apps/svc".to_owned(), "pkgs/core".to_owned())]
    );
}

#[test]
fn a_real_pass_reconciles_and_its_imports_fill_the_links_band() {
    let mut spec = runtime_spec(&["p:apps/svc"]);
    spec["shared"] = json!(["p:pkgs/core"]);
    spec["nodes"][0]["links"] = json!("auto");
    let built = build_of(&spec, &real_pass(), &|_| true);
    assert_eq!(built.findings, Vec::<String>::new());
    assert!(built
        .svg
        .unwrap()
        .contains("links</tspan><tspan class=\"small\"> — core</tspan>"));
    // the same spec without the shared core placed: the pass's other package is unplaced
    let built = build_of(&runtime_spec(&["p:apps/svc"]), &real_pass(), &|_| true);
    assert_eq!(built.findings.len(), 1);
    assert!(
        built.findings[0].contains("p:pkgs/core in scope"),
        "{:?}",
        built.findings
    );
}
