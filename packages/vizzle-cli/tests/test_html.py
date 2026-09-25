"""The page must survive whatever the parsed source named things."""

import json
import re

from vizzle_cli import html


def _graph_data(page: str) -> dict:
    return json.loads(re.search(r'<script id="graph-data" type="application/json">(.*?)</script>', page, re.S).group(1))


def test_source_derived_names_cannot_break_the_script_block() -> None:
    # `<!--<script>` puts the HTML tokenizer into the state where the real
    # `</script>` no longer closes the block; `</script>` is the classic.
    hostile = "<!--<script>x</script><script>alert(1)</script>"
    graph_json = json.dumps({"classes": [{"name": hostile}], "relations": [], "stats": {}})
    page = html.build_html(graph_json, title="t")
    assert "<!--<script>" not in page
    assert "alert(1)</script>" not in page
    assert _graph_data(page)["classes"][0]["name"] == hostile


def test_a_placeholder_shaped_identifier_is_data_not_a_placeholder() -> None:
    graph_json = json.dumps({"classes": [{"name": "__CONFIG_JSON__"}], "relations": [], "stats": {}})
    page = html.build_html(graph_json, title="__GRAPH_JSON__")
    assert _graph_data(page)["classes"][0]["name"] == "__CONFIG_JSON__"
    assert "<title>__GRAPH_JSON__</title>" in page
    assert page.count("__D3_JS__") == 0


def test_title_is_html_escaped() -> None:
    page = html.build_html('{"classes":[],"relations":[],"stats":{}}', title="</title><script>alert(9)</script>")
    assert "<script>alert(9)" not in page
    assert "&lt;/title&gt;" in page
