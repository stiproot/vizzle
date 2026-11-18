//! The module layer: one vizzle pass, read as the module graph a picture
//! reconciles against.
//!
//! The generator this replaces ran `vizzle component` and parsed its mermaid
//! with regexes. A pass now arrives as the component graph JSON
//! (`component::to_json`, the atoms' contract), and only the two facts a
//! picture needs are kept: which modules exist and which imports which.

use anyhow::{Context, Result};
use serde::Deserialize;

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

#[cfg(test)]
mod tests {
    use super::*;

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
