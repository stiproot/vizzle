//! Render a [`CodeGraph`] as a Mermaid `classDiagram`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use crate::model::{ChangeKind, Class, CodeGraph, Member, RelationKind};
use crate::resolve::{resolve_relations, Target};

#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Render fields and methods inside each class box.
    pub show_members: bool,
    /// Group classes into `namespace` blocks per module.
    pub group_by_module: bool,
    /// Emit inheritance edges to types that were not found in the parsed set
    /// (mermaid will auto-create empty nodes for them).
    pub include_externals: bool,
    /// Mermaid layout direction (TB, LR, ...).
    pub direction: Option<String>,
    pub title: Option<String>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            show_members: true,
            group_by_module: false,
            include_externals: false,
            direction: None,
            title: None,
        }
    }
}

/// Marker appended to member rows in diff mode.
fn change_marker(change: ChangeKind) -> &'static str {
    match change {
        ChangeKind::Added => " ✚",
        ChangeKind::Removed => " ✖",
        ChangeKind::Modified => " ✱",
        ChangeKind::Unchanged => "",
    }
}

pub(crate) fn sanitize_id(qualified: &str) -> String {
    let mut id: String = qualified
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if id.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        id.insert(0, '_');
    }
    id
}

pub(crate) fn escape_label(label: &str) -> String {
    label.replace('"', "'")
}

pub fn render(graph: &CodeGraph, opts: &RenderOptions) -> String {
    let mut out = String::new();
    if let Some(title) = &opts.title {
        let _ = writeln!(out, "---\ntitle: {}\n---", escape_label(title));
    }
    out.push_str("classDiagram\n");
    if let Some(direction) = &opts.direction {
        let _ = writeln!(out, "    direction {direction}");
    }

    // Unique mermaid id per class.
    let mut ids: HashMap<&str, String> = HashMap::new();
    let mut used: HashMap<String, usize> = HashMap::new();
    for class in &graph.classes {
        let mut id = sanitize_id(&class.qualified);
        let n = used.entry(id.clone()).or_insert(0);
        if *n > 0 {
            id = format!("{id}_{n}");
        }
        *n += 1;
        ids.insert(&class.qualified, id);
    }

    // Class declarations, optionally grouped into namespaces.
    let mut groups: BTreeMap<String, Vec<&Class>> = BTreeMap::new();
    for class in &graph.classes {
        let key = if opts.group_by_module {
            class.module.clone()
        } else {
            String::new()
        };
        groups.entry(key).or_default().push(class);
    }

    let diff_mode = graph
        .classes
        .iter()
        .any(|c| c.change != ChangeKind::Unchanged);

    for (module, classes) in &groups {
        let (indent, in_namespace) = if opts.group_by_module {
            let _ = writeln!(out, "    namespace {} {{", sanitize_id(module));
            ("        ", true)
        } else {
            ("    ", false)
        };
        for class in classes {
            write_class(&mut out, class, &ids, opts, diff_mode, indent);
        }
        if in_namespace {
            out.push_str("    }\n");
        }
    }

    // Relations (must live outside namespace blocks).
    let mut externals: Vec<(String, String)> = Vec::new();
    for relation in resolve_relations(graph) {
        let from_id = &ids[relation.from.as_str()];
        let to_id = match &relation.to {
            Target::Internal(qualified) => ids[qualified.as_str()].clone(),
            Target::External(name) if opts.include_externals => {
                let ext_id = format!("ext_{}", sanitize_id(name));
                externals.push((ext_id.clone(), name.clone()));
                ext_id
            }
            Target::External(_) => continue,
        };
        let arrow = match relation.kind {
            RelationKind::Inherits => "--|>",
            RelationKind::Implements => "..|>",
        };
        let _ = writeln!(out, "    {from_id} {arrow} {to_id}");
    }

    externals.sort();
    externals.dedup();
    for (ext_id, label) in &externals {
        let _ = writeln!(out, "    class {ext_id}[\"{}\"]", escape_label(label));
        let _ = writeln!(out, "    <<external>> {ext_id}");
    }

    // Change styling (GitHub-diff palette). Note: mermaid 11 only applies
    // classDef styles in classDiagrams when the classDef statements appear
    // AFTER the cssClass attachments, so these are emitted last.
    if diff_mode {
        for (change, css) in [
            (ChangeKind::Added, "vizzyAdded"),
            (ChangeKind::Removed, "vizzyRemoved"),
            (ChangeKind::Modified, "vizzyModified"),
        ] {
            let members: Vec<&str> = graph
                .classes
                .iter()
                .filter(|c| c.change == change)
                .map(|c| ids[c.qualified.as_str()].as_str())
                .collect();
            if !members.is_empty() {
                let _ = writeln!(out, "    cssClass \"{}\" {css}", members.join(","));
            }
        }
        out.push_str(concat!(
            "    classDef vizzyAdded fill:#e6ffec,stroke:#1a7f37,stroke-width:2px,color:#1a7f37\n",
            "    classDef vizzyRemoved fill:#ffebe9,stroke:#cf222e,stroke-width:2px,stroke-dasharray:6 4,color:#cf222e\n",
            "    classDef vizzyModified fill:#fff8c5,stroke:#9a6700,stroke-width:2px,color:#9a6700\n",
        ));
    }

    let relation_count = graph.classes.iter().map(|c| c.bases.len()).sum::<usize>();
    let _ = writeln!(
        out,
        "%% vizzy: {} classes, {} relations",
        graph.classes.len(),
        relation_count
    );
    out
}

