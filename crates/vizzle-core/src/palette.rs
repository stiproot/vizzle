//! The diff palette, defined once for every renderer.
//!
//! Mermaid emits its colors as `classDef` statements and the HTML views read
//! them as CSS custom properties, but a reader comparing the two formats must
//! see the same diagram. Both are generated from the table below — when these
//! lived separately they drifted, and "added" was two different greens.
//!
//! Colors that only one renderer has an opinion about (the HTML context grey,
//! the default box fill) stay in `viz-core.css`. This table is exactly the
//! set both formats must agree on.

use crate::model::ChangeKind;

pub struct ChangeColors {
    /// Wire name, used for both the CSS variable prefix and the mermaid class.
    pub name: &'static str,
    pub fill: &'static str,
    pub stroke: &'static str,
    /// Extra mermaid style fragments (removed reads as a dashed outline).
    pub mermaid_extra: &'static str,
}

pub const CHANGE_COLORS: [ChangeColors; 3] = [
    ChangeColors {
        name: "added",
        fill: "#dafbe1",
        stroke: "#1a7f37",
        mermaid_extra: "",
    },
    ChangeColors {
        name: "removed",
        fill: "#ffebe9",
        stroke: "#cf222e",
        mermaid_extra: ",stroke-dasharray:6 4",
    },
    ChangeColors {
        name: "modified",
        fill: "#fff1c2",
        stroke: "#bf8700",
        mermaid_extra: "",
    },
];

pub fn colors_for(change: ChangeKind) -> Option<&'static ChangeColors> {
    let name = match change {
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Modified => "modified",
        ChangeKind::Unchanged => return None,
    };
    CHANGE_COLORS.iter().find(|c| c.name == name)
}

/// Mermaid class name for a change, e.g. `vizzleAdded`.
pub fn mermaid_class(change: ChangeKind) -> Option<String> {
    colors_for(change).map(|c| {
        let mut name = c.name.to_owned();
        name[..1].make_ascii_uppercase();
        format!("vizzle{name}")
    })
}

/// The `classDef` block every mermaid diagram emits in diff mode.
///
/// Mermaid 11 quirk: in `classDiagram` these only take effect when they appear
/// *after* the `cssClass` attachments, so callers emit this last.
pub fn mermaid_classdefs() -> String {
    CHANGE_COLORS
        .iter()
        .map(|c| {
            let class = format!("vizzle{}{}", c.name[..1].to_uppercase(), &c.name[1..]);
            format!(
                "    classDef {class} fill:{},stroke:{},stroke-width:2px,color:{}{}\n",
                c.fill, c.stroke, c.stroke, c.mermaid_extra
            )
        })
        .collect()
}

/// The same colors as CSS custom properties, injected into every HTML page.
pub fn css_variables() -> String {
    CHANGE_COLORS
        .iter()
        .map(|c| {
            format!(
                "  --{}-fill: {};\n  --{}-stroke: {};\n",
                c.name, c.fill, c.name, c.stroke
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_formats_render_the_same_colors() {
        let mermaid = mermaid_classdefs();
        let css = css_variables();
        for colors in &CHANGE_COLORS {
            assert!(
                mermaid.contains(colors.fill),
                "{} missing in mermaid",
                colors.name
            );
            assert!(css.contains(colors.fill), "{} missing in css", colors.name);
            assert!(mermaid.contains(colors.stroke));
            assert!(css.contains(colors.stroke));
        }
        assert!(mermaid.contains("classDef vizzleAdded"));
        assert!(css.contains("--added-fill: #dafbe1;"));
        assert_eq!(
            mermaid_class(ChangeKind::Modified).as_deref(),
            Some("vizzleModified")
        );
        assert_eq!(mermaid_class(ChangeKind::Unchanged), None);
    }
}
