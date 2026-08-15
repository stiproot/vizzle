//! Resolve base-type names written in source to classes in the graph.

use std::collections::{BTreeMap, HashMap, HashSet};

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
    let by_name = index_by_name(graph);

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

fn index_by_name(graph: &CodeGraph) -> HashMap<&str, Vec<&Class>> {
    let mut by_name: HashMap<&str, Vec<&Class>> = HashMap::new();
    for class in &graph.classes {
        by_name.entry(class.name.as_str()).or_default().push(class);
    }
    by_name
}

/// Identifiers mentioned by a type expression: `Map~string, Repo~` -> `Map`,
/// `string`, `Repo`. Generic wrappers are kept as candidates — a wrapper that
/// isn't a class in the graph simply resolves to nothing.
fn type_identifiers(expression: &str) -> Vec<&str> {
    expression
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|token| {
            !token.is_empty() && token.chars().next().is_some_and(|c| c.is_alphabetic())
        })
        .collect()
}

/// Structural relations implied by member types: a field's type is an
/// association, a method's parameter or return type is a dependency.
///
/// Only internal targets produce edges — an association to a type outside the
/// parsed set says nothing useful. One edge per (from, to) pair, keeping the
/// strongest kind, so a class holding three fields of one type gets one arrow.
pub fn resolve_member_relations(graph: &CodeGraph) -> Vec<ResolvedRelation> {
    let by_name = index_by_name(graph);
    let mut strongest: BTreeMap<(&str, String), RelationKind> = BTreeMap::new();

    for class in &graph.classes {
        for member in &class.members {
            let kind = if member.is_method {
                RelationKind::Dependency
            } else {
                RelationKind::Association
            };
            for expression in &member.type_refs {
                for identifier in type_identifiers(expression) {
                    let Some(target) = resolve_base(identifier, class, &by_name) else {
                        continue;
                    };
                    let key = (class.qualified.as_str(), target.qualified.clone());
                    strongest
                        .entry(key)
                        .and_modify(|existing| *existing = (*existing).min(kind))
                        .or_insert(kind);
                }
            }
        }
    }

    strongest
        .into_iter()
        .map(|((from, to), kind)| ResolvedRelation {
            from: from.to_owned(),
            to: Target::Internal(to),
            kind,
        })
        .collect()
}

/// Inheritance plus member-implied relations, with inheritance winning when a
/// pair is connected both ways (a subclass that also holds a field of its base
/// reads as inheritance, not association).
pub fn resolve_all_relations(graph: &CodeGraph) -> Vec<ResolvedRelation> {
    let mut relations = resolve_relations(graph);
    let existing: HashSet<(String, String)> = relations
        .iter()
        .filter_map(|r| match &r.to {
            Target::Internal(to) => Some((r.from.clone(), to.clone())),
            Target::External(_) => None,
        })
        .collect();

    for relation in resolve_member_relations(graph) {
        if let Target::Internal(to) = &relation.to {
            if existing.contains(&(relation.from.clone(), to.clone())) {
                continue;
            }
        }
        relations.push(relation);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_file;

    fn kinds(graph: &CodeGraph) -> Vec<(String, String, &'static str)> {
        resolve_all_relations(graph)
            .into_iter()
            .filter_map(|r| match r.to {
                Target::Internal(to) => Some((
                    r.from.rsplit('.').next().unwrap().to_owned(),
                    to.rsplit('.').next().unwrap().to_owned(),
                    r.kind.name(),
                )),
                Target::External(_) => None,
            })
            .collect()
    }

    #[test]
    fn derives_associations_from_fields_and_dependencies_from_signatures() {
        let graph = parse_file(
            "m.ts",
            r#"
export class Engine {}
export class Wheel {}
export class Trip {}
export class Report {}
export class Car {
  private engine: Engine;
  wheels: Wheel[];
  drive(trip: Trip): Report { return null; }
}
"#,
        )
        .unwrap();
        let found = kinds(&graph);
        let relation = |to: &str| {
            found
                .iter()
                .find(|(f, t, _)| f == "Car" && t == to)
                .map(|r| r.2)
        };

        // A field's type is structural; a signature's types are a dependency.
        assert_eq!(relation("Engine"), Some("association"));
        assert_eq!(relation("Wheel"), Some("association")); // through Wheel[]
        assert_eq!(relation("Trip"), Some("dependency"));
        assert_eq!(relation("Report"), Some("dependency"));
    }

    #[test]
    fn inheritance_wins_over_member_implied_relations() {
        let graph = parse_file(
            "m.py",
            "class Base:\n    pass\n\nclass Child(Base):\n    parent: Base\n",
        )
        .unwrap();
        let found = kinds(&graph);
        let child_to_base: Vec<&str> = found
            .iter()
            .filter(|(f, t, _)| f == "Child" && t == "Base")
            .map(|r| r.2)
            .collect();
        assert_eq!(
            child_to_base,
            ["inherits"],
            "one edge, and it is inheritance"
        );
    }

    #[test]
    fn one_edge_per_pair_and_no_self_edges() {
        let graph = parse_file(
            "m.ts",
            r#"
export class Repo {}
export class Service {
  primary: Repo;
  backup: Repo;
  self: Service;
  find(other: Repo): Repo { return other; }
}
"#,
        )
        .unwrap();
        let found = kinds(&graph);
        let to_repo: Vec<&str> = found
            .iter()
            .filter(|(f, t, _)| f == "Service" && t == "Repo")
            .map(|r| r.2)
            .collect();
        // Four mentions, one edge — and the field (association) outranks the
        // method signature (dependency).
        assert_eq!(to_repo, ["association"]);
        assert!(!found
            .iter()
            .any(|(f, t, _)| f == "Service" && t == "Service"));
    }

    #[test]
    fn parameter_types_reach_the_rendered_signature() {
        let graph = parse_file(
            "m.py",
            "class A:\n    def go(self, n: int, flag=False) -> str: ...\n",
        )
        .unwrap();
        let member = &graph.classes[0].members[0];
        assert_eq!(member.detail, "n: int, flag");
        assert_eq!(member.returns.as_deref(), Some("str"));
    }
}
