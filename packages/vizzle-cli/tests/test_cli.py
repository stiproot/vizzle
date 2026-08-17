"""End-to-end CLI tests against a throwaway git repo."""

import json
import re
import subprocess
from pathlib import Path

import pytest
from click.testing import CliRunner
from vizzle_cli.cli import main


def git(cwd: Path, *args: str) -> None:
    """Run git in a throwaway repo with a fixed identity and no user config."""
    subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=True,
        capture_output=True,
        env={
            "GIT_AUTHOR_NAME": "t",
            "GIT_AUTHOR_EMAIL": "t@t",
            "GIT_COMMITTER_NAME": "t",
            "GIT_COMMITTER_EMAIL": "t@t",
            "PATH": "/usr/bin:/bin",
        },
    )


@pytest.fixture()
def repo(tmp_path: Path) -> Path:
    def git_(*args: str) -> None:
        git(tmp_path, *args)

    git_("init")
    (tmp_path / "app.py").write_text("class Base:\n    def run(self) -> int: ...\n\nclass Old:\n    pass\n")
    git_("add", ".")
    git_("commit", "-m", "base")
    (tmp_path / "app.py").write_text(
        "class Base:\n    def run(self) -> int: ...\n\nclass Fresh(Base):\n    name: str\n"
    )
    return tmp_path


def test_class_diagram(repo: Path) -> None:
    result = CliRunner().invoke(main, ["class", str(repo)])
    assert result.exit_code == 0, result.output
    assert "classDiagram" in result.output
    assert 'class app_Fresh["app.Fresh"]' in result.output
    assert "app_Fresh --|> app_Base" in result.output


def test_diff_diagram(repo: Path) -> None:
    result = CliRunner().invoke(main, ["diff", str(repo)])
    assert result.exit_code == 0, result.output
    assert 'cssClass "app_Fresh" vizzleAdded' in result.output
    assert 'cssClass "app_Old" vizzleRemoved' in result.output
    # classDef statements must trail the attachments (mermaid 11 quirk).
    assert result.output.rindex("classDef") > result.output.rindex("cssClass")


def test_class_diagram_html(repo: Path, tmp_path: Path) -> None:
    out = tmp_path / "graph.html"
    result = CliRunner().invoke(main, ["class", str(repo), "-o", str(out)])
    assert result.exit_code == 0, result.output
    page = out.read_text()
    assert page.startswith("<!doctype html>")
    assert "d3js.org" in page  # vendored d3 is inlined
    assert '"qualified":"app.Fresh"' in page.replace(" ", "")
    assert "__GRAPH_JSON__" not in page and "__D3_JS__" not in page


def test_diff_html_marks_changes(repo: Path) -> None:
    result = CliRunner().invoke(main, ["diff", str(repo), "--format", "html"])
    assert result.exit_code == 0, result.output
    compact = result.output.replace(" ", "")
    assert '"change":"added"' in compact
    assert '"change":"removed"' in compact
    assert '"diff":true' in compact


def test_diff_outside_git_repo(tmp_path: Path) -> None:
    result = CliRunner().invoke(main, ["diff", str(tmp_path)])
    assert result.exit_code != 0


@pytest.fixture()
def workspace(tmp_path: Path) -> Path:
    """A committed mini-workspace: two packages, one app depending on core."""

    def write(rel: str, contents: str) -> None:
        path = tmp_path / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)

    git(tmp_path, "init")
    write("package.json", '{"name": "root", "workspaces": ["packages/*", "apps/*"]}')
    write("packages/core/package.json", '{"name": "@w/core"}')
    write("packages/core/src/index.ts", "export class Core {}\n")
    write("packages/util/package.json", '{"name": "@w/util"}')
    write("packages/util/src/index.ts", "export const u = 1;\n")
    write("apps/svc/package.json", '{"name": "svc"}')
    write(
        "apps/svc/src/main.ts",
        'import { Core } from "@w/core";\n'
        "export class Handler {}\n"
        "export class Svc {\n"
        "  private handler: Handler;\n"
        "  run(h: Handler): Core { return null; }\n"
        "}\n",
    )
    git(tmp_path, "add", ".")
    git(tmp_path, "commit", "-m", "base")
    return tmp_path


