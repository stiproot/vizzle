//! Read a component graph back from the JSON [`crate::component::to_json`]
//! writes, so a graph that left the process (`vizzle component -f json`) can be
//! overlaid and drawn by the same renderers that drew it from source
//! (`docs/plans/impl/atoms-and-overlay.md` §5).
//!
//! The wire structs below are the read side of `component::to_json_with_lens`
//! and `export::class_json`; they refuse unknown fields, so a field added to
//! the export without being taught here fails the round-trip test rather than
//! vanishing silently on the way back in. The strings inside a field (change,
//! language, visibility, relation kind) are never spelled here: each is read
//! through the inverse of the function that writes it.
//!
//! What JSON does not carry cannot come back: file hashes, manifest names,
//! member type references. Nothing downstream of an import needs them —
//! they feed detection and diffing, which ran before the export.

use std::collections::BTreeSet;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::component::{Component, ComponentEdge, ComponentGraph, EdgeTarget, PlacedClass};
use crate::export::parse_change;
use crate::model::{ChangeKind, Class, Language, Member, RelationKind, Visibility};
use crate::resolve::{ResolvedRelation, Target};

/// A component graph read from JSON, with what the JSON carries beside the
/// graph itself: the class relations (resolved before export, and with no
/// source left to resolve them again) and the reader's lens.
#[derive(Debug, Clone)]
pub struct ImportedGraph {
    pub graph: ComponentGraph,
    pub relations: Vec<ResolvedRelation>,
    pub highlight: Option<BTreeSet<String>>,
}

