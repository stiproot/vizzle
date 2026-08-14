"""Build a self-contained interactive HTML page (d3 renderer) from graph JSON."""

from __future__ import annotations

import html as html_escape
import json
from importlib import resources


def build_html(
    graph_json: str,
    *,
    title: str,
    show_members: bool = True,
    include_externals: bool = False,
) -> str:
    assets = resources.files("vizzy_cli") / "assets"
    template = (assets / "template.html").read_text(encoding="utf-8")
    d3_source = (assets / "d3.v7.min.js").read_text(encoding="utf-8")
    config = json.dumps({"showMembers": show_members, "includeExternals": include_externals})
    # A literal "</script>" inside embedded JSON would end the script block early.
    safe_json = graph_json.replace("</", "<\\/")
    return (
        template.replace("__TITLE__", html_escape.escape(title))
        .replace("__D3_JS__", d3_source)
        .replace("__GRAPH_JSON__", safe_json)
        .replace("__CONFIG_JSON__", config)
    )


def summarize(graph_json: str) -> str:
    stats = json.loads(graph_json)["stats"]
    return f"{stats['classes']} classes, {stats['relations']} relations"
