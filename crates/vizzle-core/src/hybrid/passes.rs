//! The module layer: one vizzle pass, read as the module graph a picture
//! reconciles against.
//!
//! The generator this replaces ran `vizzle component` and parsed its mermaid
//! with regexes. A pass now arrives as the component graph JSON
//! (`component::to_json`, the atoms' contract), and only the two facts a
//! picture needs are kept: which modules exist and which imports which.

use std::collections::BTreeMap;
use std::fmt;

use anyhow::{bail, Context, Result};
use serde::de::{Error as _, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::spec;

/// What a picture knows of one pass: module ids, sorted, and the import edges
/// between them (externals dropped, as the mermaid never drew them).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleGraph {
    pub modules: Vec<String>,
    pub edges: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct WireGraph {
    components: Vec<WireComponent>,
    #[serde(default)]
    edges: Vec<WireEdge>,
}

#[derive(Deserialize)]
struct WireComponent {
    name: String,
    path: String,
    #[serde(default)]
    group: String,
}

#[derive(Deserialize)]
struct WireEdge {
    from: String,
    to: String,
    #[serde(default)]
    external: bool,
}

/// A module's id as spec refs name it: `<group>/<leaf of its name>`, or the
/// leaf alone at the top level. This is what the mermaid parse produced (the
/// subgraph label, then the bold node label's last segment), so every ref
/// written against it still resolves. It is not always the module's path: a
/// component named by its manifest (`@scope/pkg`) keeps the manifest's leaf.
pub fn module_id(group: &str, name: &str) -> String {
    let leaf = name.rsplit('/').next().unwrap_or(name);
    if group.is_empty() {
        leaf.to_owned()
    } else {
        format!("{group}/{leaf}")
    }
}

/// Read a pass's component graph JSON into its module graph.
pub fn module_graph(component_json: &str) -> Result<ModuleGraph> {
    let wire: WireGraph = serde_json::from_str(component_json)
        .context("a pass's output is not component graph JSON")?;
    let ids: std::collections::HashMap<&str, String> = wire
        .components
        .iter()
        .map(|c| (c.path.as_str(), module_id(&c.group, &c.name)))
        .collect();
    let mut modules: Vec<String> = ids.values().cloned().collect();
    modules.sort();
    let edges = wire
        .edges
        .iter()
        .filter(|e| !e.external)
        .filter_map(|e| {
            Some((
                ids.get(e.from.as_str())?.clone(),
                ids.get(e.to.as_str())?.clone(),
            ))
        })
        .collect();
    Ok(ModuleGraph { modules, edges })
}

/// The pass table (`vizzle-passes.json`), read: each pass's `vizzle component`
/// arguments, and the notes the CLI prints. The arguments stay strings here:
/// they are parsed by `component`'s own option parser in the CLI
/// (`docs/plans/impl/hybrid.md` B2), so there is one definition of what a pass
/// may say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassTable {
    pub passes: BTreeMap<String, Vec<String>>,
    pub notes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTable {
    #[serde(rename = "$comment", default)]
    _comment: Option<Value>,
    /// The version pin h's table carried; accepted, ignored, noted.
    #[serde(default)]
    vizzle: Option<Value>,
    #[serde(default, deserialize_with = "unique_passes")]
    passes: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEntry {
    #[serde(rename = "$comment", default)]
    _comment: Option<Value>,
    args: Vec<String>,
}

/// `passes`, refusing a name given twice (serde's map keeps the last one
/// silently) and wording a bad entry with the pass's name, which serde's own
/// message would drop.
fn unique_passes<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<String, Vec<String>>, D::Error> {
    struct Passes;
    impl<'de> Visitor<'de> for Passes {
        type Value = BTreeMap<String, Vec<String>>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an object of pass name → {\"args\": [strings]}")
        }
        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some(name) = map.next_key::<String>()? {
                // Read straight into the entry, never through `Value`, whose map
                // would keep only the last of a repeated `args`.
                let entry = map.next_value::<WireEntry>().map_err(|_| {
                    A::Error::custom(format!(r#"pass {name:?} must be {{"args": [strings]}}"#))
                })?;
                if out.contains_key(&name) {
                    return Err(A::Error::custom(format!("pass {name:?} is defined twice")));
                }
                out.insert(name, entry.args);
            }
            Ok(out)
        }
    }
    d.deserialize_map(Passes)
}

/// Read a pass table's text. Every refusal is "could not run" (exit 2).
pub fn pass_table(json: &str) -> Result<PassTable> {
    // Checked on the text, before serde: a derived struct also accepts a JSON
    // array, read field by field, which would take `[]` for an empty table.
    // (Going through `Value` instead would lose a repeated pass name: its map
    // keeps the last.)
    if !json.trim_start().starts_with('{') {
        bail!(r#"not a pass table: expected an object {{"passes": {{name: {{"args": [...]}}}}}}"#);
    }
    let wire: WireTable = serde_json::from_str(json).context("not a pass table")?;
    let notes = wire
        .vizzle
        .map(|pin| {
            let shown = match pin {
                Value::String(s) => s,
                other => other.to_string(),
            };
            format!(
                "pins vizzle {shown}; the pin is ignored (the passes run in this vizzle) \
                 and will be refused in a later minor — drop it"
            )
        })
        .into_iter()
        .collect();
    Ok(PassTable {
        passes: wire.passes,
        notes,
    })
}

/// The passes a spec reconciles against, in the order it names them, each
/// defined in `table`. Read with the spec parser `build` uses, so a spec that
/// `build` would refuse is refused here, before any pass runs.
pub fn spec_passes(spec_json: &str, table: &PassTable) -> Result<Vec<String>> {
    let spec = spec::parse(spec_json)?;
    if let Some(missing) = spec.vizzle.iter().find(|p| !table.passes.contains_key(*p)) {
        bail!("{}", undefined_pass(missing));
    }
    Ok(spec.vizzle)
}

/// The one wording for a spec naming a pass the table lacks. The core does
/// not name the file: its name is the CLI's.
pub(crate) fn undefined_pass(name: &str) -> String {
    format!("spec names vizzle pass {name:?}, which the pass table does not define")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(json: &str) -> String {
        format!("{:#}", pass_table(json).unwrap_err())
    }

    #[test]
    fn reads_a_pass_table_with_comments_at_both_levels() {
        let t = pass_table(
            r#"{"$comment": "x", "passes": {"b": {"$comment": "y", "args": ["-I", "b/**"]}, "a": {"args": []}}}"#,
        )
        .unwrap();
        assert_eq!(t.passes["b"], vec!["-I", "b/**"]);
        assert!(t.passes["a"].is_empty());
        assert!(t.notes.is_empty());
    }

    #[test]
    fn an_empty_object_is_an_empty_table() {
        assert_eq!(pass_table("{}").unwrap(), PassTable::default());
    }

    #[test]
    fn refuses_a_table_that_is_not_an_object() {
        assert!(refusal("[]").contains("expected an object"));
        assert!(refusal(r#""passes""#).contains("expected an object"));
        assert!(refusal("{").starts_with("not a pass table"));
    }

    #[test]
    fn refuses_passes_that_is_not_an_object() {
        assert!(refusal(r#"{"passes": []}"#).contains("expected an object of pass name"));
    }

    #[test]
    fn refuses_an_unknown_top_level_field() {
        assert!(refusal(r#"{"passez": {}}"#).contains("unknown field `passez`"));
    }

    #[test]
    fn a_bad_entry_is_worded_with_its_pass_name() {
        for entry in [
            r#""-I""#,
            r#"{}"#,
            r#"{"args": "-I"}"#,
            r#"{"args": [1]}"#,
            r#"{"args": [], "argz": []}"#,
        ] {
            let msg = refusal(&format!(r#"{{"passes": {{"ws": {entry}}}}}"#));
            assert!(
                msg.contains(r#"pass "ws" must be {"args": [strings]}"#),
                "{entry}: {msg}"
            );
        }
    }

    #[test]
    fn refuses_a_pass_defined_twice() {
        let msg = refusal(r#"{"passes": {"ws": {"args": []}, "ws": {"args": ["-I", "x"]}}}"#);
        assert!(msg.contains(r#"pass "ws" is defined twice"#), "{msg}");
    }

    #[test]
    fn refuses_a_repeated_field() {
        assert!(refusal(r#"{"passes": {}, "passes": {}}"#).contains("duplicate field `passes`"));
        let msg = refusal(r#"{"passes": {"ws": {"args": [], "args": []}}}"#);
        assert!(msg.contains(r#"pass "ws" must be"#), "{msg}");
    }

    #[test]
    fn a_string_pin_is_noted_bare_and_any_other_as_json() {
        let t = pass_table(r#"{"vizzle": "0.13.0"}"#).unwrap();
        assert_eq!(t.notes.len(), 1);
        assert!(
            t.notes[0].starts_with("pins vizzle 0.13.0; the pin is ignored"),
            "{}",
            t.notes[0]
        );
        let t = pass_table(r#"{"vizzle": {"v": 1}}"#).unwrap();
        assert!(
            t.notes[0].starts_with(r#"pins vizzle {"v":1};"#),
            "{}",
            t.notes[0]
        );
    }

    fn spec_with(vizzle: Option<&str>) -> String {
        let field = vizzle
            .map(|v| format!(r#""vizzle": {v},"#))
            .unwrap_or_default();
        format!(
            r#"{{"title": "T", "subtitle": "s", "footnote": "f", "canvas": {{"width": 800}}, {field}
                "nodes": [{{"id": "thing", "type": "box", "name": "Thing", "x": [40, 240], "y": 140}}],
                "legend": {{"x": 480, "y": 140, "w": 300, "rows": [["edge", null, "flow"]], "note": null}}}}"#
        )
    }

    #[test]
    fn spec_passes_keeps_the_spec_order_and_checks_the_table() {
        let table = pass_table(r#"{"passes": {"a": {"args": []}, "b": {"args": []}}}"#).unwrap();
        assert_eq!(
            spec_passes(&spec_with(Some(r#"["b", "a"]"#)), &table).unwrap(),
            vec!["b", "a"]
        );
        assert!(spec_passes(&spec_with(Some("[]")), &table)
            .unwrap()
            .is_empty());
        let msg = format!(
            "{:#}",
            spec_passes(&spec_with(Some(r#"["c"]"#)), &table).unwrap_err()
        );
        assert_eq!(
            msg,
            r#"spec names vizzle pass "c", which the pass table does not define"#
        );
    }

    #[test]
    fn spec_passes_refuses_what_the_spec_parser_refuses() {
        let table = PassTable::default();
        for bad in [spec_with(Some(r#""a""#)), spec_with(None)] {
            let msg = format!("{:#}", spec_passes(&bad, &table).unwrap_err());
            assert!(msg.starts_with("not a hybrid spec"), "{msg}");
        }
    }

    #[test]
    fn reads_module_ids_as_group_and_leaf_and_drops_external_edges() {
        let json = r#"{"components":[
            {"name":"@scope/core","path":"packages/core","group":"packages"},
            {"name":"cli","path":"cli","group":""}],
          "edges":[{"from":"cli","to":"packages/core","external":false},
                   {"from":"cli","to":"click","external":true}]}"#;
        let g = module_graph(json).unwrap();
        assert_eq!(g.modules, vec!["cli", "packages/core"]);
        assert_eq!(
            g.edges,
            vec![("cli".to_owned(), "packages/core".to_owned())]
        );
    }
}
