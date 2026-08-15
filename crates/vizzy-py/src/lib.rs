//! Python bindings for vizzy-core.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use ::vizzy_core as vc;
use vc::{ComponentRenderOptions, RenderOptions, SelectOptions};

fn to_py_err(err: anyhow::Error) -> PyErr {
    PyValueError::new_err(format!("{err:#}"))
}

#[allow(clippy::too_many_arguments)]
fn options(
    show_members: bool,
    group_by_module: bool,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> RenderOptions {
    RenderOptions {
        show_members,
        group_by_module,
        include_externals,
        direction,
        title,
    }
}

/// Render a Mermaid class diagram for all supported sources under `root`.
#[pyfunction]
#[pyo3(signature = (
    root,
    *,
    include = vec![],
    exclude = vec![],
    langs = vec![],
    show_members = true,
    group_by_module = false,
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
    show_members: bool,
    group_by_module: bool,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<String> {
    let select = SelectOptions {
        include,
        exclude,
        langs,
    };
    let render = options(
        show_members,
        group_by_module,
        include_externals,
        direction,
        title,
    );
    vc::diagram_from_dir(std::path::Path::new(root), &select, &render).map_err(to_py_err)
}

/// Render a Mermaid class diagram from `(relative_path, contents)` pairs.
#[pyfunction]
#[pyo3(signature = (
    files,
    *,
    show_members = true,
    group_by_module = false,
    include_externals = false,
    direction = None,
    title = None,
))]
fn class_diagram_from_files(
    files: Vec<(String, String)>,
    show_members: bool,
    group_by_module: bool,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<String> {
    let render = options(
        show_members,
        group_by_module,
        include_externals,
        direction,
        title,
    );
    vc::diagram_from_files(&files, &render).map_err(to_py_err)
}

/// Render a change-highlighted diagram from base/head revisions of a file set.
#[pyfunction]
#[pyo3(signature = (
    base_files,
    head_files,
    *,
    show_members = true,
    group_by_module = false,
    include_externals = false,
    direction = None,
    title = None,
))]
#[allow(clippy::too_many_arguments)]
fn class_diagram_diff(
    base_files: Vec<(String, String)>,
    head_files: Vec<(String, String)>,
    show_members: bool,
    group_by_module: bool,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<String> {
    let render = options(
        show_members,
        group_by_module,
        include_externals,
        direction,
        title,
    );
    vc::diff_diagram(&base_files, &head_files, &render).map_err(to_py_err)
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
    };
    let render = component_options(group, weights, include_externals, direction, title);
    vc::component_diagram_from_dir(std::path::Path::new(root), &select, &render).map_err(to_py_err)
}

/// Export the component graph under `root` as JSON (for external renderers).
#[pyfunction]
#[pyo3(signature = (root, *, include = vec![], exclude = vec![], langs = vec![], classes = true))]
fn component_json_from_dir(
    root: &str,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
    classes: bool,
) -> PyResult<String> {
    let select = SelectOptions {
        include,
        exclude,
        langs,
    };
    vc::component_json_from_dir(std::path::Path::new(root), &select, classes).map_err(to_py_err)
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
    group = true,
    weights = false,
    include_externals = false,
    direction = None,
    title = None,
))]
#[allow(clippy::too_many_arguments)]
fn component_diagram_diff(
    base_files: Vec<(String, String)>,
    base_manifests: Vec<(String, String)>,
    head_files: Vec<(String, String)>,
    head_manifests: Vec<(String, String)>,
    group: bool,
    weights: bool,
    include_externals: bool,
    direction: Option<String>,
    title: Option<String>,
) -> PyResult<String> {
    let render = component_options(group, weights, include_externals, direction, title);
    vc::component_diff_diagram(
        &base_files,
        &base_manifests,
        &head_files,
        &head_manifests,
        &render,
    )
    .map_err(to_py_err)
}

/// Export a change-annotated component graph from two full revisions as JSON.
#[pyfunction]
#[pyo3(signature = (base_files, base_manifests, head_files, head_manifests, *, classes = true))]
fn component_json_diff(
    base_files: Vec<(String, String)>,
    base_manifests: Vec<(String, String)>,
    head_files: Vec<(String, String)>,
    head_manifests: Vec<(String, String)>,
    classes: bool,
) -> PyResult<String> {
    vc::component_json_diff(
        &base_files,
        &base_manifests,
        &head_files,
        &head_manifests,
        classes,
    )
    .map_err(to_py_err)
}

/// The diff palette as CSS custom properties, for inlining into an HTML page.
/// Keeps the HTML views on the same colors as the Mermaid renderer.
#[pyfunction]
fn diff_palette_css() -> String {
    vc::palette::css_variables()
}

/// Export the class graph under `root` as JSON (for external renderers).
#[pyfunction]
#[pyo3(signature = (root, *, include = vec![], exclude = vec![], langs = vec![]))]
fn graph_json_from_dir(
    root: &str,
    include: Vec<String>,
    exclude: Vec<String>,
    langs: Vec<String>,
) -> PyResult<String> {
    let select = SelectOptions {
        include,
        exclude,
        langs,
    };
    vc::json_from_dir(std::path::Path::new(root), &select).map_err(to_py_err)
}

/// Export a change-annotated class graph from base/head revisions as JSON.
#[pyfunction]
fn graph_json_diff(
    base_files: Vec<(String, String)>,
    head_files: Vec<(String, String)>,
) -> PyResult<String> {
    vc::json_diff(&base_files, &head_files).map_err(to_py_err)
}

#[pymodule]
#[pyo3(name = "vizzy_core")]
fn vizzy_core_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(class_diagram_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(class_diagram_from_files, m)?)?;
    m.add_function(wrap_pyfunction!(class_diagram_diff, m)?)?;
    m.add_function(wrap_pyfunction!(component_diagram_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(component_json_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(component_diagram_diff, m)?)?;
    m.add_function(wrap_pyfunction!(component_json_diff, m)?)?;
    m.add_function(wrap_pyfunction!(diff_palette_css, m)?)?;
    m.add_function(wrap_pyfunction!(graph_json_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(graph_json_diff, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
