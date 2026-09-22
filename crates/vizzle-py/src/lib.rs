//! Python bindings for vizzle-core.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use ::vizzle_core as vc;
use vc::{ComponentRenderOptions, Grouping, RenderOptions, SelectOptions};

fn to_py_err(err: anyhow::Error) -> PyErr {
    PyValueError::new_err(format!("{err:#}"))
}

fn selection(
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    splits: Vec<String>,
) -> SelectOptions {
    SelectOptions {
        include,
        exclude,
        langs,
        splits,
    }
}

#[allow(clippy::too_many_arguments)]
fn options(
    show_members: bool,
    show_modules: bool,
    grouping: &str,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<RenderOptions> {
    Ok(RenderOptions {
        show_members,
        show_modules,
        grouping: Grouping::parse(grouping).map_err(PyValueError::new_err)?,
        component_of: Default::default(),
        changed_members_only: false,
        highlight: None,
        include_externals,
        direction,
        title,
    })
}

/// Render a Mermaid class diagram for all supported sources under `root`.
#[pyfunction]
#[pyo3(signature = (
    root,
    *,
    include = vec![],
    exclude = vec![],
    langs = vec![],
    highlight = vec![],
    around = vec![],
    depth = 1,
    show_members = true,
    show_modules = false,
    grouping = "none",
    include_externals = false,
    direction = None,
    title = None,
))]
#[allow(clippy::too_many_arguments)]
fn class_diagram_from_dir(
    root: &str,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    highlight: Vec<String>,
    around: Vec<String>,
    depth: usize,
    show_members: bool,
    show_modules: bool,
    grouping: &str,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<String> {
    let select = SelectOptions {
        include,
        exclude,
        langs,
        splits: vec![],
    };
    let render = options(
        show_members,
        show_modules,
        grouping,
        include_externals,
        direction,
        title,
    )?;
    vc::diagram_from_dir(
        std::path::Path::new(root),
        &select,
        &render,
        &lens(highlight, around, depth),
    )
    .map_err(to_py_err)
}

/// Render a Mermaid class diagram from `(relative_path, contents)` pairs.
#[pyfunction]
#[pyo3(signature = (
    files,
    *,
    show_members = true,
    show_modules = false,
    grouping = "none",
    include_externals = false,
    direction = None,
    title = None,
))]
fn class_diagram_from_files(
    files: Vec<(String, String)>,
    show_members: bool,
    show_modules: bool,
    grouping: &str,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<String> {
    let render = options(
        show_members,
        show_modules,
        grouping,
        include_externals,
        direction,
        title,
    )?;
    vc::diagram_from_files(&files, &render).map_err(to_py_err)
}

/// Render a change-highlighted diagram from base/head revisions of a file set.
#[pyfunction]
#[pyo3(signature = (
    base_files,
    head_files,
    *,
    include = vec![],
    exclude = vec![],
    langs = vec![],
    show_members = true,
    show_modules = false,
    grouping = "none",
    include_externals = false,
    direction = None,
    title = None,
))]
#[allow(clippy::too_many_arguments)]
fn class_diagram_diff(
    base_files: Vec<(String, String)>,
    head_files: Vec<(String, String)>,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    show_members: bool,
    show_modules: bool,
    grouping: &str,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<(String, String, Option<String>)> {
    let render = options(
        show_members,
        show_modules,
        grouping,
        include_externals,
        direction,
        title,
    )?;
    let select = selection(include, exclude, langs, vec![]);
    let diagram =
        vc::diff_diagram(&base_files, &head_files, &select, &render).map_err(to_py_err)?;
    Ok(diff_pair(diagram))
}

/// `(mermaid, verdict_json, zoom)`: the diagram, its verdict and the optional
/// class-level zoom, as the CLI wants them. The verdict is the change counts
/// plus `omitted` (components a focus pass left out) and `zoomClasses`.
fn diff_pair(diagram: vc::DiffDiagram) -> (String, String, Option<String>) {
    let mut verdict = vc::export::change_counts_json(&diagram.changes);
    verdict["omitted"] = diagram.omitted.into();
    verdict["zoomClasses"] = diagram.zoom_classes.into();
    (diagram.mermaid, verdict.to_string(), diagram.zoom)
}

fn diff_view(scope: Option<String>, focus: bool, zoom: bool) -> vc::DiffView {
    vc::DiffView {
        scope: scope.unwrap_or_default(),
        focus,
        zoom,
    }
}

fn component_options(
    group: bool,
    weights: bool,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> ComponentRenderOptions {
    ComponentRenderOptions {
        group,
        weights,
        include_externals,
        direction,
        title,
        highlight: None,
    }
}

fn lens(highlight: Vec<String>, around: Vec<String>, depth: usize) -> vc::Lens {
    vc::Lens {
        highlight,
        around,
        depth,
    }
}

/// Render a Mermaid component diagram (modules + dependency edges) for `root`.
#[pyfunction]
#[pyo3(signature = (
    root,
    *,
    include = vec![],
    exclude = vec![],
    langs = vec![],
    splits = vec![],
    highlight = vec![],
    around = vec![],
    depth = 1,
    group = true,
    weights = false,
    include_externals = false,
    direction = None,
    title = None,
))]
#[allow(clippy::too_many_arguments)]
fn component_diagram_from_dir(
    root: &str,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    splits: Vec<String>,
    highlight: Vec<String>,
    around: Vec<String>,
    depth: usize,
    group: bool,
    weights: bool,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<String> {
    let select = SelectOptions {
        include,
        exclude,
        langs,
        splits,
    };
    let render = component_options(group, weights, include_externals, direction, title);
    vc::component_diagram_from_dir(
        std::path::Path::new(root),
        &select,
        &render,
        &lens(highlight, around, depth),
    )
    .map_err(to_py_err)
}

/// Export the component graph under `root` as JSON (for external renderers).
#[pyfunction]
#[pyo3(signature = (root, *, include = vec![], exclude = vec![], langs = vec![], splits = vec![], classes = true, highlight = vec![], around = vec![], depth = 1))]
#[allow(clippy::too_many_arguments)]
fn component_json_from_dir(
    root: &str,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    splits: Vec<String>,
    classes: bool,
    highlight: Vec<String>,
    around: Vec<String>,
    depth: usize,
) -> PyResult<String> {
    let select = SelectOptions {
        include,
        exclude,
        langs,
        splits,
    };
    vc::component_json_from_dir(
        std::path::Path::new(root),
        &select,
        classes,
        &lens(highlight, around, depth),
    )
    .map_err(to_py_err)
}

/// Render a change-highlighted component diagram from two full revisions.
/// Both sides take the complete source file set plus the manifest files.
#[pyfunction]
#[pyo3(signature = (
    base_files,
    base_manifests,
    head_files,
    head_manifests,
    *,
    include = vec![],
    exclude = vec![],
    langs = vec![],
    splits = vec![],
    focus = false,
    zoom = false,
    group = true,
    weights = false,
    include_externals = false,
    direction = None,
    title = None,
    scope = None,
))]
#[allow(clippy::too_many_arguments)]
fn component_diagram_diff(
    base_files: Vec<(String, String)>,
    base_manifests: Vec<(String, String)>,
    head_files: Vec<(String, String)>,
    head_manifests: Vec<(String, String)>,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    splits: Vec<String>,
    focus: bool,
    zoom: bool,
    group: bool,
    weights: bool,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
    scope: Option<String>,
) -> PyResult<(String, String, Option<String>)> {
    let render = component_options(group, weights, include_externals, direction, title);
    let select = selection(include, exclude, langs, splits);
    let view = diff_view(scope, focus, zoom);
    let diagram = vc::component_diff_diagram(
        &base_files,
        &base_manifests,
        &head_files,
        &head_manifests,
        &select,
        &view,
        &render,
    )
    .map_err(to_py_err)?;
    Ok(diff_pair(diagram))
}