def test_component_diagram(workspace: Path) -> None:
    result = CliRunner().invoke(main, ["component", str(workspace)])
    assert result.exit_code == 0, result.output
    assert result.output.startswith("flowchart LR")
    assert 'subgraph sg_apps["apps"]' in result.output
    assert "«component»<br/><b>svc</b>" in result.output
    assert "c_apps_svc -.-> c_packages_core" in result.output
    assert "c_packages_util" in result.output  # present even with no edges
    assert "%% vizzle: 3 components, 1 dependencies" in result.output


def test_component_html(workspace: Path, tmp_path: Path) -> None:
    out = tmp_path / "components.html"
    result = CliRunner().invoke(main, ["component", str(workspace), "-o", str(out)])
    assert result.exit_code == 0, result.output
    page = out.read_text()
    assert page.startswith("<!doctype html>")
    compact = page.replace(" ", "")
    assert '"path":"packages/core"' in compact
    assert '"name":"@w/core"' in compact
    # Class detail rides along so a component can be opened in the page.
    assert '"component":"packages/core"' in compact
    assert '"name":"Core"' in compact
    assert "__GRAPH_JSON__" not in page and "__D3_JS__" not in page


def test_component_payload_carries_a_class_diagram(workspace: Path, tmp_path: Path) -> None:
    """An exploded component needs relations and typed members, not just names."""
    out = tmp_path / "components.html"
    result = CliRunner().invoke(main, ["component", str(workspace), "-o", str(out)])
    assert result.exit_code == 0, result.output
    graph = json.loads(re.search(r'id="graph-data"[^>]*>(.*?)</script>', out.read_text(), re.S).group(1))

    relations = {(r["from"].rsplit(".", 1)[-1], r["to"].rsplit(".", 1)[-1]): r["kind"] for r in graph["classRelations"]}
    # A field's type is structural; a method signature's types are a dependency.
    assert relations[("Svc", "Handler")] == "association"
    assert relations[("Svc", "Core")] == "dependency"

    svc = next(c for c in graph["classes"] if c["name"] == "Svc")
    run = next(m for m in svc["members"] if m["name"] == "run")
    assert run["detail"] == "h: Handler", "parameter types reach the rendered signature"
    assert run["returns"] == "Core"


@pytest.mark.parametrize("command", [["class"], ["component"]])
def test_pages_inline_the_shared_core(workspace: Path, tmp_path: Path, command: list[str]) -> None:
    """Both views are built from viz-core.{css,js}; neither may ship a placeholder."""
    out = tmp_path / "page.html"
    result = CliRunner().invoke(main, [*command, str(workspace), "-o", str(out)])
    assert result.exit_code == 0, result.output
    page = out.read_text()
    assert "window.vizzle" in page  # shared JS
    assert "--context-fill" in page  # shared palette
    assert "attachViewport" in page and "focus" in page  # shared viewport API
    assert not re.search(r"__[A-Z0-9_]+__", page)


def test_component_no_classes_makes_a_lean_page(workspace: Path, tmp_path: Path) -> None:
    out = tmp_path / "lean.html"
    result = CliRunner().invoke(main, ["component", str(workspace), "--no-classes", "-o", str(out)])
    assert result.exit_code == 0, result.output
    compact = out.read_text().replace(" ", "")
    assert '"classes":[]' in compact
    assert '"path":"packages/core"' in compact  # components still there


def test_component_diff_shows_rewiring(workspace: Path) -> None:
    # A new file wires svc to @w/util, which it never used before.
    extra = workspace / "apps/svc/src/extra.ts"
    extra.write_text('import { u } from "@w/util";\n')
    result = CliRunner().invoke(main, ["diff", str(workspace), "--type", "component"])
    assert result.exit_code == 0, result.output
    assert "«component»<br/><b>svc ✱</b>" in result.output
    assert 'c_apps_svc -. "✚" .-> c_packages_util' in result.output
    assert "linkStyle" in result.output and "#1a7f37" in result.output
    # The untouched dependency stays uncolored context.
    assert "c_apps_svc -.-> c_packages_core" in result.output