impl ImportedGraph {
    /// Back to the JSON it came from: the same export, the carried relations.
    pub fn to_json(&self) -> String {
        crate::component::json_with_relations(
            &self.graph,
            true,
            self.highlight.as_ref(),
            &self.relations,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WireGraph {
    components: Vec<WireComponent>,
    edges: Vec<WireEdge>,
    #[serde(default)]
    classes: Vec<WireClass>,
    #[serde(default)]
    class_relations: Vec<WireRelation>,
    #[serde(default)]
    stats: WireStats,
}

/// Only what the export cannot recompute; counts and the change verdict are
/// derived again on the way out, so they are not read.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WireStats {
    #[serde(default)]
    omitted: usize,
    #[serde(default)]
    selection: Vec<String>,
    #[serde(default)]
    highlight: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WireComponent {
    name: String,
    path: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    langs: Vec<String>,
    #[serde(default)]
    files: usize,
    #[serde(default)]
    classes: usize,
    #[serde(default)]
    change: Option<String>,
    #[serde(default)]
    changed_files: Vec<String>,
    /// Per-component echo of `stats.highlight`; the stats list is what is read.
    #[serde(default)]
    #[allow(dead_code)]
    highlight: Option<bool>,
    #[serde(default)]
    boundary: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WireEdge {
    from: String,
    to: String,
    #[serde(default)]
    external: bool,
    #[serde(default = "one")]
    weight: usize,
    #[serde(default)]
    change: Option<String>,
}

fn one() -> usize {
    1
}

/// One class, as `export::class_json` writes it plus the owning component.
/// Shared with the overlay's `add.classes`, which may omit what can be
/// derived; [`WireClass::into_placed`] is the one place those defaults live.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct WireClass {
    pub qualified: String,
    pub component: String,
    pub file: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub annotation: Option<String>,
    #[serde(default)]
    pub change: Option<String>,
    #[serde(default)]
    pub members: Vec<WireMember>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct WireMember {
    pub name: String,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub returns: Option<String>,
    #[serde(default)]
    pub is_method: bool,
    #[serde(default)]
    pub is_static: bool,
    #[serde(default)]
    pub is_abstract: bool,
    #[serde(default)]
    pub change: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireRelation {
    pub from: String,
    pub to: String,
    pub kind: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub external: bool,
}

fn change_of(value: Option<&str>) -> Result<ChangeKind> {
    match value {
        None => Ok(ChangeKind::Unchanged),
        Some(name) => parse_change(name).ok_or_else(|| anyhow!("unknown change `{name}`")),
    }
}

impl WireMember {
    /// `forced` overrides the member's own change (an overlay addition is
    /// `added` whatever it says); `None` reads it from the JSON.
    pub(crate) fn into_member(self, forced: Option<ChangeKind>) -> Result<Member> {
        let visibility = match self.visibility.as_deref() {
            None => Visibility::default(),
            Some(sigil) => Visibility::from_sigil(sigil)
                .ok_or_else(|| anyhow!("member `{}`: unknown visibility `{sigil}`", self.name))?,
        };
        let change = match forced {
            Some(change) => change,
            None => change_of(self.change.as_deref())?,
        };
        Ok(Member {
            name: self.name,
            visibility,
            detail: self.detail,
            returns: self.returns,
            is_method: self.is_method,
            is_static: self.is_static,
            is_abstract: self.is_abstract,
            change,
            ..Member::default()
        })
    }
}

impl WireClass {
    /// The class, with every omitted field derived the way extraction would:
    /// `name` and `module` from the dotted qualified name, `lang` from the
    /// file suffix. `forced` as for [`WireMember::into_member`].
    pub(crate) fn into_placed(self, forced: Option<ChangeKind>) -> Result<PlacedClass> {
        let qualified = self.qualified;
        let (module, bare) = match qualified.rsplit_once('.') {
            Some((module, bare)) => (module.to_owned(), bare.to_owned()),
            None => (String::new(), qualified.clone()),
        };
        let lang = match self.lang.as_deref() {
            Some(name) => Language::from_name(name)
                .ok_or_else(|| anyhow!("class `{qualified}`: unknown lang `{name}`"))?,
            None => Language::from_path(&self.file).ok_or_else(|| {
                anyhow!(
                    "class `{qualified}`: cannot tell its language from `{}`; give `lang`",
                    self.file
                )
            })?,
        };
        let change = match forced {
            Some(change) => change,
            None => {
                change_of(self.change.as_deref()).with_context(|| format!("class `{qualified}`"))?
            }
        };
        let members = self
            .members
            .into_iter()
            .map(|m| m.into_member(forced))
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("class `{qualified}`"))?;
        Ok(PlacedClass {
            component: self.component,
            class: Class {
                name: self.name.unwrap_or(bare),
                module: self.module.unwrap_or(module),
                qualified,
                file: self.file,
                annotation: self.annotation,
                bases: Vec::new(),
                members,
                lang,
                change,
            },
        })
    }
}

/// A relation kind as the export writes it ([`RelationKind::name`]).
pub(crate) fn relation_kind(name: &str) -> Result<RelationKind> {
    RelationKind::from_name(name).ok_or_else(|| anyhow!("unknown relation kind `{name}`"))
}

/// A component's language list as `&'static str`, the way detection stores it.
pub(crate) fn static_langs(names: &[String]) -> Result<Vec<&'static str>> {
    let mut langs: BTreeSet<&'static str> = BTreeSet::new();
    for name in names {
        let lang = Language::from_name(name).ok_or_else(|| anyhow!("unknown lang `{name}`"))?;
        langs.insert(lang.name());
    }
    Ok(langs.into_iter().collect())
}

/// Read a component graph from `vizzle component -f json` (or `diff --type
/// component -f json`, or `vizzle overlay`'s output).
pub fn component_graph(json: &str) -> Result<ImportedGraph> {
    let value: serde_json::Value = serde_json::from_str(json).context("not valid JSON")?;
    if value.get("components").is_none() {
        bail!(
            "not a component graph: no `components` key (expected the output of \
             `vizzle component -f json`)"
        );
    }
    let wire: WireGraph = serde_json::from_value(value).context("not a component graph")?;

    let components = wire
        .components
        .into_iter()
        .map(|c| {
            Ok(Component {
                langs: static_langs(&c.langs).with_context(|| format!("component `{}`", c.path))?,
                change: change_of(c.change.as_deref())
                    .with_context(|| format!("component `{}`", c.path))?,
                name: c.name,
                manifest_name: None,
                path: c.path,
                group: c.group,
                files: c.files,
                classes: c.classes,
                fingerprint: 0,
                file_hashes: Default::default(),
                changed_files: c.changed_files,
                is_boundary: c.boundary,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = wire
        .edges
        .into_iter()
        .map(|e| {
            Ok(ComponentEdge {
                change: change_of(e.change.as_deref())
                    .with_context(|| format!("edge `{}` -> `{}`", e.from, e.to))?,
                to: if e.external {
                    EdgeTarget::External(e.to)
                } else {
                    EdgeTarget::Component(e.to)
                },
                from: e.from,
                weight: e.weight,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let classes = wire
        .classes
        .into_iter()
        .map(|c| c.into_placed(None))
        .collect::<Result<Vec<_>>>()?;
    let relations = wire
        .class_relations
        .into_iter()
        .map(|r| {
            Ok(ResolvedRelation {
                kind: relation_kind(&r.kind)?,
                from: r.from,
                to: Target::Internal(r.to),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ImportedGraph {
        graph: ComponentGraph {
            components,
            edges,
            classes,
            selection: wire.stats.selection,
            omitted: wire.stats.omitted,
        },
        relations,
        highlight: wire.stats.highlight.map(|lit| lit.into_iter().collect()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::{self, ComponentRenderOptions};

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
            .collect()
    }

    /// Two packages, a class hierarchy with a field, and a private module
    /// helper — enough to exercise every field the export writes.
    fn sample(core_body: &str) -> ComponentGraph {
        let manifests = pairs(&[
            ("pkgs/core/package.json", r#"{"name": "@x/core"}"#),
            ("apps/svc/package.json", r#"{"name": "svc"}"#),
        ]);
        let files = pairs(&[
            ("pkgs/core/src/index.ts", core_body),
            (
                "apps/svc/src/main.ts",
                "import { Core } from \"@x/core\";\nexport class Base {}\nexport class Svc extends Base {\n  private core: Core;\n  run(n: number): void {}\n}\nfunction helper() {}\nexport function api() {}\n",
            ),
        ]);
        component::build(&files, &manifests, &[]).unwrap()
    }

    #[test]
    fn a_graph_round_trips_through_its_json() {
        let graph = sample("export class Core {}\n");
        let lit: BTreeSet<String> = ["apps/svc".to_owned()].into();
        for highlight in [None, Some(&lit)] {
            let json = component::to_json_with_lens(&graph, true, highlight);
            let back = component_graph(&json).unwrap();
            // Relations and module boxes are what a careless import loses.
            assert!(!back.relations.is_empty());
            assert!(back.graph.classes.iter().any(|p| p.class.is_module_box()));
            assert_eq!(back.to_json(), json);
        }
    }

    #[test]
    fn a_diff_round_trips_and_draws_the_same_mermaid() {
        let base = sample("export class Core {}\n");
        let head = sample("export class Core { x = 1 }\nexport class Added {}\n");
        let merged = component::diff(&base, &head);
        let json = component::to_json(&merged, true);
        let back = component_graph(&json).unwrap();
        assert_eq!(back.to_json(), json);
        let opts = ComponentRenderOptions::default();
        assert_eq!(
            component::render_mermaid(&back.graph, &opts),
            component::render_mermaid(&merged, &opts)
        );
    }

    #[test]
    fn a_class_graph_is_refused_by_name() {
        let err = component_graph(r#"{"classes": [], "relations": []}"#).unwrap_err();
        assert!(err.to_string().contains("not a component graph"), "{err}");
    }

    #[test]
    fn an_unknown_change_is_an_error() {
        let json =
            r#"{"components": [{"name": "a", "path": "a", "change": "renamed"}], "edges": []}"#;
        let err = format!("{:#}", component_graph(json).unwrap_err());
        assert!(err.contains("unknown change `renamed`"), "{err}");
    }
}
