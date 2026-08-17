//! Render a [`CodeGraph`] as a Mermaid `classDiagram`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use crate::model::{ChangeKind, Class, CodeGraph, Member};
use crate::palette;
use crate::resolve::{resolve_all_relations, Target};

#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Render fields and methods inside each class box.
    pub show_members: bool,
    /// Include the `<<module>>` boxes holding module-level functions
    /// (class.md §2.4). Off by default: on h they add 124 boxes to 213.
    pub show_modules: bool,
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
            show_modules: false,
            group_by_module: false,
            include_externals: false,
            direction: None,
            title: None,
        }
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
    // Filter once, here, so every loop below is unaware of the option.
    let pruned;
    let graph = if opts.show_modules {
        graph
    } else {
        pruned = graph.without_module_boxes();
        &pruned
    };

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

    let diff_mode = graph.diff_mode();

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

    // Relations (must live outside namespace blocks): inheritance plus the
    // associations and dependencies implied by member types.
    let mut externals: Vec<(String, String)> = Vec::new();
    let relations = resolve_all_relations(graph);
    let relation_count = relations.len();
    for relation in relations {
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
        let _ = writeln!(out, "    {from_id} {} {to_id}", relation.kind.arrow());
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
        for change in [ChangeKind::Added, ChangeKind::Removed, ChangeKind::Modified] {
            let members: Vec<&str> = graph
                .classes
                .iter()
                .filter(|c| c.change == change)
                .map(|c| ids[c.qualified.as_str()].as_str())
                .collect();
            if let (false, Some(css)) = (members.is_empty(), palette::mermaid_class(change)) {
                let _ = writeln!(out, "    cssClass \"{}\" {css}", members.join(","));
            }
        }
        out.push_str(&palette::mermaid_classdefs());
    }

    let _ = writeln!(
        out,
        "%% vizzle: {} classes, {} relations",
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
        label.push_str(class.change.glyph());
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
            let _ = writeln!(
                out,
                "{indent}    {}",
                member_row(member, diff_mode, Params::Typed)
            );
        }
    }
    let _ = writeln!(out, "{indent}}}");
}

/// How much of a signature a member line carries. An exhaustive diagram wants
/// the types; a curated one is read by people and wants the shape
/// (curated-diagrams.md §5.1).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Params {
    Typed,
    NamesOnly,
}

pub(crate) fn member_row(member: &Member, diff_mode: bool, params: Params) -> String {
    let vis = member.visibility.sigil();
    let classifier = if member.is_abstract {
        "*"
    } else if member.is_static {
        "$"
    } else {
        ""
    };
    let marker = if diff_mode { member.change.glyph() } else { "" };
    let row = if member.is_method {
        let returns = member
            .returns
            .as_deref()
            .map(|r| format!(" {r}"))
            .unwrap_or_default();
        let signature = match params {
            Params::Typed => member.detail.clone(),
            Params::NamesOnly => member.param_names.join(", "),
        };
        format!("{vis}{}({}){classifier}{returns}", member.name, signature)
    } else if member.detail.is_empty() {
        format!("{vis}{}{classifier}", member.name)
    } else {
        format!("{vis}{} : {}{classifier}", member.name, member.detail)
    };
    format!("{}{marker}", mermaid_safe(&one_line(&row)))
}

/// A member is one line by definition. A destructured parameter carries the
/// source's newlines and indentation into the signature, which mermaid reads as
/// the end of the member and the start of nonsense.
fn one_line(row: &str) -> String {
    row.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The last thing every member line passes through, because mermaid fails the
/// *whole diagram* on a malformed one. Two distinct hazards:
///
/// - A brace is structural inside a class body — mermaid reads `{` as
///   OPEN_IN_STRUCT and stops, however balanced it is. A destructured parameter
///   (`buildInvocationResult({ events, stderr }: Opts)`) puts one there.
/// - Parens must balance, which a truncated parameter list can break.
fn mermaid_safe(row: &str) -> String {
    let row: String = row.chars().filter(|c| !matches!(c, '{' | '}')).collect();
    if row.matches('(').count() == row.matches(')').count() {
        return row;
    }
    row.chars().filter(|c| !matches!(c, '(' | ')')).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_file;

    #[test]
    fn a_member_is_always_one_line_and_keeps_its_arrows() {
        let graph = crate::parse::parse_file(
            "web/src/app.ts",
            "export interface I {\n  handler: (a: string) => void;\n  go({\n    a,\n    b,\n  }: Opts): void;\n}\n",
        )
        .unwrap();
        let out = render(&graph, &RenderOptions::default());
        for line in out.lines().filter(|l| l.trim_start().starts_with('+')) {
            assert!(
                !line.contains("=~"),
                "`=>` must survive the generic rewrite: {line}"
            );
        }
        // A destructured parameter must not spill the source's newlines.
        let members: Vec<&str> = out
            .lines()
            .filter(|l| l.trim_start().starts_with('+'))
            .collect();
        assert_eq!(members.len(), 2, "one line per member, got:\n{out}");
    }

    #[test]
    fn member_rows_never_leave_a_delimiter_unbalanced() {
        let graph = crate::parse::parse_file(
            "web/src/app.ts",
            "export function buildInvocationResult({ a, b }: Opts, r: Map<string, number>): void {}\n",
        )
        .unwrap();
        let out = render(
            &graph,
            &RenderOptions {
                show_modules: true,
                ..Default::default()
            },
        );
        for line in out.lines().filter(|l| l.trim_start().starts_with('+')) {
            assert_eq!(
                line.matches('(').count(),
                line.matches(')').count(),
                "parens: {line}"
            );
            assert_eq!(
                line.matches('{').count(),
                line.matches('}').count(),
                "braces: {line}"
            );
            assert!(
                !line.contains('<'),
                "raw angle bracket kills mermaid: {line}"
            );
        }
    }

    #[test]
    fn module_boxes_are_opt_in() {
        let graph = crate::parse::parse_file(
            "web/src/app.ts",
            "export function parse(x: string): void {}\nexport class Real {}\n",
        )
        .unwrap();

        let hidden = render(&graph, &RenderOptions::default());
        assert!(hidden.contains("Real"), "real classes always render");
        assert!(
            !hidden.contains("module"),
            "module box hidden by default:\n{hidden}"
        );

        let shown = render(
            &graph,
            &RenderOptions {
                show_modules: true,
                ..Default::default()
            },
        );
        assert!(
            shown.contains("<<module>>"),
            "shown under show_modules:\n{shown}"
        );
        assert!(shown.contains("parse"));
    }

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
        assert!(out.contains("classDef vizzleAdded"));
        assert!(out.contains("cssClass \"m_B\" vizzleAdded"));
    }
}