/// Export a change-annotated component graph from two full revisions as JSON.
#[pyfunction]
#[pyo3(signature = (
    base_files,
    base_manifests,
    head_files,
    head_manifests,
    *,
    include = vec![],
    exclude = vec![],
    langs = vec![],
    splits = vec![],
    focus = false,
    classes = true,
    scope = None,
))]
#[allow(clippy::too_many_arguments)]
fn component_json_diff(
    base_files: Vec<(String, String)>,
    base_manifests: Vec<(String, String)>,
    head_files: Vec<(String, String)>,
    head_manifests: Vec<(String, String)>,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    splits: Vec<String>,
    focus: bool,
    classes: bool,
    scope: Option<String>,
) -> PyResult<String> {
    let select = selection(include, exclude, langs, splits);
    let view = diff_view(scope, focus, false);
    vc::component_json_diff(
        &base_files,
        &base_manifests,
        &head_files,
        &head_manifests,
        &select,
        &view,
        classes,
    )
    .map_err(to_py_err)
}

/// Render a curated diagram from a `gen:c4-code` manifest and the sources under `root`.
#[pyfunction]
#[pyo3(signature = (root, manifest, *, include = vec![], exclude = vec![], langs = vec![]))]
fn curated_diagram_from_dir(
    root: &str,
    manifest: &str,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
) -> PyResult<String> {
    let select = SelectOptions {
        include,
        exclude,
        langs,
        splits: vec![],
    };
    vc::curated_from_dir(std::path::Path::new(root), &select, manifest).map_err(to_py_err)
}

/// The diff palette as CSS custom properties, for inlining into an HTML page.
/// Keeps the HTML views on the same colors as the Mermaid renderer.
#[pyfunction]
fn diff_palette_css() -> String {
    vc::palette::css_variables()
}

/// Export the class graph under `root` as JSON (for external renderers).
#[pyfunction]
#[pyo3(signature = (root, *, include = vec![], exclude = vec![], langs = vec![], highlight = vec![], around = vec![], depth = 1))]
#[allow(clippy::too_many_arguments)]
fn graph_json_from_dir(
    root: &str,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    highlight: Vec<String>,
    around: Vec<String>,
    depth: usize,
) -> PyResult<String> {
    let select = SelectOptions {
        include,
        exclude,
        langs,
        splits: vec![],
    };
    vc::json_from_dir(
        std::path::Path::new(root),
        &select,
        &lens(highlight, around, depth),
    )
    .map_err(to_py_err)
}

/// Export a change-annotated class graph from base/head revisions as JSON.
#[pyfunction]
#[pyo3(signature = (base_files, head_files, *, include = vec![], exclude = vec![], langs = vec![]))]
fn graph_json_diff(
    base_files: Vec<(String, String)>,
    head_files: Vec<(String, String)>,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
) -> PyResult<String> {
    let select = selection(include, exclude, langs, vec![]);
    vc::json_diff(&base_files, &head_files, &select).map_err(to_py_err)
}

#[pymodule]
#[pyo3(name = "_core")]
fn vizzle_core_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(class_diagram_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(class_diagram_from_files, m)?)?;
    m.add_function(wrap_pyfunction!(class_diagram_diff, m)?)?;
    m.add_function(wrap_pyfunction!(component_diagram_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(component_json_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(component_diagram_diff, m)?)?;
    m.add_function(wrap_pyfunction!(component_json_diff, m)?)?;
    m.add_function(wrap_pyfunction!(curated_diagram_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(diff_palette_css, m)?)?;
    m.add_function(wrap_pyfunction!(graph_json_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(graph_json_diff, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