DOC = """# A managed diagram

Prose above the fence.

<!-- gen:c4-code {
  "classes": [
    {"id": "Shape", "kind": "interface", "file": "src/shape.ts", "symbol": "Shape"},
    {"id": "redis", "kind": "external", "stereotype": "peer service", "note": "flat keyspace"}
  ],
  "relations": [["Shape", "redis", null, "caches in"]]
} -->

```mermaid
classDiagram
  stale content
```

Prose below the fence.
"""


def _managed_repo(tmp_path):
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "shape.ts").write_text("export interface Shape { area(): number; }\n")
    doc = tmp_path / "diagram.md"
    doc.write_text(DOC)
    return doc


def test_doc_regenerates_only_the_fence(tmp_path):
    doc = _managed_repo(tmp_path)
    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert result.exit_code == 0, result.output
    text = doc.read_text()

    assert "Prose above the fence." in text and "Prose below the fence." in text
    assert "stale content" not in text
    assert "<<interface>>" in text and "+area() number" in text
    # A fully curated entry contributes its note and needs no source.
    assert "<<peer service>>" in text and "flat keyspace" in text
    assert "Shape --> redis : caches in" in text


def test_doc_check_detects_drift_and_writes_nothing(tmp_path):
    doc = _managed_repo(tmp_path)
    before = doc.read_text()

    stale = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path), "--check"])
    assert stale.exit_code != 0, "a stale fence must fail --check"
    assert doc.read_text() == before, "--check must not write"

    CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    current = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path), "--check"])
    assert current.exit_code == 0, current.output


def test_doc_reports_an_entry_that_no_longer_resolves(tmp_path):
    doc = _managed_repo(tmp_path)
    (tmp_path / "src" / "shape.ts").write_text("export interface Renamed { area(): number; }\n")
    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert result.exit_code != 0
    assert "Shape" in result.output and "not in the parsed graph" in result.output


MODULE_DOC = """<!-- gen:c4-code {
  "classes": [
    {"id": "Cmd", "kind": "module", "file": "app.py",
     "functions": ["run"], "consts": ["WELL_KNOWN"], "stereotype": "Typer app"}
  ]
} -->

```mermaid
classDiagram
```
"""


def test_doc_lists_consts_before_functions_and_omits_param_types(tmp_path):
    (tmp_path / "app.py").write_text(
        "WELL_KNOWN: dict[str, str] = {}\n"
        "_PRIVATE = 1\n"
        "def run(ctx: Context, slug: str | None, verbose: bool) -> None: ...\n"
    )
    doc = tmp_path / "d.md"
    doc.write_text(MODULE_DOC)

    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert result.exit_code == 0, result.output
    body = doc.read_text()

    # An entry naming both must get both — consts first, as the managed docs read.
    assert body.index("WELL_KNOWN") < body.index("+run("), body
    # A curated diagram is read by people: parameter names, not their types.
    assert "+run(ctx, slug, verbose) None" in body, body
    assert "ctx: Context" not in body
    assert "_PRIVATE" not in body, "a private module-level name is not module surface"


def test_render_selects_diagram_sources_not_the_readme(tmp_path):
    from vizzle_cli import render as render_mod

    (tmp_path / "README.md").write_text("# index\n")
    (tmp_path / "b.md").write_text("```mermaid\nclassDiagram\n```\n")
    (tmp_path / "a.mmd").write_text("classDiagram\n")
    (tmp_path / "notes.txt").write_text("ignore me\n")

    names = [p.name for p in render_mod.sources(tmp_path)]
    assert names == ["a.mmd", "b.md"], "sorted, README and non-diagrams excluded"
    assert render_mod.sources(tmp_path / "b.md") == [tmp_path / "b.md"]


def test_render_reports_an_empty_directory(tmp_path):
    from vizzle_cli import render as render_mod

    (tmp_path / "README.md").write_text("# only an index\n")
    with pytest.raises(render_mod.RenderError, match="no diagram sources"):
        render_mod.sources(tmp_path)


def test_render_raises_the_mermaid_text_cap(tmp_path):
    from vizzle_cli import render as render_mod

    # A whole-repo diagram is past mermaid's default 50,000-character limit, and
    # mermaid draws a small error graphic rather than failing.
    assert render_mod.CONFIG["maxTextSize"] > 50_000
