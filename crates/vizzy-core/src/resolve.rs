//! Resolve base-type names written in source to classes in the graph.

use std::collections::HashMap;

use crate::model::{Class, CodeGraph, RelationKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Qualified name of a class present in the graph.
    Internal(String),
    /// Bare name of a type outside the parsed set.
    External(String),
}

#[derive(Debug, Clone)]
pub struct ResolvedRelation {
    /// Qualified name of the subtype.
    pub from: String,
    pub to: Target,
    pub kind: RelationKind,
}

/// Resolve every base reference in the graph: same module first, then a
/// globally unique name; ambiguous names resolve to `External` (a wrong edge
/// is worse than a missing one).
pub fn resolve_relations(graph: &CodeGraph) -> Vec<ResolvedRelation> {
    let mut by_name: HashMap<&str, Vec<&Class>> = HashMap::new();
    for class in &graph.classes {
        by_name.entry(class.name.as_str()).or_default().push(class);
    }

    let mut relations = Vec::new();
    for class in &graph.classes {
        for relation in &class.bases {
            let to = match resolve_base(&relation.to, class, &by_name) {
                Some(target) => Target::Internal(target.qualified.clone()),
                None => Target::External(relation.to.clone()),
            };
            relations.push(ResolvedRelation {
                from: class.qualified.clone(),
                to,
                kind: relation.kind,
            });
        }
    }
    relations
}

fn resolve_base<'g>(
    to: &str,
    from: &Class,
    by_name: &HashMap<&str, Vec<&'g Class>>,
) -> Option<&'g Class> {
    let candidates = by_name.get(to)?;
    if let Some(same_module) = candidates
        .iter()
        .find(|c| c.module == from.module && c.qualified != from.qualified)
    {
        return Some(same_module);
    }
    match candidates.len() {
        1 if candidates[0].qualified != from.qualified => Some(candidates[0]),
        _ => None,
    }
}
