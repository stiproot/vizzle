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


@pytest.mark.parametrize("command", [["class"], ["component"]])
def test_pages_have_no_external_references(workspace: Path, tmp_path: Path, command: list[str]) -> None:
    """An emitted page must not fetch anything; d3 and all assets are inlined."""
    out = tmp_path / "page.html"
    result = CliRunner().invoke(main, [*command, str(workspace), "-o", str(out)])
    assert result.exit_code == 0, result.output
    page = out.read_text()
    offenders = re.findall(
        r'(?:src|href)=["\']https?://[^\'"]+["\']|url\(https?://[^)]+\)',
        page,
    )
    assert not offenders, f"External references found in page: {offenders}"


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


def test_component_diff_scoped_to_package_root_has_fewer_components(workspace: Path) -> None:
    """Scoping to a component root restricts the diagram to that component and its boundary neighbours.

    Workspace: core, util, svc (svc→core). Scoping to packages/core gives only
    core (in-scope) + svc (boundary, imports core) = 2 components, vs 3 unscoped.
    packages/util has no edge to core, so it is excluded.
    """
    # Touch a file in core so the diff has something to record.
    (workspace / "packages/core/src/extra.ts").write_text("export const v = 2;\n")

    unscoped = CliRunner().invoke(main, ["diff", str(workspace), "--type", "component"])
    assert unscoped.exit_code == 0, unscoped.output

    # Scope to packages/core: only core + svc (which imports core) should appear;
    # packages/util has no edge touching core.
    scoped = CliRunner().invoke(main, ["diff", str(workspace / "packages/core"), "--type", "component"])
    assert scoped.exit_code == 0, scoped.output

    import re

    def component_count(output: str) -> int:
        m = re.search(r"(\d+) components", output)
        assert m, f"no component count in: {output}"
        return int(m.group(1))

    unscoped_n = component_count(unscoped.output)
    scoped_n = component_count(scoped.output)
    assert scoped_n < unscoped_n, f"scoped ({scoped_n}) should have fewer components than unscoped ({unscoped_n})"
    # packages/core must appear; packages/util must NOT.
    assert "packages_core" in scoped.output, "core must appear in its own scope"
    assert "packages_util" not in scoped.output, "util has no edge to core and must be excluded"


FIXTURE_GLOB = "**/tests/fixtures/**"


def _add_fixture_component(workspace: Path) -> None:
    """A test fixture that is itself a package: a manifest plus one source file, committed."""
    fixture = workspace / "packages/core/tests/fixtures/repo"
    fixture.mkdir(parents=True)
    (fixture / "package.json").write_text('{"name": "fixture-repo"}')
    (fixture / "index.ts").write_text("export class Fixture {}\n")
    git(workspace, "add", ".")
    git(workspace, "commit", "-m", "fixture")


def _component_count(output: str) -> int:
    m = re.search(r"(\d+) components", output)
    assert m, f"no component count in: {output}"
    return int(m.group(1))


def test_component_diff_exclude_drops_a_component_on_both_revisions(workspace: Path) -> None:
    """-E removes the matching component from the diff without it reading as added or removed."""
    _add_fixture_component(workspace)
    (workspace / "packages/core/src/extra.ts").write_text("export const v = 2;\n")

    unfiltered = CliRunner().invoke(main, ["diff", str(workspace), "--type", "component"])
    assert unfiltered.exit_code == 0, unfiltered.output
    assert "fixture-repo" in unfiltered.output
    assert _component_count(unfiltered.output) == 4

    filtered = CliRunner().invoke(main, ["diff", str(workspace), "--type", "component", "-E", FIXTURE_GLOB])
    assert filtered.exit_code == 0, filtered.output
    assert "fixture-repo" not in filtered.output
    assert _component_count(filtered.output) == 3
    # The fixture exists at both revisions; filtering it must not surface as a change.
    assert "✚" not in filtered.output and "✖" not in filtered.output, filtered.output
    assert "«component»<br/><b>@w/core ✱</b>" in filtered.output
    assert f"%% vizzle: selection: exclude {FIXTURE_GLOB}" in filtered.output


