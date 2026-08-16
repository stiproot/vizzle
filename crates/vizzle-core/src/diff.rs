//! Compare two code graphs (base revision vs head) and produce a single
//! annotated graph where every class carries a [`ChangeKind`].

use std::collections::{HashMap, HashSet};

use crate::model::{ChangeKind, Class, CodeGraph};

/// Merge `base` and `head` into one graph annotated with change status.
///
/// - class only in `head`  -> `Added`
/// - class only in `base`  -> `Removed` (rendered from its base-revision shape)
/// - in both, same shape   -> `Unchanged`
/// - in both, shape differs-> `Modified` (members carry their own status)
pub fn diff_graphs(base: &CodeGraph, head: &CodeGraph) -> CodeGraph {
    let base_by_key: HashMap<&str, &Class> = base
        .classes
        .iter()
        .map(|c| (c.qualified.as_str(), c))
        .collect();
    let head_keys: HashSet<&str> = head.classes.iter().map(|c| c.qualified.as_str()).collect();

    let mut merged = CodeGraph::default();

    for class in &head.classes {
        let mut class = class.clone();
        match base_by_key.get(class.qualified.as_str()) {
            None => {
                class.change = ChangeKind::Added;
                for m in &mut class.members {
                    m.change = ChangeKind::Added;
                }
            }
            Some(old) if old.fingerprint() == class.fingerprint() => {
                class.change = ChangeKind::Unchanged;
            }
            Some(old) => {
                class.change = ChangeKind::Modified;
                annotate_members(&mut class, old);
            }
        }
        merged.classes.push(class);
    }

    for class in &base.classes {
        if !head_keys.contains(class.qualified.as_str()) {
            let mut class = class.clone();
            class.change = ChangeKind::Removed;
            for m in &mut class.members {
                m.change = ChangeKind::Removed;
            }
            merged.classes.push(class);
        }
    }

    merged.normalize();
    merged
}

/// Mark added/modified members on a modified class, and append members that
/// were removed since `old` so the diagram can show them explicitly.
fn annotate_members(class: &mut Class, old: &Class) {
    let old_fingerprints: HashSet<String> = old.members.iter().map(|m| m.fingerprint()).collect();
    let old_names: HashSet<&str> = old.members.iter().map(|m| m.name.as_str()).collect();
    let head_names: HashSet<String> = class.members.iter().map(|m| m.name.clone()).collect();

    for member in &mut class.members {
        member.change = if old_fingerprints.contains(&member.fingerprint()) {
            ChangeKind::Unchanged
        } else if old_names.contains(member.name.as_str()) {
            ChangeKind::Modified
        } else {
            ChangeKind::Added
        };
    }
    for old_member in &old.members {
        if !head_names.contains(&old_member.name) {
            let mut removed = old_member.clone();
            removed.change = ChangeKind::Removed;
            class.members.push(removed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_file;

    fn graph(src: &str) -> CodeGraph {
        parse_file("m.py", src).unwrap()
    }

    #[test]
    fn classifies_added_removed_modified() {
        let base = graph("class Kept:\n    def a(self): ...\nclass Gone:\n    pass\nclass Changed:\n    def old(self): ...\n");
        let head = graph("class Kept:\n    def a(self): ...\nclass New:\n    pass\nclass Changed:\n    def new(self): ...\n");
        let merged = diff_graphs(&base, &head);
        let change = |n: &str| merged.classes.iter().find(|c| c.name == n).unwrap().change;
        assert_eq!(change("Kept"), ChangeKind::Unchanged);
        assert_eq!(change("Gone"), ChangeKind::Removed);
        assert_eq!(change("New"), ChangeKind::Added);
        assert_eq!(change("Changed"), ChangeKind::Modified);

        let changed = merged.classes.iter().find(|c| c.name == "Changed").unwrap();
        let member = |n: &str| changed.members.iter().find(|m| m.name == n).unwrap();
        assert_eq!(member("new").change, ChangeKind::Added);
        assert_eq!(member("old").change, ChangeKind::Removed);
    }
}
