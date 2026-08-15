"""Build a self-contained interactive HTML page (d3 renderer) from graph JSON."""

from __future__ import annotations

import html as html_escape
import json
from importlib import resources


def _fill(template_name: str, graph_json: str, title: str, config: dict) -> str:
    assets = resources.files("vizzy_cli") / "assets"
    template = (assets / template_name).read_text(encoding="utf-8")
    d3_source = (assets / "d3.v7.min.js").read_text(encoding="utf-8")
    # A literal "</script>" inside embedded JSON would end the script block early.
    safe_json = graph_json.replace("</", "<\\/")
    return (
        template.replace("__TITLE__", html_escape.escape(title))
        .replace("__D3_JS__", d3_source)
        .replace("__GRAPH_JSON__", safe_json)
        .replace("__CONFIG_JSON__", json.dumps(config))
    )


def build_html(
    graph_json: str,
    *,
    title: str,
    show_members: bool = True,
    include_externals: bool = False,
) -> str:
    config = {"showMembers": show_members, "includeExternals": include_externals}
    return _fill("template.html", graph_json, title, config)


def build_component_html(
    graph_json: str,
    *,
    title: str,
    include_externals: bool = False,
) -> str:
    config = {"includeExternals": include_externals}
    return _fill("template-component.html", graph_json, title, config)


def summarize(graph_json: str) -> str:
    stats = json.loads(graph_json)["stats"]
    return f"{stats['classes']} classes, {stats['relations']} relations"


def summarize_components(graph_json: str) -> str:
    graph = json.loads(graph_json)
    internal = sum(1 for e in graph["edges"] if not e["external"])
    return f"{graph['stats']['components']} components, {internal} dependencies"