def test_component_diff_exclude_wins_over_boundary(workspace: Path) -> None:
    """An excluded component is dropped even when it would otherwise be kept as a «boundary» neighbour."""
    (workspace / "packages/core/src/extra.ts").write_text("export const v = 2;\n")
    scoped = CliRunner().invoke(main, ["diff", str(workspace / "packages/core"), "--type", "component"])
    assert scoped.exit_code == 0, scoped.output
    assert "«boundary»<br/><b>svc</b>" in scoped.output
    assert "c_apps_svc -.-> c_packages_core" in scoped.output

    result = CliRunner().invoke(
        main, ["diff", str(workspace / "packages/core"), "--type", "component", "-E", "apps/**"]
    )
    assert result.exit_code == 0, result.output
    assert "«boundary»" not in result.output
    assert "c_apps_svc" not in result.output, "the edge into the excluded component goes with it"
    assert _component_count(result.output) == 1


def test_component_diff_change_confined_to_excluded_paths_is_no_change(workspace: Path) -> None:
    """A change that touches only excluded files renders as no structural change (spec §6.2)."""
    _add_fixture_component(workspace)
    (workspace / "packages/core/tests/fixtures/repo/index.ts").write_text("export class Fixture { x = 1 }\n")

    unfiltered = CliRunner().invoke(main, ["diff", str(workspace), "--type", "component"])
    assert unfiltered.exit_code == 0, unfiltered.output
    assert "vizzleModified" in unfiltered.output

    filtered = CliRunner().invoke(main, ["diff", str(workspace), "--type", "component", "-E", FIXTURE_GLOB])
    assert filtered.exit_code == 0, filtered.output
    assert not re.search(r"vizzle(Added|Removed|Modified)", filtered.output), filtered.output


def test_component_diff_html_legend_carries_the_selection(workspace: Path, tmp_path: Path) -> None:
    _add_fixture_component(workspace)
    (workspace / "packages/core/src/extra.ts").write_text("export const v = 2;\n")
    out = tmp_path / "diff.html"
    result = CliRunner().invoke(
        main, ["diff", str(workspace), "--type", "component", "-E", FIXTURE_GLOB, "-l", "typescript", "-o", str(out)]
    )
    assert result.exit_code == 0, result.output
    page = out.read_text()
    payload = json.loads(re.search(r'id="graph-data"[^>]*>(.*?)</script>', page, re.S).group(1))
    assert payload["stats"]["selection"] == [f"exclude {FIXTURE_GLOB}", "lang typescript"]
    assert "fixture-repo" not in page


def test_class_diff_selection_that_filters_everything_is_an_error(repo: Path) -> None:
    result = CliRunner().invoke(main, ["diff", str(repo), "-E", "app.py"])
    assert result.exit_code == 1, result.output
    assert "Error: no changed files match the include/exclude/lang selection" in result.output, result.output


def test_core_errors_are_reported_not_raised(workspace: Path) -> None:
    """An invalid glob is the user's input; it comes back as `Error:`, never a traceback."""
    result = CliRunner().invoke(main, ["component", str(workspace), "-E", "["])
    assert result.exit_code == 1, result.output
    assert result.output.startswith("Error: invalid glob `[`"), result.output


def test_component_diff_refuses_non_root_path(workspace: Path) -> None:
    """Passing a path inside a component root (not the root itself) is an error."""
    # packages/core/src is inside the packages/core component root.
    src_dir = workspace / "packages/core/src"
    src_dir.mkdir(parents=True, exist_ok=True)

    result = CliRunner().invoke(main, ["diff", str(src_dir), "--type", "component"])
    assert result.exit_code != 0, "should refuse a sub-root path"
    assert "no component is rooted at" in result.output, result.output
    # The error must name the enclosing component root.
    assert "packages/core" in result.output, result.output


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


# Scoped managed documents: a manifest that names a path instead of symbols.
# The point of the mode is that it catches an *addition*, which a curated
# symbol list cannot. See docs/plans/scope-and-grouping.md.


def _scoped_repo(tmp_path: Path, group: str = "module", extra: str = "") -> Path:
    src = tmp_path / "src" / "pkg"
    src.mkdir(parents=True)
    (src / "alpha.py").write_text("class Alpha:\n    def run(self) -> int: ...\n")
    (src / "beta.py").write_text("class Beta:\n    pass\n")
    if extra:
        (src / "extra.py").write_text(extra)
    docs = tmp_path / "docs"
    docs.mkdir()
    doc = docs / "scoped.md"
    doc.write_text(
        "# Scoped\n\nProse above.\n\n"
        '<!-- gen:c4-code {"scope":{"path":"src/pkg","lang":"python",'
        f'"group":"{group}"}}}} -->\n\n'
        "```mermaid\nstale\n```\n\nProse below.\n"
    )
    return doc