fn write_class(
    out: &mut String,
    class: &Class,
    ids: &HashMap<&str, String>,
    opts: &RenderOptions,
    diff_mode: bool,
    indent: &str,
) {
    let id = &ids[class.qualified.as_str()];
    let label = if opts.group_by_module {
        class.name.clone()
    } else {
        class.qualified.clone()
    };
    let mut label = escape_label(&label);
    if diff_mode {
        match class.change {
            ChangeKind::Added => label.push_str(" ✚"),
            ChangeKind::Removed => label.push_str(" ✖"),
            ChangeKind::Modified => label.push_str(" ✱"),
            ChangeKind::Unchanged => {}
        }
    }

    let has_body = class.annotation.is_some() || (opts.show_members && !class.members.is_empty());
    if !has_body {
        let _ = writeln!(out, "{indent}class {id}[\"{label}\"]");
        return;
    }

    let _ = writeln!(out, "{indent}class {id}[\"{label}\"] {{");
    if let Some(annotation) = &class.annotation {
        let _ = writeln!(out, "{indent}    <<{annotation}>>");
    }
    if opts.show_members {
        for member in &class.members {
            let _ = writeln!(out, "{indent}    {}", member_row(member, diff_mode));
        }
    }
    let _ = writeln!(out, "{indent}}}");
}

fn member_row(member: &Member, diff_mode: bool) -> String {
    let vis = member.visibility.sigil();
    let classifier = if member.is_abstract {
        "*"
    } else if member.is_static {
        "$"
    } else {
        ""
    };
    let marker = if diff_mode {
        change_marker(member.change)
    } else {
        ""
    };
    let row = if member.is_method {
        let returns = member
            .returns
            .as_deref()
            .map(|r| format!(" {r}"))
            .unwrap_or_default();
        format!(
            "{vis}{}({}){classifier}{returns}",
            member.name, member.detail
        )
    } else if member.detail.is_empty() {
        format!("{vis}{}{classifier}", member.name)
    } else {
        format!("{vis}{} : {}{classifier}", member.name, member.detail)
    };
    format!("{row}{marker}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_file;

    #[test]
    fn renders_basic_diagram() {
        let graph = parse_file(
            "pkg/mod.py",
            "class Base:\n    def run(self) -> int: ...\n\nclass Child(Base):\n    name: str\n",
        )
        .unwrap();
        let out = render(&graph, &RenderOptions::default());
        assert!(out.starts_with("classDiagram"));
        assert!(out.contains("class pkg_mod_Child[\"pkg.mod.Child\"] {"));
        assert!(out.contains("+name : str"));
        assert!(out.contains("+run() int"));
        assert!(out.contains("pkg_mod_Child --|> pkg_mod_Base"));
        assert!(!out.contains("cssClass"));
    }

    #[test]
    fn renders_diff_styling() {
        let base = parse_file("m.py", "class A:\n    pass\n").unwrap();
        let head = parse_file("m.py", "class A:\n    pass\nclass B:\n    pass\n").unwrap();
        let merged = crate::diff::diff_graphs(&base, &head);
        let out = render(&merged, &RenderOptions::default());
        assert!(out.contains("classDef vizzyAdded"));
        assert!(out.contains("cssClass \"m_B\" vizzyAdded"));
    }
}
