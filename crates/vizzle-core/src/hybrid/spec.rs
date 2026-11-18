//! The hybrid spec, typed: the hand-written JSON a picture is built from
//! (`docs/diagram-types/hybrid.md`, "The spec"). The format is h's, kept
//! exactly (`docs/plans/impl/hybrid.md` §9 D3), with one addition: a field the
//! format does not know is refused, so a misspelling fails the build instead
//! of silently drawing a default.

use std::fmt;

use anyhow::{bail, Context, Result};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::palette::{self, HybridFamily};

/// One of the five colour families. The generic names are canonical; h's own
/// names stay accepted as aliases because they never reach the SVG (§9 D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Family {
    #[serde(rename = "primary", alias = "sn")]
    Primary,
    #[serde(rename = "secondary", alias = "dist")]
    Secondary,
    #[serde(rename = "shared")]
    Shared,
    #[serde(rename = "external", alias = "ext")]
    External,
    #[serde(rename = "entry")]
    Entry,
}

impl Family {
    pub fn colors(self) -> &'static HybridFamily {
        match self {
            Family::Primary => &palette::HYBRID_PRIMARY,
            Family::Secondary => &palette::HYBRID_SECONDARY,
            Family::Shared => &palette::HYBRID_SHARED,
            Family::External => &palette::HYBRID_EXTERNAL,
            Family::Entry => &palette::HYBRID_ENTRY,
        }
    }
}

/// A node's family, or the default every emitter falls back to. Note it is
/// `primary`, not the spec's own `family`: the generator always did this.
pub fn family_or_default(f: Option<Family>) -> Family {
    f.unwrap_or(Family::Primary)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Spec {
    /// Free text: the facts the picture rests on and when they were checked.
    #[serde(rename = "$comment", default)]
    pub comment: Option<Value>,
    pub title: String,
    pub subtitle: String,
    #[serde(default)]
    pub subtitle_w: Option<f64>,
    pub footnote: String,
    #[serde(default)]
    pub family: Option<Family>,
    pub canvas: Canvas,
    /// The passes this picture reconciles against; empty for a concept.
    pub vizzle: Vec<String>,
    #[serde(default)]
    pub shared: Vec<String>,
    /// `ref → reason`, in the order written: findings list them that way.
    #[serde(default, deserialize_with = "ordered_map")]
    pub omit: Vec<(String, String)>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    pub legend: Legend,
}

impl Spec {
    pub fn omits(&self, reference: &str) -> bool {
        self.omit.iter().any(|(r, _)| r == reference)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canvas {
    pub width: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Component,
    Store,
    Box,
    Band,
    Boundary,
    Tile,
}

/// One node. Flat, as the format is: which fields a type reads is the
/// layout's business (`layout::measure`), not the parser's.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Node {
    #[serde(rename = "$comment", default)]
    pub comment: Option<Value>,
    pub id: String,
    #[serde(rename = "type")]
    pub kind: NodeType,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub x: Option<[f64; 2]>,
    #[serde(default)]
    pub y: Option<YSpec>,
    #[serde(default)]
    pub family: Option<Family>,
    #[serde(default)]
    pub sub: Option<String>,
    /// A component's stack line.
    #[serde(default)]
    pub stack: Option<String>,
    /// A band's text after its name.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub cols: Option<usize>,
    #[serde(default)]
    pub modules: Vec<Module>,
    #[serde(default)]
    pub links: Option<Links>,
    #[serde(default)]
    pub links_label: Option<String>,
    /// Siblings sharing a row are made equal in height.
    #[serde(default)]
    pub row: Option<String>,
    /// The tile this node sits in: its `x` and numeric `y` are offsets from it.
    #[serde(rename = "in", default)]
    pub inside: Option<String>,
    /// A boundary's members, its padding (top, right, bottom, left) and label.
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default)]
    pub pad: Option<[f64; 4]>,
    #[serde(default)]
    pub label: String,
    /// A tile's fixed height, and its monospace foot.
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub foot: Option<String>,
    /// A box's minimum height.
    #[serde(default)]
    pub min_h: Option<f64>,
    /// Source paths this node stands for; each must exist in the tree.
    #[serde(default)]
    pub path: Option<OneOrMany>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    #[serde(rename = "$comment", default)]
    pub comment: Option<Value>,
    pub name: String,
    /// `<pass>:<module id>` for each vizzle module this box stands for.
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub path: Option<OneOrMany>,
    #[serde(default)]
    pub family: Option<Family>,
    #[serde(default)]
    pub span: Option<usize>,
    #[serde(default)]
    pub sub: Option<String>,
}

/// A component's links band: computed from the imports, or claimed.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Links {
    Word(String),
    List(Vec<String>),
}

