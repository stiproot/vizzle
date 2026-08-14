"""vizzy command-line interface."""

from __future__ import annotations

import sys
from pathlib import Path

import click
import vizzy_core

from . import git
from .html import build_html, summarize


def _render_kwargs(members: bool, group: bool, externals: bool, direction: str | None, title: str | None) -> dict:
    return {
        "show_members": members,
        "group_by_module": group,
        "include_externals": externals,
        "direction": direction,
        "title": title,
    }


def _resolve_format(fmt: str | None, output: Path | None) -> str:
    if fmt:
        return fmt
    if output and output.suffix.lower() in (".html", ".htm"):
        return "html"
    return "mermaid"


def _emit(content: str, output: Path | None, summary: str) -> None:
    if output:
        output.write_text(content)
        click.echo(f"wrote {output}  ({summary})", err=True)
    else:
        click.echo(content)


def _emit_mermaid(diagram: str, output: Path | None) -> None:
    marker = next((line for line in diagram.splitlines() if line.startswith("%% vizzy:")), "")
    _emit(diagram, output, marker.removeprefix("%% vizzy: "))


def render_options(fn):
    fn = click.option("--members/--no-members", default=True, show_default=True, help="Render fields and methods.")(fn)
    fn = click.option(
        "--group/--no-group",
        "group",
        default=False,
        show_default=True,
        help="Group classes into namespace blocks per module (mermaid only).",
    )(fn)
    fn = click.option("--externals", is_flag=True, help="Show inheritance edges to types outside the parsed set.")(fn)
    fn = click.option(
        "--direction",
        type=click.Choice(["TB", "BT", "LR", "RL"]),
        default=None,
        help="Layout direction (mermaid only).",
    )(fn)
    fn = click.option("--title", default=None, help="Diagram title.")(fn)
    fn = click.option(
        "-f",
        "--format",
        "fmt",
        type=click.Choice(["mermaid", "html"]),
        default=None,
        help="Output format. Defaults to html when the output file ends in .html, else mermaid.",
    )(fn)
    fn = click.option(
        "-o",
        "--output",
        type=click.Path(dir_okay=False, path_type=Path),
        default=None,
        help="Write the diagram to a file instead of stdout.",
    )(fn)
    return fn


@click.group()
@click.version_option(package_name="vizzy")
def main() -> None:
    """UML visualization for git: class diagrams from code, as Mermaid or interactive HTML."""


@main.command("class")
@click.argument("path", type=click.Path(exists=True, path_type=Path), default=".")
@click.option("-I", "--include", multiple=True, help="Glob of relative paths to include (repeatable).")
@click.option("-E", "--exclude", multiple=True, help="Glob of relative paths to exclude (repeatable).")
@click.option(
    "-l", "--lang", multiple=True, type=click.Choice(["python", "typescript"]), help="Restrict languages (repeatable)."
)
@render_options
def class_diagram(
    path: Path,
    include: tuple[str, ...],
    exclude: tuple[str, ...],
    lang: tuple[str, ...],
    members: bool,
    group: bool,
    externals: bool,
    direction: str | None,
    title: str | None,
    fmt: str | None,
    output: Path | None,
) -> None:
    """Generate a class diagram for the codebase at PATH."""
    resolved_fmt = _resolve_format(fmt, output)
    if resolved_fmt == "html":
        graph_json = vizzy_core.graph_json_from_dir(
            str(path), include=list(include), exclude=list(exclude), langs=list(lang)
        )
        page = build_html(
            graph_json,
            title=title or f"{path.resolve().name} — class diagram",
            show_members=members,
            include_externals=externals,
        )
        _emit(page, output, summarize(graph_json))
        return

    diagram = vizzy_core.class_diagram_from_dir(
        str(path),
        include=list(include),
        exclude=list(exclude),
        langs=list(lang),
        **_render_kwargs(members, group, externals, direction, title),
    )
    _emit_mermaid(diagram, output)


@main.command("diff")
@click.argument("path", type=click.Path(exists=True, path_type=Path), default=".")
@click.option("--base", default="HEAD", show_default=True, help="Base git revision to compare against.")
@click.option("--head", default=None, help="Head git revision (defaults to the working tree).")
@render_options
def diff_diagram(
    path: Path,
    base: str,
    head: str | None,
    members: bool,
    group: bool,
    externals: bool,
    direction: str | None,
    title: str | None,
    fmt: str | None,
    output: Path | None,
) -> None:
    """Class diagram of what changed between BASE and HEAD (or the working tree).

    Added classes are green, removed red, modified yellow; member rows carry
    ✚ / ✖ / ✱ markers. Unchanged classes in touched files appear as context.
    """
    try:
        root = git.repo_root(path)
    except git.GitError as err:
        raise click.ClickException(f"not a git repository: {err}") from err

    pathspec = None
    resolved = path.resolve()
    if resolved != root:
        pathspec = str(resolved.relative_to(root))

    try:
        changes = git.changed_files(root, base, head, pathspec)
    except git.GitError as err:
        raise click.ClickException(str(err)) from err

    if not changes:
        raise click.ClickException(
            f"no changed Python/TypeScript files between {base} and {head or 'the working tree'}"
        )

    base_files: list[tuple[str, str]] = []
    head_files: list[tuple[str, str]] = []
    for change in changes:
        base_path = change.old_path or change.path
        if change.status != "A":
            contents = git.file_at_ref(root, base, base_path)
            if contents is not None:
                base_files.append((base_path, contents))
        if change.status != "D":
            contents = git.file_at_ref(root, head, change.path) if head else git.file_in_worktree(root, change.path)
            if contents is not None:
                head_files.append((change.path, contents))

    resolved_title = title or f"changes vs {base}"
    if _resolve_format(fmt, output) == "html":
        graph_json = vizzy_core.graph_json_diff(base_files, head_files)
        page = build_html(
            graph_json,
            title=resolved_title,
            show_members=members,
            include_externals=externals,
        )
        _emit(page, output, summarize(graph_json))
        return

    diagram = vizzy_core.class_diagram_diff(
        base_files,
        head_files,
        **_render_kwargs(members, group, externals, direction, resolved_title),
    )
    _emit_mermaid(diagram, output)


if __name__ == "__main__":
    sys.exit(main())
