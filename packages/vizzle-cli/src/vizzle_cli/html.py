"""Build a self-contained interactive HTML page (d3 renderer) from graph JSON."""

from __future__ import annotations

import html as html_escape
import json
from importlib import resources

from . import _core


def _fill(template_name: str, graph_json: str, title: str, config: dict) -> str:
    """Inline every asset into one self-contained page.

    Each view owns its own template; the palette, viewport, filter, and header
    plumbing they share live in viz-core.{css,js} and are inlined into both.
    """
    assets = resources.files("vizzle_cli") / "assets"

    def read(name: str) -> str:
        return (assets / name).read_text(encoding="utf-8")

    # A literal "</script>" inside embedded JSON would end the script block early.
    safe_json = graph_json.replace("</", "<\\/")
    # The change palette is owned by vizzle-core so HTML and Mermaid agree.
    core_css = read("viz-core.css").replace("__PALETTE_CSS__", _core.diff_palette_css())
    return (
        read(template_name)
        .replace("__TITLE__", html_escape.escape(title))
        .replace("__VIZ_CORE_CSS__", core_css)
        .replace("__D3_JS__", read("d3.v7.min.js"))
        .replace("__VIZ_CORE_JS__", read("viz-core.js"))
        .replace("__GRAPH_JSON__", safe_json)
        .replace("__CONFIG_JSON__", json.dumps(config))
    )


def build_html(
    graph_json: str,
    *,
    title: str,
    show_members: bool = True,
    show_modules: bool = False,
    include_externals: bool = False,
) -> str:
    # showModules mirrors the mermaid renderer's show_modules: the graph JSON
    # always carries the «module» boxes, and the page decides (class.md §2.4).
    config = {
        "showMembers": show_members,
        "showModules": show_modules,
        "includeExternals": include_externals,
    }
    return _fill("template.html", graph_json, title, config)


def build_component_html(
    graph_json: str,
    *,
    title: str,
    include_externals: bool = False,
) -> str:
    config = {"includeExternals": include_externals}
    return _fill("template-component.html", graph_json, title, config)


def summarize(graph_json: str, *, show_modules: bool = True) -> str:
    graph = json.loads(graph_json)
    stats = graph["stats"]
    if show_modules:
        return f"{stats['classes']} classes, {stats['relations']} relations"
    # The JSON always carries the «module» boxes and their edges; the page hides
    # them, so report what will actually be drawn rather than what was parsed.
    hidden = {c["qualified"] for c in graph["classes"] if c.get("annotation") == "module"}
    relations = sum(1 for r in graph["relations"] if r["from"] not in hidden and r["to"] not in hidden)
    return f"{stats['classes'] - len(hidden)} classes, {relations} relations"


def summarize_components(graph_json: str) -> str:
    graph = json.loads(graph_json)
    internal = sum(1 for e in graph["edges"] if not e["external"])
    summary = f"{graph['stats']['components']} components, {internal} dependencies"
    if graph["stats"].get("classes"):
        summary += f", {graph['stats']['classes']} classes"
    return summary