/// A string or a list of strings, as `path` and `below` take.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn items(&self) -> Vec<&str> {
        match self {
            OneOrMany::One(s) => vec![s.as_str()],
            OneOrMany::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

/// A node's vertical placement: a number, or relative to other nodes.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum YSpec {
    At(f64),
    Relative(RelativeY),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelativeY {
    #[serde(default)]
    pub below: Option<OneOrMany>,
    #[serde(default)]
    pub gap: Option<f64>,
    #[serde(default)]
    pub top: Option<String>,
    #[serde(default)]
    pub dy: Option<f64>,
    #[serde(default)]
    pub at: Option<Expr>,
}

/// A coordinate: a number, or an expression over placed nodes
/// (`runner.bottom-30`, `machine.bottom~llm.top`), resolved by `layout::Layout::val`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Expr {
    Num(f64),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    pub fn vertical(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabelSide {
    Right,
    Left,
    Above,
    Below,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    #[default]
    Runtime,
    Weak,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Edge {
    #[serde(rename = "$comment", default)]
    pub comment: Option<Value>,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub kind: EdgeKind,
    /// Module refs the edge claims its source imports; each must be reachable.
    #[serde(default)]
    pub imports: Option<Vec<String>>,
    #[serde(default)]
    pub from_side: Option<Side>,
    #[serde(default)]
    pub to_side: Option<Side>,
    #[serde(default)]
    pub x: Option<Expr>,
    #[serde(default)]
    pub y: Option<Expr>,
    #[serde(default)]
    pub fx: Option<Expr>,
    #[serde(default)]
    pub tx: Option<Expr>,
    #[serde(default)]
    pub fy: Option<Expr>,
    #[serde(default)]
    pub ty: Option<Expr>,
    #[serde(default)]
    pub lane: Option<Expr>,
    #[serde(default)]
    pub via: Option<Vec<[Expr; 2]>>,
    /// Which segment carries the label; negative counts from the end.
    #[serde(default)]
    pub label_seg: Option<i64>,
    #[serde(default)]
    pub label_w: Option<f64>,
    #[serde(default)]
    pub label_t: Option<f64>,
    #[serde(default)]
    pub label_x: Option<Expr>,
    #[serde(default)]
    pub label_y: Option<Expr>,
    #[serde(default)]
    pub label_side: Option<LabelSide>,
    #[serde(default)]
    pub label_dx: Option<f64>,
    #[serde(default)]
    pub label_dy: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LegendKind {
    Swatch,
    Edge,
    Weak,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Legend {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    /// `[kind, family | null, text]`; absent = the substrate legend.
    #[serde(default)]
    pub rows: Option<Vec<(LegendKind, Option<Family>, String)>>,
    #[serde(default)]
    pub cols: Option<usize>,
    /// Absent = the default note; `null` = no note.
    #[serde(default, deserialize_with = "present")]
    pub note: Option<Option<String>>,
}

/// Distinguishes a field set to `null` (`Some(None)`) from one left out (`None`).
fn present<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

/// A JSON object read as `(key, value)` pairs in the order written.
fn ordered_map<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<(String, String)>, D::Error> {
    struct Ordered;
    impl<'de> Visitor<'de> for Ordered {
        type Value = Vec<(String, String)>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an object of module ref → reason")
        }
        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(entry) = map.next_entry::<String, String>()? {
                out.push(entry);
            }
            Ok(out)
        }
    }
    d.deserialize_map(Ordered)
}

/// Read a spec, refusing what the format does not know, then check the
/// references serde cannot: node ids that must exist, and fields a node type
/// cannot be drawn without. Every refusal is "could not run" (exit 2).
pub fn parse(json: &str) -> Result<Spec> {
    let spec: Spec = serde_json::from_str(json).context("not a hybrid spec")?;
    validate(&spec)?;
    Ok(spec)
}

fn validate(spec: &Spec) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for n in &spec.nodes {
        if !seen.insert(n.id.as_str()) {
            bail!("node id {:?} is used twice", n.id);
        }
    }
    let known = |id: &str, what: &str| -> Result<()> {
        if seen.contains(id) {
            Ok(())
        } else {
            bail!("{what} names unknown node {id:?}")
        }
    };
    for n in &spec.nodes {
        if let Some(Links::Word(w)) = &n.links {
            if w != "auto" {
                bail!("{}: links is \"auto\" or a list of refs, not {w:?}", n.id);
            }
        }
        if let Some(t) = &n.inside {
            known(t, &format!("{}.in", n.id))?;
        }
        if n.kind == NodeType::Boundary {
            if n.contains.is_empty() {
                bail!("{}: a boundary needs `contains`", n.id);
            }
            for k in &n.contains {
                known(k, &format!("{}.contains", n.id))?;
            }
            continue;
        }
        if n.x.is_none() {
            bail!("{}: needs `x: [x0, x1]`", n.id);
        }
        if n.y.is_none() {
            bail!("{}: needs `y`", n.id);
        }
        if n.kind == NodeType::Tile && n.height.is_none() {
            bail!("{}: a tile needs a fixed `height`", n.id);
        }
        if let Some(YSpec::Relative(RelativeY { below: Some(b), .. })) = &n.y {
            for id in b.items() {
                known(id, &format!("{}.y.below", n.id))?;
            }
        }
    }
    for e in &spec.edges {
        known(&e.from, "an edge's `from`")?;
        known(&e.to, "an edge's `to`")?;
    }
    if spec.legend.rows.is_none() && spec.family.is_none() {
        bail!("the default legend names the spec's `family`; set one, or give the legend its own `rows`");
    }
    Ok(())
}