def test_doc_scope_generates_from_a_path(tmp_path):
    doc = _scoped_repo(tmp_path)
    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert result.exit_code == 0, result.output

    text = doc.read_text()
    assert "Prose above." in text and "Prose below." in text
    assert "stale" not in text
    assert "Alpha" in text and "Beta" in text
    assert "+run() int" in text, "scoped mode renders members like the class command"


def test_doc_scope_catches_an_added_class(tmp_path):
    """The reason this mode exists: a curated symbol list cannot do this."""
    doc = _scoped_repo(tmp_path)
    CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path), "--check"]).exit_code == 0

    (tmp_path / "src" / "pkg" / "gamma.py").write_text("class Gamma:\n    pass\n")

    stale = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path), "--check"])
    assert stale.exit_code != 0, "a new class in scope must fail --check"
    assert "out of date" in stale.output

    CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert "Gamma" in doc.read_text()


def test_doc_scope_groups_by_component(tmp_path):
    doc = _scoped_repo(tmp_path, group="component")
    (tmp_path / "pyproject.toml").write_text('[project]\nname = "demo"\n')
    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert result.exit_code == 0, result.output
    assert "namespace" in doc.read_text()


def test_doc_rejects_a_manifest_carrying_both_scope_and_classes(tmp_path):
    docs = tmp_path / "docs"
    docs.mkdir()
    doc = docs / "both.md"
    doc.write_text('# X\n<!-- gen:c4-code {"scope":{"path":"src"},"classes":[]} -->\n\n```mermaid\nx\n```\n')
    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert result.exit_code != 0
    assert "both `scope` and `classes`" in result.output


def test_doc_rejects_an_unknown_scope_key(tmp_path):
    docs = tmp_path / "docs"
    docs.mkdir()
    doc = docs / "typo.md"
    doc.write_text('# X\n<!-- gen:c4-code {"scope":{"path":"src","grouping":"module"}} -->\n\n```mermaid\nx\n```\n')
    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    assert result.exit_code != 0
    assert "unknown `scope` key(s): grouping" in result.output


def test_doc_fails_a_diagram_past_the_mermaid_limit(tmp_path, monkeypatch):
    """Past the ceiling mermaid draws an error graphic, so nobody reports it."""
    from vizzle_cli import managed

    monkeypatch.setattr(managed, "MERMAID_LIMIT", 200)
    monkeypatch.setattr(managed, "MERMAID_WARN", 100)
    doc = _scoped_repo(tmp_path)

    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path), "--check"])
    assert result.exit_code != 0
    assert "too large" in result.output
    assert "error graphic" in result.output


def test_doc_warns_inside_the_mermaid_margin_but_passes(tmp_path, monkeypatch):
    from vizzle_cli import managed

    doc = _scoped_repo(tmp_path)
    CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path)])
    size = len(doc.read_text())
    # A band this document lands inside, without exceeding the limit.
    monkeypatch.setattr(managed, "MERMAID_LIMIT", size + 200)
    monkeypatch.setattr(managed, "MERMAID_WARN", 1)

    result = CliRunner().invoke(main, ["doc", str(doc), "--root", str(tmp_path), "--check"])
    assert result.exit_code == 0, result.output
    assert "short of mermaid's" in result.output


def test_class_group_by_component_needs_no_flag_change_for_module(tmp_path):
    """--group stays a working alias so existing invocations keep their output."""
    _scoped_repo(tmp_path)
    by_alias = CliRunner().invoke(main, ["class", str(tmp_path / "src"), "--group"])
    by_name = CliRunner().invoke(main, ["class", str(tmp_path / "src"), "--group-by", "module"])
    assert by_alias.exit_code == 0 and by_name.exit_code == 0
    assert by_alias.output == by_name.output


def test_diff_rejects_component_grouping(repo: Path) -> None:
    """A diff has no tree to detect components in; say so rather than degrade."""
    result = CliRunner().invoke(main, ["diff", str(repo), "--group-by", "component"])
    assert result.exit_code != 0
    assert "not available for a diff" in result.output
