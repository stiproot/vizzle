//! Render a [`CodeGraph`] as a Mermaid `classDiagram`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use crate::model::{ChangeKind, Class, CodeGraph, Member};
use crate::palette;
use crate::resolve::{resolve_all_relations, Target};

/// How class boxes are gathered into `namespace` blocks.
///
/// `Component` needs a lookup the renderer cannot derive from a [`CodeGraph`]
/// alone — component ownership comes from package manifests on disk — so the
/// caller supplies it in [`RenderOptions::component_of`]. `diagram_from_dir`
/// fills it; a caller rendering from in-memory files that leaves it empty gets
/// ungrouped output rather than a wrong grouping.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Grouping {
    /// One flat list of classes.
    #[default]
    None,
    /// One namespace per module (for Python, per file).
    Module,
    /// One namespace per detected component (the package a class belongs to).
    Component,
}

impl Grouping {
    /// Parse the CLI spelling. Unknown values are an error, not a silent default.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "none" => Ok(Self::None),
            "module" => Ok(Self::Module),
            "component" => Ok(Self::Component),
            other => Err(format!(
                "unknown grouping `{other}`; expected none, module or component"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Render fields and methods inside each class box.
    pub show_members: bool,
    /// Include the `<<module>>` boxes holding module-level functions
    /// (class.md §2.4). Off by default: on h they add 124 boxes to 213.
    pub show_modules: bool,
    /// How classes are gathered into `namespace` blocks.
    pub grouping: Grouping,
    /// Qualified class name to owning component name, for [`Grouping::Component`].
    /// Empty for every other grouping.
    pub component_of: HashMap<String, String>,
    /// In diff mode, draw only the members that changed and one row saying
    /// how many did not (component.md §6.4). A 300-member class with two
    /// changed methods is otherwise 300 rows of context around two signals.
    pub changed_members_only: bool,
    /// The reader's lens (class.md §7b): qualified names to light; every other
    /// class is drawn as context. `None` means no lens.
    pub highlight: Option<std::collections::BTreeSet<String>>,
    /// Emit inheritance edges to types that were not found in the parsed set
    /// (mermaid will auto-create empty nodes for them).
    pub include_externals: bool,
    /// Say which file each class lives in (component.md §6.4): the file's
    /// base name shares the stereotype line (`<<enumeration · run.py>>`, or
    /// `<<worker.py>>` for a plain class), and a module box is titled by its
    /// file. For a diagram grouped by component, where several files sit in
    /// one namespace and a reader cannot otherwise tell which class came
    /// from which.
    pub show_files: bool,
    /// How much of a signature each member row carries. Typed is the
    /// exhaustive default; names-only is for a diagram people read rather
    /// than mine, where a twelve-parameter typed signature is a wall of text
    /// (curated-diagrams.md §5.1, component.md §6.4).
    pub params: Params,
    /// Mermaid layout direction (TB, LR, ...).
    pub direction: Option<String>,
    pub title: Option<String>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            show_members: true,
            show_modules: false,
            show_files: false,
            params: Params::Typed,
            grouping: Grouping::default(),
            component_of: HashMap::new(),
            changed_members_only: false,
            highlight: None,
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

/// A label is data, and mermaid reads it as syntax the moment it can: a quote
/// closes the label, a newline (legal in a manifest `name`) ends the statement
/// and starts another — a `click` or a `%%{init}%%` directive, say — and
/// `<`, `>`, `{`, `}` are markup or structure. So quotes become apostrophes,
/// control characters spaces, and the four are entity-encoded, which mermaid
/// decodes back for display in every diagram type.
pub(crate) fn escape_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    for c in label.chars() {
        match c {
            '"' => out.push('\''),
            '<' => out.push_str("#lt;"),
            '>' => out.push_str("#gt;"),
            '{' => out.push_str("#123;"),
            '}' => out.push_str("#125;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A front-matter title is one YAML scalar. Unquoted, a newline would start a
/// second key and a `: ` would make the line a mapping (mermaid then rejects
/// the whole diagram); double-quoted, only `"` and `\` need escaping and every
/// other character is literal. Entities are not decoded here, so this is the
/// only treatment a title gets.
pub(crate) fn frontmatter_title(title: &str) -> String {
    let flat: Vec<&str> = title.split(|c: char| c.is_control()).collect();
    format!(
        "\"{}\"",
        flat.join(" ").replace('\\', "\\\\").replace('"', "\\\"")
    )
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
        let _ = writeln!(out, "---\ntitle: {}\n---", frontmatter_title(title));
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
        let key = match opts.grouping {
            Grouping::None => String::new(),
            Grouping::Module => class.module.clone(),
            Grouping::Component => opts
                .component_of
                .get(&class.qualified)
                .cloned()
                .unwrap_or_default(),
        };
        groups.entry(key).or_default().push(class);
    }

    let diff_mode = graph.diff_mode();

    for (group, classes) in &groups {
        // Empty key: ungrouped, either because grouping is off or because this
        // class has no component. Mermaid rejects an unnamed namespace anyway.
        let (indent, in_namespace) = if group.is_empty() {
            ("    ", false)
        } else {
            let _ = writeln!(out, "    namespace {} {{", sanitize_id(group));
            ("        ", true)
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
    if let Some(lit) = &opts.highlight {
        out.push_str(&palette::mermaid_lens_classdefs());
        let _ = writeln!(
            out,
            "%% vizzle: highlight: {} of {} classes",
            lit.len(),
            graph.classes.len()
        );
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
    // An in-memory graph has no file to name; the option is then a no-op
    // rather than an empty stereotype.
    let file = (opts.show_files && !class.file.is_empty()).then(|| class.file_name());
    let label = match file {
        Some(file) if class.is_module_box() => file.to_owned(),
        _ if opts.grouping == Grouping::None => class.qualified.clone(),
        _ => class.name.clone(),
    };
    let mut label = escape_label(&label);
    if diff_mode {
        label.push_str(class.change.glyph());
    }

    // The lens rides the class line itself (`:::name`): measured to render
    // where a detached `cssClass` line did not for one consumer, and it
    // cannot be separated from the line it styles.
    let lens = match &opts.highlight {
        Some(lit) if lit.contains(&class.qualified) => format!(":::{}", palette::MERMAID_HIGHLIGHT),
        Some(_) => format!(":::{}", palette::MERMAID_CONTEXT),
        None => String::new(),
    };
    // Mermaid draws one stereotype line per class, so the file shares it with
    // the UML stereotype when the class has one. A module box is already
    // titled by its file, and keeps `<<module>>` alone.
    let annotation = match (&class.annotation, file) {
        (Some(_), Some(_)) if class.is_module_box() => class.annotation.clone(),
        (Some(stereotype), Some(file)) => Some(format!("{stereotype} · {file}")),
        (None, Some(file)) => Some(file.to_owned()),
        (stereotype, None) => stereotype.clone(),
    };
    let has_body =
        annotation.is_some() || (opts.show_members && class.drawn_members().next().is_some());
    if !has_body {
        let _ = writeln!(out, "{indent}class {id}[\"{label}\"]{lens}");
        return;
    }

    let _ = writeln!(out, "{indent}class {id}[\"{label}\"]{lens} {{");
    if let Some(annotation) = &annotation {
        let _ = writeln!(out, "{indent}    <<{annotation}>>");
    }
    if opts.show_members {
        let changed_only = opts.changed_members_only && diff_mode;
        let mut hidden = 0usize;
        for member in class.drawn_members() {
            if changed_only && member.change == ChangeKind::Unchanged {
                hidden += 1;
                continue;
            }
            let _ = writeln!(
                out,
                "{indent}    {}",
                member_row(member, diff_mode, opts.params)
            );
        }
        if hidden > 0 {
            // A plain attribute row: no visibility sigil, no parens, so mermaid
            // reads it as a field and draws it as text. The reader learns that
            // the box is a window onto a larger class, not the whole of it.
            let noun = if hidden == 1 { "member" } else { "members" };
            let _ = writeln!(out, "{indent}    … {hidden} unchanged {noun}");
        }
    }
    let _ = writeln!(out, "{indent}}}");
}

/// How much of a signature a member line carries. An exhaustive diagram wants
/// the types; a curated one is read by people and wants the shape
/// (curated-diagrams.md §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Params {
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
/// - `<` and `>` are generic syntax (`~T~` once `clean_type` has been through a
///   type) and end the diagram anywhere else — a TypeScript member can be
///   named by a string literal holding either. They are entity-encoded, which
///   mermaid decodes back for display.
fn mermaid_safe(row: &str) -> String {
    let row: String = row
        .chars()
        .filter(|c| !matches!(c, '{' | '}'))
        .collect::<String>()
        .replace('<', "#lt;")
        .replace('>', "#gt;");
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
    fn a_label_cannot_leave_its_statement() {
        let hostile =
            "evil\n    click c_pkg \"https://x\" \n%%{init: {\"theme\":\"forest\"}}%%\n<b>x</b>";
        let label = escape_label(hostile);
        assert!(!label.contains('\n'));
        assert!(!label.contains('"'));
        assert!(!label.contains('{') && !label.contains('<'));
        assert_eq!(escape_label("a<T>"), "a#lt;T#gt;");
    }

    #[test]
    fn a_title_stays_on_its_line() {
        assert_eq!(
            frontmatter_title("x\nconfig:\n  theme: forest"),
            "\"x config:   theme: forest\""
        );
        assert_eq!(
            frontmatter_title(r#"say "hi" \ bye"#),
            r#""say \"hi\" \\ bye""#
        );
    }

    #[test]
    fn a_member_named_with_angle_brackets_does_not_end_the_diagram() {
        assert_eq!(
            mermaid_safe("+\"</script>\"() void"),
            "+\"#lt;/script#gt;\"() void"
        );
    }

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

    /// Two classes in two modules, the minimum to tell groupings apart.
    fn two_class_graph() -> CodeGraph {
        crate::parse::parse_files(&[
            ("a.py".to_owned(), "class Alpha:\n    pass\n".to_owned()),
            ("b.py".to_owned(), "class Beta:\n    pass\n".to_owned()),
        ])
        .unwrap()
    }

    #[test]
    fn grouping_parses_its_three_spellings() {
        assert_eq!(Grouping::parse("none").unwrap(), Grouping::None);
        assert_eq!(Grouping::parse("module").unwrap(), Grouping::Module);
        assert_eq!(Grouping::parse("component").unwrap(), Grouping::Component);
        // An unknown spelling is an error, never a silent fallback to None:
        // a typo that quietly dropped the grouping would be hard to notice.
        assert!(Grouping::parse("Module").is_err());
        assert!(Grouping::parse("package").is_err());
    }

    #[test]
    fn component_grouping_uses_the_supplied_map() {
        let graph = two_class_graph();
        let mut opts = RenderOptions {
            grouping: Grouping::Component,
            ..Default::default()
        };
        opts.component_of
            .insert("a.Alpha".to_owned(), "core".to_owned());
        opts.component_of
            .insert("b.Beta".to_owned(), "core".to_owned());
        let out = render(&graph, &opts);
        assert_eq!(out.matches("namespace ").count(), 1, "{out}");
        assert!(out.contains("namespace core {"), "{out}");
    }

    #[test]
    fn component_grouping_without_a_map_leaves_classes_ungrouped() {
        // diagram_from_files has no tree to detect components in. Ungrouped
        // output is honest; a namespace named after nothing would not be.
        let graph = two_class_graph();
        let out = render(
            &graph,
            &RenderOptions {
                grouping: Grouping::Component,
                ..Default::default()
            },
        );
        assert!(!out.contains("namespace"), "{out}");
    }

    #[test]
    fn a_class_outside_the_map_is_not_forced_into_a_namespace() {
        let graph = two_class_graph();
        let mut opts = RenderOptions {
            grouping: Grouping::Component,
            ..Default::default()
        };
        opts.component_of
            .insert("a.Alpha".to_owned(), "core".to_owned());
        let out = render(&graph, &opts);
        assert_eq!(out.matches("namespace ").count(), 1, "{out}");
        // Beta is still drawn, just outside the block.
        assert!(out.contains("Beta"), "{out}");
    }

    #[test]
    fn module_grouping_still_groups_per_module() {
        let graph = two_class_graph();
        let out = render(
            &graph,
            &RenderOptions {
                grouping: Grouping::Module,
                ..Default::default()
            },
        );
        assert_eq!(out.matches("namespace ").count(), 2, "{out}");
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
        assert!(out.contains("classDef diffAdded"));
        assert!(out.contains("cssClass \"m_B\" diffAdded"));
    }

    #[test]
    fn show_files_puts_the_file_on_the_stereotype_line() {
        let graph = crate::parse::parse_files(&[(
            "pkg/run.py".to_owned(),
            "from enum import Enum\nclass FailureClass(Enum):\n    A = 1\nclass Plain:\n    x: int\nLIMIT = 3\n".to_owned(),
        )])
        .unwrap();
        let out = render(
            &graph,
            &RenderOptions {
                show_modules: true,
                show_files: true,
                ..Default::default()
            },
        );
        // A stereotype shares the line with the file; a plain class gets the
        // file alone; a module box is titled by its file and keeps <<module>>.
        assert!(out.contains("<<enumeration · run.py>>"), "{out}");
        assert!(out.contains("<<run.py>>"), "{out}");
        assert!(out.contains("class pkg_run[\"run.py\"] {"), "{out}");
        assert!(out.contains("<<module>>"), "{out}");
        // Off by default, so every existing diagram is unchanged.
        let plain = render(
            &graph,
            &RenderOptions {
                show_modules: true,
                ..Default::default()
            },
        );
        assert!(!plain.contains("run.py"), "{plain}");
    }

    #[test]
    fn names_only_params_drop_the_types_but_keep_the_return() {
        let graph = parse_file(
            "m.py",
            "class A:\n    def go(self, issue: IssueRecord, loud: bool = False) -> str: ...\n",
        )
        .unwrap();
        let out = render(
            &graph,
            &RenderOptions {
                params: Params::NamesOnly,
                ..Default::default()
            },
        );
        assert!(out.contains("+go(issue, loud) str"), "{out}");
    }

    #[test]
    fn an_unchanged_private_module_function_is_not_drawn() {
        let graph = parse_file("m.py", "def _helper(): ...\ndef api(): ...\n").unwrap();
        let out = render(
            &graph,
            &RenderOptions {
                show_modules: true,
                ..Default::default()
            },
        );
        assert!(out.contains("+api()"), "{out}");
        assert!(
            !out.contains("_helper"),
            "not module surface (class.md §2.5): {out}"
        );
    }
}
