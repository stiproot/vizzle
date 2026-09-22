//! Export a [`CodeGraph`] as JSON for external renderers (e.g. the d3 HTML view).

use serde_json::{json, Value};

use crate::model::{ChangeCounts, ChangeKind, Class, CodeGraph};
use crate::resolve::{resolve_all_relations, Target};

/// Change status as the wire-format string every renderer keys its palette on.
pub(crate) fn change_str(change: ChangeKind) -> &'static str {
    match change {
        ChangeKind::Unchanged => "unchanged",
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Modified => "modified",
    }
}

/// Change counts as JSON: the shape `stats.changes` takes in both exports and
/// in the CLI's `--stats` sidecar.
pub fn change_counts_json(counts: &ChangeCounts) -> Value {
    json!({
        "added": counts.added,
        "removed": counts.removed,
        "modified": counts.modified,
    })
}

/// One resolved relation as JSON, shared by both diagram exports.
pub(crate) fn relation_json(relation: &crate::resolve::ResolvedRelation) -> Value {
    let (to, external) = match &relation.to {
        Target::Internal(qualified) => (qualified.clone(), false),
        Target::External(name) => (name.clone(), true),
    };
    json!({
        "from": relation.from,
        "to": to,
        "external": external,
        "kind": relation.kind.name(),
    })
}

/// One class as JSON. Shared by the class diagram's export and the component
/// diagram's per-component class detail, so both views describe a class the
/// same way and the renderers can share their drawing code.
pub(crate) fn class_json(class: &Class) -> Value {
    json!({
        "name": class.name,
        "qualified": class.qualified,
        "module": class.module,
        "file": class.file,
        "lang": class.lang.name(),
        "annotation": class.annotation,
        "change": change_str(class.change),
        "members": class.drawn_members().map(|m| json!({
            "name": m.name,
            "visibility": m.visibility.sigil().to_string(),
            "detail": m.detail,
            "returns": m.returns,
            "isMethod": m.is_method,
            "isStatic": m.is_static,
            "isAbstract": m.is_abstract,
            "change": change_str(m.change),
        })).collect::<Vec<_>>(),
    })
}

/// Serialize the graph with resolved relations. Shape:
///
/// ```json
/// {
///   "classes": [{"name", "qualified", "module", "file", "lang", "annotation",
///                "change", "members": [{"name", "visibility", "detail",
///                "returns", "isMethod", "isStatic", "isAbstract", "change"}]}],
///   "relations": [{"from", "to", "kind", "external"}],
///   "stats": {"classes", "relations", "diff"}
/// }
/// ```
pub fn to_json(graph: &CodeGraph) -> String {
    to_json_with_lens(graph, None)
}

/// [`to_json`] with the reader's lens: each class gains `"highlight": bool`
/// and `stats.highlight` lists the lit qualified names, so the page can dim
/// the rest the way the mermaid does. Without a lens neither key appears.
pub fn to_json_with_lens(
    graph: &CodeGraph,
    highlight: Option<&std::collections::BTreeSet<String>>,
) -> String {
    let classes: Vec<Value> = graph
        .classes
        .iter()
        .map(|c| {
            let mut v = class_json(c);
            if let Some(lit) = highlight {
                v["highlight"] = Value::Bool(lit.contains(&c.qualified));
            }
            v
        })
        .collect();

    let relations: Vec<Value> = resolve_all_relations(graph)
        .iter()
        .map(relation_json)
        .collect();

    let changes = graph.change_counts();

    let mut stats = json!({
        "classes": graph.classes.len(),
        "relations": relations.len(),
        "diff": changes.changed(),
        "changes": change_counts_json(&changes),
    });
    if let Some(lit) = highlight {
        stats["highlight"] = json!(lit.iter().collect::<Vec<_>>());
    }
    json!({
        "classes": classes,
        "relations": relations,
        "stats": stats,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_file;

    #[test]
    fn exports_resolved_relations() {
        let graph = parse_file(
            "pkg/mod.py",
            "class Base:\n    pass\n\nclass Child(Base, External):\n    name: str\n",
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&to_json(&graph)).unwrap();
        assert_eq!(value["stats"]["classes"], 2);
        let relations = value["relations"].as_array().unwrap();
        assert_eq!(relations.len(), 2);
        let internal = relations.iter().find(|r| r["external"] == false).unwrap();
        assert_eq!(internal["to"], "pkg.mod.Base");
        let external = relations.iter().find(|r| r["external"] == true).unwrap();
        assert_eq!(external["to"], "External");
    }

    #[test]
    fn exports_the_file_and_only_the_drawn_members() {
        let graph = parse_file(
            "pkg/mod.py",
            "def _helper(): ...\ndef api(): ...\nclass C:\n    def _m(self): ...\n",
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&to_json(&graph)).unwrap();
        let classes = value["classes"].as_array().unwrap();
        let by_name = |n: &str| classes.iter().find(|c| c["name"] == n).unwrap();
        assert_eq!(by_name("C")["file"], "pkg/mod.py");
        // A class's private method is its shape and is exported; an unchanged
        // private module function is not surface and is not.
        assert_eq!(by_name("C")["members"].as_array().unwrap().len(), 1);
        let module_members: Vec<&str> = by_name("mod")["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["name"].as_str().unwrap())
            .collect();
        assert_eq!(module_members, vec!["api"]);
    }
}
