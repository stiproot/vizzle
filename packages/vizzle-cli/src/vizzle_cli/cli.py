"""vizzle command-line interface."""

from __future__ import annotations

import sys
from pathlib import Path

import click

from . import _core, git, managed
from . import render as render_mod
from .html import build_component_html, build_html, summarize, summarize_components


def _render_kwargs(
    members: bool, modules: bool, group: bool, externals: bool, direction: str | None, title: str | None
) -> dict:
    return {
        "show_members": members,
        "show_modules": modules,
        "group_by_module": group,
        "include_externals": externals,
        "direction": direction,
        "title": title,
    }


def _component_render_kwargs(
    group: bool, weights: bool, externals: bool, direction: str | None, title: str | None
) -> dict:
    return {
        "group": group,
        "weights": weights,
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
        # Explicit utf-8: diagrams carry «guillemets» and ✚✖✱ glyphs, and the
        # platform default encoding would mangle or refuse them.
        output.write_text(content, encoding="utf-8")
        click.echo(f"wrote {output}  ({summary})", err=True)
    else:
        click.echo(content)


def _emit_mermaid(diagram: str, output: Path | None) -> None:
    marker = next((line for line in diagram.splitlines() if line.startswith("%% vizzle:")), "")
    _emit(diagram, output, marker.removeprefix("%% vizzle: "))


def _compose(*options):
    """Apply a set of click options as one decorator, declared in one place."""

    def decorate(fn):
        for option in reversed(options):
            fn = option(fn)
        return fn

    return decorate


# Which files to read. Shared by every command that walks a tree.
select_options = _compose(
    click.option("-I", "--include", multiple=True, help="Glob of relative paths to include (repeatable)."),
    click.option("-E", "--exclude", multiple=True, help="Glob of relative paths to exclude (repeatable)."),
    click.option(
        "-l",
        "--lang",
        multiple=True,
        type=click.Choice(["python", "typescript"]),
        help="Restrict languages (repeatable).",
    ),
)

# Where the diagram goes and what it is called. Shared by every command that emits one.
output_options = _compose(
    click.option(
        "--direction",
        type=click.Choice(["TB", "BT", "LR", "RL"]),
        default=None,
        help="Layout direction (mermaid only).",
    ),
    click.option("--title", default=None, help="Diagram title."),
    click.option(
        "-f",
        "--format",
        "fmt",
        type=click.Choice(["mermaid", "html"]),
        default=None,
        help="Output format. Defaults to html when the output file ends in .html, else mermaid.",
    ),
    click.option(
        "-o",
        "--output",
        type=click.Path(dir_okay=False, path_type=Path),
        default=None,
        help="Write the diagram to a file instead of stdout.",
    ),
)

# Class-diagram rendering choices, plus the shared output set.
render_options = _compose(
    click.option("--members/--no-members", default=True, show_default=True, help="Render fields and methods."),
    click.option(
        "--modules",
        is_flag=True,
        help="Add one «module» box per module holding its public module-level functions.",
    ),
    click.option(
        "--group/--no-group",
        "group",
        default=False,
        show_default=True,
        help="Group classes into namespace blocks per module (mermaid only).",
    ),
    click.option("--externals", is_flag=True, help="Show inheritance edges to types outside the parsed set."),
    output_options,
)


@click.group()
@click.version_option(package_name="vizzle")
def main() -> None:
    """UML visualization for git: class diagrams from code, as Mermaid or interactive HTML."""


@main.command("class")
@click.argument("path", type=click.Path(exists=True, path_type=Path), default=".")
@select_options
@render_options
def class_diagram(
    path: Path,
    include: tuple[str, ...],
    exclude: tuple[str, ...],
    lang: tuple[str, ...],
    members: bool,
    modules: bool,
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
        graph_json = _core.graph_json_from_dir(
            str(path), include=list(include), exclude=list(exclude), langs=list(lang)
        )
        page = build_html(
            graph_json,
            title=title or f"{path.resolve().name} — class diagram",
            show_members=members,
            show_modules=modules,
            include_externals=externals,
        )
        _emit(page, output, summarize(graph_json, show_modules=modules))
        return

    diagram = _core.class_diagram_from_dir(
        str(path),
        include=list(include),
        exclude=list(exclude),
        langs=list(lang),
        **_render_kwargs(members, modules, group, externals, direction, title),
    )
    _emit_mermaid(diagram, output)


@main.command("doc")
@click.argument("docs", nargs=-1, type=click.Path(exists=True, dir_okay=False, path_type=Path))
@click.option(
    "--dir",
    "directory",
    type=click.Path(exists=True, file_okay=False, path_type=Path),
    default=None,
    help="Regenerate every managed document under this directory.",
)
@click.option(
    "--root",
    type=click.Path(exists=True, file_okay=False, path_type=Path),
    default=Path("."),
    show_default=True,
    help="Repo root that manifest `file` paths resolve against.",
)
@click.option("--check", "check", is_flag=True, help="Fail if any document is out of date; write nothing.")
@select_options
def doc_command(
    docs: tuple[Path, ...],
    directory: Path | None,
    root: Path,
    check: bool,
    include: tuple[str, ...],
    exclude: tuple[str, ...],
    lang: tuple[str, ...],
) -> None:
    """Regenerate managed diagram documents from their gen:c4-code manifest.

    A managed document is markdown carrying a manifest comment and one mermaid
    fence; only the fence is rewritten. `--check` reports drift and writes
    nothing, which is what belongs in a lint chain.
    Spec: docs/curated-diagrams.md.
    """
    if not docs and directory is None:
        raise click.UsageError("give document paths, or --dir to scan a directory")

    paths = list(docs) + (managed.discover(directory) if directory else [])
    stale: list[Path] = []
    written = 0
    for path in paths:
        try:
            doc = managed.read(path)
        except managed.ManagedDocError as err:
            raise click.ClickException(str(err)) from err
        if doc is None:
            continue

        try:
            diagram = _core.curated_diagram_from_dir(
                str(root), doc.manifest, include=list(include), exclude=list(exclude), langs=list(lang)
            )
            updated = doc.with_diagram(diagram)
        except (ValueError, managed.ManagedDocError) as err:
            raise click.ClickException(f"{path}: {err}") from err

        if updated == doc.text:
            continue
        if check:
            stale.append(path)
        else:
            path.write_text(updated, encoding="utf-8")
            click.echo(f"regenerated {path}", err=True)
            written += 1

    if check and stale:
        for path in stale:
            click.echo(f"out of date: {path}", err=True)
        raise click.ClickException(f"{len(stale)} document(s) need regenerating; run `vizzle doc`")
    if check:
        click.echo(f"{len(paths)} document(s) checked, all current", err=True)
    elif not written:
        click.echo("nothing to regenerate", err=True)


@main.command("render")
@click.argument("src", type=click.Path(exists=True, path_type=Path))
@click.argument("out_dir", type=click.Path(file_okay=False, path_type=Path))
@click.option("-f", "--format", "fmt", type=click.Choice(["png", "svg"]), default="png", show_default=True)
@click.option("--scale", default=2, show_default=True, help="Pixel density multiplier (png only).")
@click.option("--background", default="white", show_default=True, help="Page background colour.")
def render_command(src: Path, out_dir: Path, fmt: str, scale: int, background: str) -> None:
    """Render mermaid sources under SRC to images in OUT_DIR.

    SRC is a `.md` (every fence in it), a `.mmd`, or a directory of them
    (README.md excluded). Sources stay the truth — GitHub and IDEs render fences
    natively — so images are produced on demand and usually gitignored.

    Uses mermaid-cli, resolved from PATH or run through bunx/npx; nothing is
    installed. The whole-repo `maxTextSize` limit is raised for you.
    """
    try:
        written = [
            path
            for source in render_mod.sources(src)
            for path in render_mod.render(source, out_dir, fmt=fmt, scale=scale, background=background)
        ]
    except render_mod.RenderError as err:
        raise click.ClickException(str(err)) from err
    for path in written:
        click.echo(f"rendered {path}", err=True)
    if not written:
        click.echo("nothing rendered", err=True)


@main.command("component")
@click.argument("path", type=click.Path(exists=True, path_type=Path), default=".")
@select_options
@click.option(
    "--group/--no-group",
    "group",
    default=True,
    show_default=True,
    help="Wrap sibling components in a block per parent directory.",
)
@click.option("--weights", is_flag=True, help="Label edges with their weight (distinct importing files).")
@click.option(
    "--classes/--no-classes",
    "classes",
    default=True,
    show_default=True,
    help="Embed each component's classes so they can be opened in the page (html only).",
)
@click.option("--externals", is_flag=True, help="Show one node per external package (npm/PyPI).")
@output_options
def component_diagram(
    path: Path,
    include: tuple[str, ...],
    exclude: tuple[str, ...],
    lang: tuple[str, ...],
    group: bool,
    weights: bool,
    classes: bool,
    externals: bool,
    direction: str | None,
    title: str | None,
    fmt: str | None,
    output: Path | None,
) -> None:
    """Generate a component diagram for the codebase at PATH.

    One box per build-level module (workspace package, app, service), one
    dashed arrow per dependency derived from imports. In the HTML view, open a
    component to see the classes inside it. Spec: docs/diagram-types/component.md.
    """
    resolved_fmt = _resolve_format(fmt, output)
    if resolved_fmt == "html":
        graph_json = _core.component_json_from_dir(
            str(path), include=list(include), exclude=list(exclude), langs=list(lang), classes=classes
        )
        page = build_component_html(
            graph_json,
            title=title or f"{path.resolve().name} — component diagram",
            include_externals=externals,
        )
        _emit(page, output, summarize_components(graph_json))
        return

    diagram = _core.component_diagram_from_dir(
        str(path),
        include=list(include),
        exclude=list(exclude),
        langs=list(lang),
        **_component_render_kwargs(group, weights, externals, direction, title),
    )
    _emit_mermaid(diagram, output)


def _repo_root(path: Path) -> Path:
    """The repository `path` lives in, as a CLI error if there isn't one."""
    try:
        return git.repo_root(path)
    except git.GitError as err:
        raise click.ClickException(f"not a git repository: {err}") from err


def _collect_diff_files(path: Path, base: str, head: str | None) -> tuple[list[tuple[str, str]], list[tuple[str, str]]]:
    """Base- and head-revision contents of every changed source file."""
    root = _repo_root(path)

    pathspec = None
    resolved = path.resolve()
    if resolved != root:
        pathspec = str(resolved.relative_to(root))

    try:
        changes = git.changed_files(root, base, head, pathspec)
    except git.GitError as err:
        raise click.ClickException(str(err)) from err

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
    return base_files, head_files


def _collect_component_revision(root: Path, ref: str | None) -> tuple[list[tuple[str, str]], list[tuple[str, str]]]:
    """The complete `(sources, manifests)` file sets at `ref` (worktree if None).

    Unlike the class diff, the component diff needs full revisions on both
    sides: whether an edge exists depends on files a change never touched.
    """
    paths = git.worktree_paths(root) if ref is None else git.tree_paths(root, ref)
    sources = [p for p in paths if git.is_source(p)]
    manifests = [p for p in paths if git.is_manifest(p)]
    if ref is None:
        return git.files_in_worktree(root, sources), git.files_in_worktree(root, manifests)
    return git.files_at_ref(root, ref, sources), git.files_at_ref(root, ref, manifests)


def _collect_component_diff(path: Path, base: str, head: str | None) -> tuple[list, list, list, list]:
    root = _repo_root(path)
    try:
        base_files, base_manifests = _collect_component_revision(root, base)
        head_files, head_manifests = _collect_component_revision(root, head)
    except git.GitError as err:
        raise click.ClickException(str(err)) from err
    return base_files, base_manifests, head_files, head_manifests


@main.command("diff")
@click.argument("path", type=click.Path(exists=True, path_type=Path), default=".")
@click.option("--base", default="HEAD", show_default=True, help="Base git revision to compare against.")
@click.option("--head", default=None, help="Head git revision (defaults to the working tree).")
@click.option(
    "--type",
    "diagram_type",
    type=click.Choice(["class", "component"]),
    default="class",
    show_default=True,
    help="Diagram type. `component` diffs the module dependency graph (rewiring shows loudest).",
)
@click.option("--weights", is_flag=True, help="Label edges with their weight (component type, mermaid).")
@render_options
def diff_diagram(
    path: Path,
    base: str,
    head: str | None,
    diagram_type: str,
    weights: bool,
    members: bool,
    modules: bool,
    group: bool,
    externals: bool,
    direction: str | None,
    title: str | None,
    fmt: str | None,
    output: Path | None,
) -> None:
    """Diagram of what changed between BASE and HEAD (or the working tree).

    Added elements are green, removed red, modified yellow; class member rows
    carry ✚ / ✖ / ✱ markers. Unchanged classes in touched files appear as
    context. With --type component, both revisions are parsed in full and the
    diagram highlights components whose files changed plus dependency edges
    that were added or removed.
    """
    if diagram_type == "component":
        base_files, base_manifests, head_files, head_manifests = _collect_component_diff(path, base, head)
        resolved_title = title or f"changes vs {base}"
        if _resolve_format(fmt, output) == "html":
            graph_json = _core.component_json_diff(base_files, base_manifests, head_files, head_manifests, classes=True)
            page = build_component_html(graph_json, title=resolved_title, include_externals=externals)
            _emit(page, output, summarize_components(graph_json))
            return
        diagram = _core.component_diagram_diff(
            base_files,
            base_manifests,
            head_files,
            head_manifests,
            **_component_render_kwargs(True, weights, externals, direction, resolved_title),
        )
        _emit_mermaid(diagram, output)
        return

    base_files, head_files = _collect_diff_files(path, base, head)
    if not base_files and not head_files:
        raise click.ClickException(
            f"no changed Python/TypeScript files between {base} and {head or 'the working tree'}"
        )

    resolved_title = title or f"changes vs {base}"
    if _resolve_format(fmt, output) == "html":
        graph_json = _core.graph_json_diff(base_files, head_files)
        page = build_html(
            graph_json,
            title=resolved_title,
            show_members=members,
            show_modules=modules,
            include_externals=externals,
        )
        _emit(page, output, summarize(graph_json, show_modules=modules))
        return

    diagram = _core.class_diagram_diff(
        base_files,
        head_files,
        **_render_kwargs(members, modules, group, externals, direction, resolved_title),
    )
    _emit_mermaid(diagram, output)


@main.command("serve")
@click.argument("path", type=click.Path(exists=True, path_type=Path), default=".")
@click.option(
    "--type",
    "diagram_type",
    type=click.Choice(["class", "component"]),
    default="class",
    show_default=True,
    help="Diagram type to serve.",
)
@click.option("--diff", "diff_mode", is_flag=True, help="Serve a live diff of the working tree against --base.")
@click.option("--base", default="HEAD", show_default=True, help="Base git revision (diff mode).")
@click.option("--head", default=None, help="Head git revision (diff mode; defaults to the working tree).")
@click.option("-I", "--include", multiple=True, help="Glob of relative paths to include (class mode, repeatable).")
@click.option("-E", "--exclude", multiple=True, help="Glob of relative paths to exclude (class mode, repeatable).")
@click.option("-l", "--lang", multiple=True, type=click.Choice(["python", "typescript"]), help="Restrict languages.")
@click.option("--members/--no-members", default=True, show_default=True, help="Render fields and methods.")
@click.option(
    "--modules",
    is_flag=True,
    help="Add one «module» box per module holding its public module-level functions.",
)
@click.option("--externals", is_flag=True, help="Show inheritance edges to types outside the parsed set.")
@click.option("--title", default=None, help="Diagram title.")
@click.option("--host", default="127.0.0.1", show_default=True)
@click.option("--port", default=8499, show_default=True, help="Port to bind (0 picks a free port).")
@click.option("--open", "open_browser", is_flag=True, help="Open the page in your browser.")
def serve_command(
    path: Path,
    diagram_type: str,
    diff_mode: bool,
    base: str,
    head: str | None,
    include: tuple[str, ...],
    exclude: tuple[str, ...],
    lang: tuple[str, ...],
    members: bool,
    modules: bool,
    externals: bool,
    title: str | None,
    host: str,
    port: int,
    open_browser: bool,
) -> None:
    """Serve the diagram for PATH with live reload.

    The page regenerates on every load, and connected browsers reload
    automatically whenever a watched source file under PATH changes.
    With --diff, you watch the working tree's changes against --base
    reshape the diagram as you edit.
    """
    from . import server

    if head:
        diff_mode = True

    def build_page() -> str:
        if diagram_type == "component":
            if diff_mode:
                base_files, base_manifests, head_files, head_manifests = _collect_component_diff(path, base, head)
                graph_json = _core.component_json_diff(
                    base_files, base_manifests, head_files, head_manifests, classes=True
                )
                page_title = title or f"changes vs {base} (live)"
            else:
                graph_json = _core.component_json_from_dir(
                    str(path), include=list(include), exclude=list(exclude), langs=list(lang)
                )
                page_title = title or f"{path.resolve().name} — component diagram (live)"
            return build_component_html(graph_json, title=page_title, include_externals=externals)
        if diff_mode:
            base_files, head_files = _collect_diff_files(path, base, head)
            graph_json = _core.graph_json_diff(base_files, head_files)
            page_title = title or f"changes vs {base} (live)"
        else:
            graph_json = _core.graph_json_from_dir(
                str(path), include=list(include), exclude=list(exclude), langs=list(lang)
            )
            page_title = title or f"{path.resolve().name} — class diagram (live)"
        return build_html(
            graph_json,
            title=page_title,
            show_members=members,
            show_modules=modules,
            include_externals=externals,
        )

    build_page()  # fail fast (bad path, not a git repo, ...) before binding the port

    def on_ready(url: str) -> None:
        mode = f"{diagram_type} diff vs {base}" if diff_mode else f"{diagram_type} diagram"
        click.echo(f"serving {mode} of {path.resolve()} at {url}  (ctrl-c to stop)", err=True)
        if open_browser:
            import webbrowser

            webbrowser.open(url)

    def on_error(message: str) -> None:
        click.echo(f"warning: {message}", err=True)

    server.serve(build_page, path.resolve(), host, port, on_ready, on_error)


if __name__ == "__main__":
    sys.exit(main())
