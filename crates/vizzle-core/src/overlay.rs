//! Planned parts over real code: merge an overlay (components, classes, edges
//! and relations a change will add, the real classes it will touch or delete)
//! into a component graph, marking each with the change a diff would give it
//! (`docs/plans/impl/atoms-and-overlay.md` §3–§4).
//!
//! Two kinds of trouble, kept apart because CI needs them apart: an overlay
//! that cannot be read (bad JSON, an unknown field or relation kind) is an
//! error; an overlay that reads but names what the code does not have (a
//! `modify` target that was renamed, an `add` that has since been built) is
//! *stale* — a list of findings, not an error, so `--check` can report all of
//! them at once.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::component::{dir_of, last_segment, Component, ComponentEdge, EdgeTarget};
use crate::import::{relation_kind, static_langs, ImportedGraph, WireClass, WireMember};
use crate::model::{ChangeKind, Language, RelationKind};
use crate::resolve::{ResolvedRelation, Target};

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    #[serde(default)]
    add: Additions,
    #[serde(default)]
    modify: Vec<Modification>,
    #[serde(default)]
    remove: Vec<Removal>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Additions {
    #[serde(default)]
    components: Vec<PlannedComponent>,
    #[serde(default)]
    classes: Vec<WireClass>,
    #[serde(default)]
    edges: Vec<PlannedEdge>,
    #[serde(default)]
    class_relations: Vec<PlannedRelation>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct PlannedComponent {
    path: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    group: Option<String>,
    /// Repo-relative paths of the files the component will own.
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    langs: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct PlannedEdge {
    from: String,
    to: String,
    #[serde(default)]
    external: bool,
    #[serde(default)]
    weight: Option<usize>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct PlannedRelation {
    from: String,
    to: String,
    kind: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Modification {
    class: String,
    #[serde(default)]
    members: Vec<MemberRef>,
}

/// A name touches a member the class has; an object plans a new one.
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum MemberRef {
    Existing(String),
    Planned(WireMember),
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Removal {
    class: String,
}

/// Read an overlay document.
pub fn parse(json: &str) -> Result<Overlay> {
    serde_json::from_str(json).context("not a valid overlay")
}

/// A relation kind as an author writes it: the export's own names, plus the
/// UML words for the two inheritance arrows, because a design document is
/// where a reader says "realization".
fn planned_kind(name: &str) -> Result<RelationKind> {
    match name {
        "realization" => Ok(RelationKind::Implements),
        "generalization" => Ok(RelationKind::Inherits),
        other => relation_kind(other),
    }
}

/// A repo-relative file as the component lists it: relative to the
/// component's own directory, the way `changedFiles` is written.
fn within(component: &str, file: &str) -> String {
    if component.is_empty() {
        return file.to_owned();
    }
    file.strip_prefix(component)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(file)
        .to_owned()
}

/// Mark a component as touched by a change to `file`: a planned component is
/// already `added`, a real one becomes `modified`, and either lists the file.
fn touch(component: &mut Component, file: &str) {
    if component.change == ChangeKind::Unchanged {
        component.change = ChangeKind::Modified;
    }
    let file = within(&component.path, file);
    if !component.changed_files.contains(&file) {
        component.changed_files.push(file);
        component.changed_files.sort();
    }
}

fn mark_modified(change: &mut ChangeKind) {
    if *change == ChangeKind::Unchanged {
        *change = ChangeKind::Modified;
    }
}

/// Merge `overlay` into `graph`. Returns the merged graph and the stale
/// findings; the merge still applies everything that could be applied, so a
/// caller that wants a picture regardless can have one, but a caller that
/// must not draw a lie should refuse on any finding.
pub fn apply(
    mut imported: ImportedGraph,
    overlay: Overlay,
) -> Result<(ImportedGraph, Vec<String>)> {
    let mut stale = Vec::new();
    let graph = &mut imported.graph;

    for planned in overlay.add.components {
        if graph.components.iter().any(|c| c.path == planned.path) {
            stale.push(format!(
                "add.components: `{}` already exists; say `modify` or drop it",
                planned.path
            ));
            continue;
        }
        let langs = match &planned.langs {
            Some(names) => static_langs(names),
            None => Ok(planned
                .files
                .iter()
                .filter_map(|f| Language::from_path(f).map(|l| l.name()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()),
        }
        .with_context(|| format!("add.components `{}`", planned.path))?;
        let mut changed_files: Vec<String> = planned
            .files
            .iter()
            .map(|f| within(&planned.path, f))
            .collect();
        changed_files.sort();
        changed_files.dedup();
        graph.components.push(Component {
            name: planned
                .name
                .unwrap_or_else(|| last_segment(&planned.path).to_owned()),
            manifest_name: None,
            group: planned
                .group
                .unwrap_or_else(|| dir_of(&planned.path).to_owned()),
            langs,
            files: changed_files.len(),
            classes: 0,
            fingerprint: 0,
            file_hashes: Default::default(),
            changed_files,
            change: ChangeKind::Added,
            is_boundary: false,
            path: planned.path,
        });
    }

    for planned in overlay.add.classes {
        if graph
            .classes
            .iter()
            .any(|p| p.class.qualified == planned.qualified)
        {
            stale.push(format!(
                "add.classes: `{}` already exists; say `modify` or drop it",
                planned.qualified
            ));
            continue;
        }
        let Some(component) = graph
            .components
            .iter_mut()
            .find(|c| c.path == planned.component)
        else {
            stale.push(format!(
                "add.classes `{}`: no component `{}`",
                planned.qualified, planned.component
            ));
            continue;
        };
        let placed = planned.into_placed(Some(ChangeKind::Added))?;
        let known_file = component
            .changed_files
            .contains(&within(&component.path, &placed.class.file));
        touch(component, &placed.class.file);
        // A planned component's file count is its file list; a real one's
        // files are not in the JSON, so its count is left as extracted.
        if component.change == ChangeKind::Added && !known_file {
            component.files += 1;
        }
        component.classes += 1;
        if !component.langs.contains(&placed.class.lang.name()) {
            component.langs.push(placed.class.lang.name());
            component.langs.sort();
        }
        graph.classes.push(placed);
    }

    for modification in overlay.modify {
        let Some(placed) = graph
            .classes
            .iter_mut()
            .find(|p| p.class.qualified == modification.class)
        else {
            stale.push(format!(
                "modify: no class `{}` in the code",
                modification.class
            ));
            continue;
        };
        for member in modification.members {
            match member {
                MemberRef::Existing(name) => {
                    match placed.class.members.iter_mut().find(|m| m.name == name) {
                        Some(m) => mark_modified(&mut m.change),
                        None => stale.push(format!(
                            "modify `{}`: no member `{name}`",
                            modification.class
                        )),
                    }
                }
                MemberRef::Planned(wire) => {
                    if placed.class.members.iter().any(|m| m.name == wire.name) {
                        stale.push(format!(
                            "modify `{}`: member `{}` already exists; name it instead",
                            modification.class, wire.name
                        ));
                        continue;
                    }
                    let member = wire
                        .into_member(Some(ChangeKind::Added))
                        .with_context(|| format!("modify `{}`", modification.class))?;
                    placed.class.members.push(member);
                }
            }
        }
        mark_modified(&mut placed.class.change);
        let (component, file) = (placed.component.clone(), placed.class.file.clone());
        if let Some(c) = graph.components.iter_mut().find(|c| c.path == component) {
            touch(c, &file);
        }
    }

    for removal in overlay.remove {
        let Some(placed) = graph
            .classes
            .iter_mut()
            .find(|p| p.class.qualified == removal.class)
        else {
            stale.push(format!("remove: no class `{}` in the code", removal.class));
            continue;
        };
        // As a diff draws a deleted class: the class and every member struck.
        placed.class.change = ChangeKind::Removed;
        for m in &mut placed.class.members {
            m.change = ChangeKind::Removed;
        }
        let (component, file) = (placed.component.clone(), placed.class.file.clone());
        if let Some(c) = graph.components.iter_mut().find(|c| c.path == component) {
            touch(c, &file);
        }
    }

    let components: BTreeSet<String> = graph.components.iter().map(|c| c.path.clone()).collect();
    for planned in overlay.add.edges {
        let to = if planned.external {
            EdgeTarget::External(planned.to.clone())
        } else {
            EdgeTarget::Component(planned.to.clone())
        };
        let mut missing: Vec<&str> = Vec::new();
        if !components.contains(&planned.from) {
            missing.push(&planned.from);
        }
        if !planned.external && !components.contains(&planned.to) {
            missing.push(&planned.to);
        }
        if !missing.is_empty() {
            stale.push(format!(
                "add.edges `{}` -> `{}`: no component {}",
                planned.from,
                planned.to,
                missing
                    .iter()
                    .map(|p| format!("`{p}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            continue;
        }
        if graph
            .edges
            .iter()
            .any(|e| e.from == planned.from && e.to == to)
        {
            stale.push(format!(
                "add.edges: `{}` -> `{}` already exists; drop it",
                planned.from, planned.to
            ));
            continue;
        }
        graph.edges.push(ComponentEdge {
            from: planned.from,
            to,
            weight: planned.weight.unwrap_or(1),
            change: ChangeKind::Added,
        });
    }

    let classes: BTreeSet<String> = graph
        .classes
        .iter()
        .map(|p| p.class.qualified.clone())
        .collect();
    for planned in overlay.add.class_relations {
        let kind = planned_kind(&planned.kind)
            .with_context(|| format!("add.classRelations `{}`", planned.from))?;
        let missing: Vec<String> = [&planned.from, &planned.to]
            .into_iter()
            .filter(|q| !classes.contains(*q))
            .map(|q| format!("`{q}`"))
            .collect();
        if !missing.is_empty() {
            stale.push(format!(
                "add.classRelations `{}` -> `{}`: no class {}",
                planned.from,
                planned.to,
                missing.join(", ")
            ));
            continue;
        }
        let target = Target::Internal(planned.to.clone());
        if imported
            .relations
            .iter()
            .any(|r| r.from == planned.from && r.to == target)
        {
            stale.push(format!(
                "add.classRelations: `{}` -> `{}` already exists; drop it",
                planned.from, planned.to
            ));
            continue;
        }
        imported.relations.push(ResolvedRelation {
            from: planned.from,
            to: target,
            kind,
        });
    }

    imported.graph.normalize();
    Ok((imported, stale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component;
    use crate::import::component_graph;

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
            .collect()
    }

    fn real() -> ImportedGraph {
        let manifests = pairs(&[
            ("pkgs/core/package.json", r#"{"name": "@x/core"}"#),
            ("apps/svc/package.json", r#"{"name": "svc"}"#),
        ]);
        let files = pairs(&[
            (
                "pkgs/core/src/index.ts",
                "export class Core {\n  run(): void {}\n}\nexport class Old {}\n",
            ),
            (
                "apps/svc/src/main.ts",
                "import { Core } from \"@x/core\";\nexport class Svc {}\n",
            ),
        ]);
        let graph = component::build(&files, &manifests, &[]).unwrap();
        component_graph(&component::to_json(&graph, true)).unwrap()
    }

    fn merge(overlay: &str) -> (serde_json::Value, Vec<String>) {
        let (merged, stale) = apply(real(), parse(overlay).unwrap()).unwrap();
        (serde_json::from_str(&merged.to_json()).unwrap(), stale)
    }

    fn find<'v>(items: &'v serde_json::Value, key: &str, value: &str) -> &'v serde_json::Value {
        items
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i[key] == value)
            .unwrap_or_else(|| panic!("no {key} = {value}"))
    }

    const PROPOSAL: &str = r#"{
      "add": {
        "components": [{"path": "pkgs/ctl", "files": ["pkgs/ctl/src/contract.ts"]}],
        "classes": [
          {"qualified": "pkgs.ctl.src.contract.Controller", "component": "pkgs/ctl",
           "file": "pkgs/ctl/src/contract.ts", "annotation": "interface",
           "members": [{"name": "start", "detail": "job", "returns": "Result", "isMethod": true}]},
          {"qualified": "pkgs.core.src.steps.Steps", "component": "pkgs/core",
           "file": "pkgs/core/src/steps.ts"}
        ],
        "edges": [{"from": "pkgs/core", "to": "pkgs/ctl"}],
        "classRelations": [{"from": "pkgs.core.src.steps.Steps",
                            "to": "pkgs.ctl.src.contract.Controller", "kind": "realization"}]
      },
      "modify": [{"class": "pkgs.core.src.index.Core", "members": ["run", {"name": "stop", "isMethod": true}]}],
      "remove": [{"class": "pkgs.core.src.index.Old"}]
    }"#;

    #[test]
    fn a_proposal_merges_with_the_marks_a_diff_would_give() {
        let (value, stale) = merge(PROPOSAL);
        assert!(stale.is_empty(), "{stale:?}");

        let ctl = find(&value["components"], "path", "pkgs/ctl");
        assert_eq!(ctl["change"], "added");
        assert_eq!(ctl["name"], "ctl");
        assert_eq!(ctl["group"], "pkgs");
        assert_eq!(ctl["langs"], serde_json::json!(["typescript"]));
        assert_eq!(ctl["files"], 1);
        assert_eq!(ctl["classes"], 1);
        assert_eq!(ctl["changedFiles"], serde_json::json!(["src/contract.ts"]));

        let core = find(&value["components"], "path", "pkgs/core");
        assert_eq!(core["change"], "modified");
        assert_eq!(
            core["changedFiles"],
            serde_json::json!(["src/index.ts", "src/steps.ts"])
        );
        assert_eq!(
            find(&value["components"], "path", "apps/svc")["change"],
            "unchanged"
        );

        let controller = find(
            &value["classes"],
            "qualified",
            "pkgs.ctl.src.contract.Controller",
        );
        assert_eq!(controller["change"], "added");
        assert_eq!(controller["name"], "Controller");
        assert_eq!(controller["module"], "pkgs.ctl.src.contract");
        assert_eq!(controller["lang"], "typescript");
        assert_eq!(controller["members"][0]["change"], "added");
        assert_eq!(controller["members"][0]["visibility"], "+");

        let core_class = find(&value["classes"], "qualified", "pkgs.core.src.index.Core");
        assert_eq!(core_class["change"], "modified");
        assert_eq!(
            find(&core_class["members"], "name", "run")["change"],
            "modified"
        );
        assert_eq!(
            find(&core_class["members"], "name", "stop")["change"],
            "added"
        );
        assert_eq!(
            find(&value["classes"], "qualified", "pkgs.core.src.index.Old")["change"],
            "removed"
        );

        let edge = find(&value["edges"], "to", "pkgs/ctl");
        assert_eq!(edge["change"], "added");
        let relation = find(
            &value["classRelations"],
            "from",
            "pkgs.core.src.steps.Steps",
        );
        assert_eq!(relation["kind"], "implements");

        // Recomputed, never read: two components and one edge changed.
        assert_eq!(value["stats"]["diff"], true);
        assert_eq!(
            value["stats"]["changes"],
            serde_json::json!({"added": 2, "removed": 0, "modified": 1})
        );
    }

    #[test]
    fn a_target_that_names_nothing_is_stale() {
        let (_, stale) = merge(
            r#"{"modify": [{"class": "pkgs.core.src.index.Renamed"},
                           {"class": "pkgs.core.src.index.Core", "members": ["gone"]}],
                "remove": [{"class": "nope.Nope"}],
                "add": {"edges": [{"from": "pkgs/core", "to": "pkgs/none"}],
                        "classRelations": [{"from": "a.A", "to": "pkgs.core.src.index.Core", "kind": "dependency"}],
                        "classes": [{"qualified": "x.X", "component": "pkgs/none", "file": "x.ts"}]}}"#,
        );
        assert_eq!(stale.len(), 6, "{stale:#?}");
        assert!(stale.iter().any(|s| s.contains("Renamed")));
        assert!(stale.iter().any(|s| s.contains("no member `gone`")));
        assert!(stale.iter().any(|s| s.contains("nope.Nope")));
        assert!(stale.iter().any(|s| s.contains("no component `pkgs/none`")));
        assert!(stale.iter().any(|s| s.contains("no class `a.A`")));
    }

    #[test]
    fn an_addition_that_was_built_is_stale() {
        let (_, stale) = merge(
            r#"{"add": {"components": [{"path": "pkgs/core"}],
                        "classes": [{"qualified": "pkgs.core.src.index.Core", "component": "pkgs/core", "file": "pkgs/core/src/index.ts"}],
                        "edges": [{"from": "apps/svc", "to": "pkgs/core"}]}}"#,
        );
        assert_eq!(stale.len(), 3, "{stale:#?}");
        assert!(stale.iter().all(|s| s.contains("already exists")));
    }

    #[test]
    fn an_unreadable_overlay_is_an_error_not_a_finding() {
        assert!(parse(r#"{"ad": {}}"#).is_err(), "a misspelt section");
        assert!(parse("not json").is_err());
        let overlay = parse(
            r#"{"add": {"classRelations": [{"from": "pkgs.core.src.index.Core",
                "to": "pkgs.core.src.index.Old", "kind": "friendship"}]}}"#,
        )
        .unwrap();
        let err = format!("{:#}", apply(real(), overlay).unwrap_err());
        assert!(err.contains("unknown relation kind `friendship`"), "{err}");
    }

    #[test]
    fn a_change_written_on_a_planned_class_or_member_is_refused_not_overridden() {
        // Decision 13: the overlay decides what a planned part's change is; a `change` the author wrote is
        // refused like an unknown field, never silently replaced by `added`.
        let class = |extra: &str| {
            parse(&format!(
                r#"{{"add": {{"classes": [{{"qualified": "pkgs.core.src.plan.Plan", "component": "pkgs/core",
                    "file": "pkgs/core/src/plan.ts"{extra}}}]}}}}"#
            ))
            .unwrap()
        };
        let err = format!(
            "{:#}",
            apply(real(), class(r#", "change": "modified""#)).unwrap_err()
        );
        assert!(err.contains("a planned class takes no `change`"), "{err}");
        let err = format!(
            "{:#}",
            apply(
                real(),
                class(r#", "members": [{"name": "run", "change": "removed"}]"#)
            )
            .unwrap_err()
        );
        assert!(
            err.contains("member `run`: a planned member takes no `change`"),
            "{err}"
        );
        // without the field, the same class is planned and marked added
        assert!(apply(real(), class("")).is_ok());
    }
}
