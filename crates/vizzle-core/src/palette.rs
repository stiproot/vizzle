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

/// Colours for boundary nodes (outside scope, kept because an edge crosses into them).
/// A boundary node renders as pure context even when it changed elsewhere — no
/// change fill/stroke or glyph is applied.
pub struct BoundaryColors {
    pub fill: &'static str,
    pub stroke: &'static str,
    pub color: &'static str,
}

pub const BOUNDARY: BoundaryColors = BoundaryColors {
    fill: "#f6f8fa",
    stroke: "#57606a",
    color: "#57606a",
};

/// The reader's lens (`--highlight`): the elements a question is about, and
/// the context they sit in. Context is the same grey the HTML diff lens uses
/// for unchanged elements, so "not the subject" reads the same everywhere.
pub struct LensColors {
    pub fill: &'static str,
    pub stroke: &'static str,
    pub color: &'static str,
}

pub const HIGHLIGHT: LensColors = LensColors {
    fill: "#ddf4ff",
    stroke: "#0969da",
    color: "#0969da",
};

pub const CONTEXT: LensColors = LensColors {
    fill: "#f2f4f7",
    stroke: "#b6bec8",
    color: "#8b949e",
};

/// Mermaid class names for the lens; unbranded like the diff classes.
pub const MERMAID_HIGHLIGHT: &str = "highlight";
pub const MERMAID_CONTEXT: &str = "context";

/// The `classDef` lines for the lens, emitted last like the diff block.
pub fn mermaid_lens_classdefs() -> String {
    format!(
        "    classDef {MERMAID_HIGHLIGHT} fill:{},stroke:{},stroke-width:3px,color:{}\n    classDef {MERMAID_CONTEXT} fill:{},stroke:{},color:{}\n",
        HIGHLIGHT.fill, HIGHLIGHT.stroke, HIGHLIGHT.color, CONTEXT.fill, CONTEXT.stroke, CONTEXT.color
    )
}

/// CSS custom properties for the lens colours.
pub fn css_lens_variables() -> String {
    format!(
        "  --highlight-fill: {};\n  --highlight-stroke: {};\n  --context-fill: {};\n  --context-stroke: {};\n  --context-ink: {};\n",
        HIGHLIGHT.fill, HIGHLIGHT.stroke, CONTEXT.fill, CONTEXT.stroke, CONTEXT.color
    )
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

/// Mermaid class name for a change, e.g. `diffAdded`.
///
/// Deliberately NOT branded: this string lands in diagram source that consumers post to pull
/// requests and docs, where a tool name is noise for the reader (2026-09-22).
pub fn mermaid_class(change: ChangeKind) -> Option<String> {
    colors_for(change).map(|c| {
        let mut name = c.name.to_owned();
        name[..1].make_ascii_uppercase();
        format!("diff{name}")
    })
}

/// The `classDef` line for the boundary class.
pub fn mermaid_boundary_classdef() -> String {
    format!(
        "    classDef diffBoundary fill:{},stroke:{},stroke-dasharray:4 3,color:{}\n",
        BOUNDARY.fill, BOUNDARY.stroke, BOUNDARY.color
    )
}

/// CSS custom properties for the boundary colours, injected into every HTML page
/// that uses scoped component diagrams.
pub fn css_boundary_variables() -> String {
    format!(
        "  --boundary-fill: {};\n  --boundary-stroke: {};\n  --boundary-color: {};\n",
        BOUNDARY.fill, BOUNDARY.stroke, BOUNDARY.color
    )
}

/// The `classDef` block every mermaid diagram emits in diff mode.
///
/// Mermaid 11 quirk: in `classDiagram` these only take effect when they appear
/// *after* the `cssClass` attachments, so callers emit this last.
pub fn mermaid_classdefs() -> String {
    CHANGE_COLORS
        .iter()
        .map(|c| {
            let class = format!("diff{}{}", c.name[..1].to_uppercase(), &c.name[1..]);
            format!(
                "    classDef {class} fill:{},stroke:{},stroke-width:2px,color:{}{}\n",
                c.fill, c.stroke, c.stroke, c.mermaid_extra
            )
        })
        .collect()
}

/// The same colors as CSS custom properties, injected into every HTML page.
pub fn css_variables() -> String {
    let change_vars: String = CHANGE_COLORS
        .iter()
        .map(|c| {
            format!(
                "  --{}-fill: {};\n  --{}-stroke: {};\n",
                c.name, c.fill, c.name, c.stroke
            )
        })
        .collect();
    change_vars + &css_boundary_variables() + &css_lens_variables()
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
        assert!(mermaid.contains("classDef diffAdded"));
        assert!(css.contains("--added-fill: #dafbe1;"));
        assert_eq!(
            mermaid_class(ChangeKind::Modified).as_deref(),
            Some("diffModified")
        );
        assert_eq!(mermaid_class(ChangeKind::Unchanged), None);
    }

    #[test]
    fn boundary_colors_are_consistent_across_renderers() {
        let mermaid = mermaid_boundary_classdef();
        let css = css_variables();
        assert!(
            mermaid.contains(BOUNDARY.fill),
            "boundary fill missing in mermaid classdef"
        );
        assert!(
            mermaid.contains(BOUNDARY.stroke),
            "boundary stroke missing in mermaid classdef"
        );
        assert!(
            css.contains(BOUNDARY.fill),
            "boundary fill missing in css variables"
        );
        assert!(
            css.contains(BOUNDARY.stroke),
            "boundary stroke missing in css variables"
        );
        assert!(mermaid.contains("diffBoundary"));
        assert!(css.contains("--boundary-fill:"));
    }
}
