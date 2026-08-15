//! Export a [`CodeGraph`] as JSON for external renderers (e.g. the d3 HTML view).

use serde_json::{json, Value};

use crate::model::{ChangeKind, Class, CodeGraph, RelationKind};
use crate::resolve::{resolve_relations, Target};

/// Change status as the wire-format string every renderer keys its palette on.
pub(crate) fn change_str(change: ChangeKind) -> &'static str {
    match change {
        ChangeKind::Unchanged => "unchanged",
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Modified => "modified",
    }
}

/// One class as JSON. Shared by the class diagram's export and the component
/// diagram's per-component class detail, so both views describe a class the
/// same way and the renderers can share their drawing code.
pub(crate) fn class_json(class: &Class) -> Value {
    json!({
        "name": class.name,
        "qualified": class.qualified,
        "module": class.module,
        "lang": class.lang.name(),
        "annotation": class.annotation,
        "change": change_str(class.change),
        "members": class.members.iter().map(|m| json!({
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
///   "classes": [{"name", "qualified", "module", "lang", "annotation",
///                "change", "members": [{"name", "visibility", "detail",
///                "returns", "isMethod", "isStatic", "isAbstract", "change"}]}],
///   "relations": [{"from", "to", "kind", "external"}],
///   "stats": {"classes", "relations", "diff"}
/// }
/// ```
pub fn to_json(graph: &CodeGraph) -> String {
    let classes: Vec<Value> = graph.classes.iter().map(class_json).collect();

    let relations: Vec<Value> = resolve_relations(graph)
        .iter()
        .map(|r| {
            let (to, external) = match &r.to {
                Target::Internal(qualified) => (qualified.clone(), false),
                Target::External(name) => (name.clone(), true),
            };
            json!({
                "from": r.from,
                "to": to,
                "external": external,
                "kind": match r.kind {
                    RelationKind::Inherits => "inherits",
                    RelationKind::Implements => "implements",
                },
            })
        })
        .collect();

    let diff = graph
        .classes
        .iter()
        .any(|c| c.change != ChangeKind::Unchanged);

    json!({
        "classes": classes,
        "relations": relations,
        "stats": {
            "classes": graph.classes.len(),
            "relations": relations.len(),
            "diff": diff,
        },
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
}
