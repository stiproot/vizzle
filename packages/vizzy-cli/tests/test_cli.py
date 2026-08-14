"""End-to-end CLI tests against a throwaway git repo."""

import subprocess
from pathlib import Path

import pytest
from click.testing import CliRunner
from vizzy_cli.cli import main


@pytest.fixture()
def repo(tmp_path: Path) -> Path:
    def git(*args: str) -> None:
        subprocess.run(
            ["git", *args],
            cwd=tmp_path,
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

    git("init")
    (tmp_path / "app.py").write_text("class Base:\n    def run(self) -> int: ...\n\nclass Old:\n    pass\n")
    git("add", ".")
    git("commit", "-m", "base")
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
    assert 'cssClass "app_Fresh" vizzyAdded' in result.output
    assert 'cssClass "app_Old" vizzyRemoved' in result.output
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
