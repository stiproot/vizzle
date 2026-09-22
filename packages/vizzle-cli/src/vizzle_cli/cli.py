"""vizzle command-line interface."""

from __future__ import annotations

import json
import sys
from pathlib import Path

import click

from . import _core, git, managed
from . import render as render_mod
from .html import build_component_html, build_html, summarize, summarize_components

GROUPINGS = ("none", "module", "component")


def _grouping(group_by: str | None, group: bool) -> str:
    """Resolve the grouping from the current flag and the deprecated one.

    `--group` predates `--group-by` and meant "per module". It stays as an
    alias so existing invocations and manifests keep working; `--group-by`
    wins when both are given.
    """
    if group_by is not None:
        return group_by
    return "module" if group else "none"


def _render_kwargs(
    members: bool,
    modules: bool,
    grouping: str,
    externals: bool,
    direction: str | None,
    title: str | None,
) -> dict:
    return {
        "show_members": members,
        "show_modules": modules,
        "grouping": grouping,
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


def _selection_kwargs(include: tuple[str, ...], exclude: tuple[str, ...], lang: tuple[str, ...]) -> dict:
    return {"include": list(include), "exclude": list(exclude), "langs": list(lang)}


def _component_scope_path(path: Path) -> str:
    """Compute scope path for component diff: "" if path is repo root, else relative path.

    Raises ClickException if the path is inside a component root but not rooted at one
    (i.e. no manifest file sits directly in the given directory).
    """
    root = _repo_root(path)
    resolved = path.resolve()
    try:
        relative = resolved.relative_to(root)
    except ValueError as exc:
        raise click.ClickException(f"path is outside the repository root {root}: {path}") from exc
    if resolved == root:
        return ""
    rel_str = relative.as_posix()
    if _is_component_root(resolved):
        return rel_str
    # Path is not a component root — find the nearest enclosing one.
    parent = resolved.parent
    while parent != root:
        if _is_component_root(parent):
            parent_rel = parent.relative_to(root).as_posix()
            raise click.ClickException(
                f"no component is rooted at {rel_str}\nThe nearest enclosing component is {parent_rel}"
            )
        parent = parent.parent
    raise click.ClickException(f"no component is rooted at {rel_str}\nNo enclosing component found in the repository")


def _is_component_root(path: Path) -> bool:
    """True when a manifest file sits directly in `path`, making it a component root."""
    return any((path / name).exists() for name in git.MANIFEST_NAMES)


def _effective_classes_for_diff(classes: bool | None) -> bool:
    """Default for --classes in component diff/serve-diff mode: False (lean page)."""
    return classes if classes is not None else False


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


def _write_diff_stats(
    path: Path | None,
    *,
    diagram_type: str,
    fmt: str,
    content: str,
    changes: dict[str, int],
    omitted: int | None = None,
) -> None:
    """The verdict behind a diff, for a consumer that must not read the drawing.

    A CI script deciding between "post the diagram" and "say nothing" needs to
    know whether anything changed and whether the mermaid fits where it is
    going. Reading that off the diagram text couples the script to class names
    and glyphs that are free to change between releases; this file is the
    contract instead. `chars` is the rendered size; for mermaid the ceiling it
    is measured against comes along, so the consumer only has to compare.
    """
    if path is None:
        return
    stats: dict[str, object] = {
        "type": diagram_type,
        "format": fmt,
        "changed": any(changes.values()),
        "changes": changes,
        "chars": len(content),
    }
    if fmt == "mermaid":
        stats["mermaidLimit"] = managed.MERMAID_LIMIT
        stats["oversized"] = len(content) > managed.MERMAID_LIMIT
    if omitted is not None:
        stats["omitted"] = omitted
    path.write_text(json.dumps(stats, indent=2) + "\n", encoding="utf-8")


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

split_option = click.option(
    "--split",
    "split",
    multiple=True,
    metavar="DIR",
    help=(
        "Treat each direct child directory of DIR as a component of its own "
        "(repeatable). For a package that is one manifest but many subsystems. "
        "Spec: component.md §3.4."
    ),
)


def _split_paths(root: Path, split: tuple[str, ...], *, scope: str = "") -> list[str]:
    """Split directories as the core wants them: relative to `root`.

    Accepts each entry spelled relative to the root, relative to the scope
    (so a flag written for `component <scope>` means the same on `diff <scope>`,
    like `-E`), or as any path that resolves under the root.
    """
    out: list[str] = []
    for given in split:
        candidates = [Path(given)]
        if scope:
            candidates.append(Path(scope) / given)
        for candidate in candidates:
            target = candidate if candidate.is_absolute() else root / candidate
            if target.is_dir():
                try:
                    out.append(target.resolve().relative_to(root.resolve()).as_posix())
                except ValueError as exc:
                    raise click.ClickException(f"--split {given}: not under {root}") from exc
                break
        else:
            raise click.ClickException(f"--split {given}: no such directory under {root}")
    return out


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
        "--group-by",
        "group_by",
        type=click.Choice(GROUPINGS),
        default=None,
        help=(
            "Gather classes into namespace blocks (mermaid only): per module, "
            "per detected component, or not at all. [default: none]"
        ),
    ),
    click.option(
        "--group/--no-group",
        "group",
        default=False,
        help="Deprecated alias for --group-by module.",
    ),
    click.option("--externals", is_flag=True, help="Show inheritance edges to types outside the parsed set."),
    output_options,
)


class _CoreErrorsAreUserErrors(click.Group):
    """The Rust core reports every failure as a ValueError with the full anyhow
    context (an invalid glob, a selection that filters everything). Those are
    the user's inputs, so they come out as a one-line `Error:`, not a traceback.
    """

    def invoke(self, ctx: click.Context):
        try:
            return super().invoke(ctx)
        except ValueError as exc:
            raise click.ClickException(str(exc)) from exc


@click.group(cls=_CoreErrorsAreUserErrors)
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
    group_by: str | None,
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
        **_render_kwargs(members, modules, _grouping(group_by, group), externals, direction, title),
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
    oversized: list[Path] = []
    managed_count = 0
    written = 0
    for path in paths:
        try:
            doc = managed.read(path)
        except managed.ManagedDocError as err:
            raise click.ClickException(str(err)) from err
        if doc is None:
            continue
        managed_count += 1

        try:
            scope = managed.scope_of(doc.manifest)
            if scope is None:
                diagram = _core.curated_diagram_from_dir(
                    str(root), doc.manifest, include=list(include), exclude=list(exclude), langs=list(lang)
                )
            else:
                diagram = _core.class_diagram_from_dir(
                    str(root / scope.path),
                    include=list(include) + list(scope.include),
                    exclude=list(exclude) + list(scope.exclude),
                    langs=list(lang) or ([scope.lang] if scope.lang else []),
                    **_render_kwargs(scope.members, False, scope.group, False, scope.direction, None),
                )
            updated = doc.with_diagram(diagram)
        except (ValueError, managed.ManagedDocError) as err:
            raise click.ClickException(f"{path}: {err}") from err

        # A diagram past the ceiling is broken whether or not it drifted, so
        # this is checked before the equality test, not after it.
        if len(diagram) > managed.MERMAID_LIMIT:
            oversized.append(path)
        elif len(diagram) > managed.MERMAID_WARN:
            click.echo(
                f"{path}: {len(diagram):,} characters, "
                f"{managed.MERMAID_LIMIT - len(diagram):,} short of mermaid's "
                f"{managed.MERMAID_LIMIT:,} limit; narrow its scope before it crosses",
                err=True,
            )

        if updated == doc.text:
            continue
        if check:
            stale.append(path)
        else:
            path.write_text(updated, encoding="utf-8")
            click.echo(f"regenerated {path}", err=True)
            written += 1

    for path in oversized:
        click.echo(
            f"too large: {path} exceeds mermaid's {managed.MERMAID_LIMIT:,}-character "
            f"limit, which renders an error graphic instead of the diagram",
            err=True,
        )
    for path in stale:
        click.echo(f"out of date: {path}", err=True)
    if stale or oversized:
        raise click.ClickException(
            f"{len(stale)} document(s) need regenerating, {len(oversized)} too large; run `vizzle doc`"
            if oversized
            else f"{len(stale)} document(s) need regenerating; run `vizzle doc`"
        )
    if check:
        click.echo(f"{managed_count} managed document(s) checked, all current", err=True)
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
@split_option
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
    split: tuple[str, ...],
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
    splits = _split_paths(path, split)
    if resolved_fmt == "html":
        graph_json = _core.component_json_from_dir(
            str(path),
            include=list(include),
            exclude=list(exclude),
            langs=list(lang),
            splits=splits,
            classes=classes,
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
        splits=splits,
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


def _fork_point(path: Path, base: str, head: str | None) -> str:
    """Resolve `base` to where `head` (or the checked-out commit) forked from it.

    For a linear history this is `base` itself, so `--base HEAD~20` is
    unchanged. For a branch whose base has moved on, it is the merge base, so
    the diagram describes the branch and not the base's progress since.
    """
    root = _repo_root(path)
    return git.merge_base(root, base, head or "HEAD") or base


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
@click.option(
    "--classes/--no-classes",
    default=None,
    help="Embed class detail (component type, HTML only). Default: --no-classes for component type.",
)
@split_option
@click.option(
    "--focus",
    is_flag=True,
    help=(
        "Component type only: draw the changed components, the added/removed "
        "edges, and the unchanged neighbours those touch; leave the rest out and "
        "say how many. Spec: component.md §6.3."
    ),
)
@click.option(
    "--stats",
    "stats_path",
    type=click.Path(dir_okay=False, path_type=Path),
    default=None,
    help=(
        "Also write a JSON verdict to FILE: whether anything changed, counts of "
        "added/removed/modified elements, and the rendered size. For tooling that "
        "must not read the diagram text."
    ),
)
@select_options
@render_options
def diff_diagram(
    path: Path,
    base: str,
    head: str | None,
    diagram_type: str,
    weights: bool,
    classes: bool | None,
    split: tuple[str, ...],
    focus: bool,
    stats_path: Path | None,
    include: tuple[str, ...],
    exclude: tuple[str, ...],
    lang: tuple[str, ...],
    members: bool,
    modules: bool,
    group_by: str | None,
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
    that were added or removed. -I/-E/-l apply to BOTH revisions, so a
    filtered file never reads as added or removed. --stats FILE writes the
    verdict (changed, counts, size) as JSON beside the diagram.
    """
    # A diff renders two revisions held in memory, so there is no tree to
    # detect components in. Say so rather than silently emitting ungrouped output.
    grouping = _grouping(group_by, group)
    if grouping == "component":
        raise click.UsageError(
            "--group-by component is not available for a diff: component ownership "
            "comes from the package manifests on disk, and a diff renders two "
            "revisions held in memory. Use --group-by module."
        )
    selection = _selection_kwargs(include, exclude, lang)
    if diagram_type != "component" and (split or focus):
        raise click.UsageError("--split and --focus apply to --type component only")
    # Title keeps the reader's spelling of the base; the revisions compared
    # are the fork point and head.
    resolved_title = title or f"changes vs {base}"
    base = _fork_point(path, base, head)
    if diagram_type == "component":
        base_files, base_manifests, head_files, head_manifests = _collect_component_diff(path, base, head)
        scope_path = _component_scope_path(path)
        selection["splits"] = _split_paths(_repo_root(path), split, scope=scope_path)
        effective_classes = _effective_classes_for_diff(classes)
        resolved_format = _resolve_format(fmt, output)
        if resolved_format == "html":
            graph_json = _core.component_json_diff(
                base_files,
                base_manifests,
                head_files,
                head_manifests,
                classes=effective_classes,
                scope=scope_path,
                focus=focus,
                **selection,
            )
            page = build_component_html(graph_json, title=resolved_title, include_externals=externals)
            graph_stats = json.loads(graph_json)["stats"]
            _write_diff_stats(
                stats_path,
                diagram_type=diagram_type,
                fmt=resolved_format,
                content=page,
                changes=graph_stats["changes"],
                omitted=graph_stats["omitted"],
            )
            _emit(page, output, summarize_components(graph_json))
            return
        diagram, verdict_json = _core.component_diagram_diff(
            base_files,
            base_manifests,
            head_files,
            head_manifests,
            scope=scope_path,
            focus=focus,
            **selection,
            **_component_render_kwargs(True, weights, externals, direction, resolved_title),
        )
        verdict = json.loads(verdict_json)
        _write_diff_stats(
            stats_path,
            diagram_type=diagram_type,
            fmt=resolved_format,
            content=diagram,
            changes={k: verdict[k] for k in ("added", "removed", "modified")},
            omitted=verdict["omitted"],
        )
        _emit_mermaid(diagram, output)
        return

    base_files, head_files = _collect_diff_files(path, base, head)
    if not base_files and not head_files:
        raise click.ClickException(
            f"no changed Python/TypeScript files between {base} and {head or 'the working tree'}"
        )

    grouping = _grouping(group_by, group)
    if grouping == "component":
        raise click.UsageError(
            "--group-by component is not available for a diff: component ownership "
            "comes from the package manifests on disk, and a diff renders two "
            "revisions held in memory. Use --group-by module."
        )
    resolved_format = _resolve_format(fmt, output)
    if resolved_format == "html":
        graph_json = _core.graph_json_diff(base_files, head_files, **selection)
        page = build_html(
            graph_json,
            title=resolved_title,
            show_members=members,
            show_modules=modules,
            include_externals=externals,
        )
        _write_diff_stats(
            stats_path,
            diagram_type=diagram_type,
            fmt=resolved_format,
            content=page,
            changes=json.loads(graph_json)["stats"]["changes"],
        )
        _emit(page, output, summarize(graph_json, show_modules=modules))
        return

    diagram, verdict_json = _core.class_diagram_diff(
        base_files,
        head_files,
        **selection,
        **_render_kwargs(members, modules, grouping, externals, direction, resolved_title),
    )
    verdict = json.loads(verdict_json)
    _write_diff_stats(
        stats_path,
        diagram_type=diagram_type,
        fmt=resolved_format,
        content=diagram,
        changes={k: verdict[k] for k in ("added", "removed", "modified")},
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
@select_options
@click.option("--members/--no-members", default=True, show_default=True, help="Render fields and methods.")
@click.option(
    "--modules",
    is_flag=True,
    help="Add one «module» box per module holding its public module-level functions.",
)
@click.option("--externals", is_flag=True, help="Show inheritance edges to types outside the parsed set.")
@click.option(
    "--classes/--no-classes",
    default=None,
    help="Embed class detail (component type, diff mode). Default: --no-classes for component type.",
)
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
    classes: bool | None,
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

    selection = _selection_kwargs(include, exclude, lang)

    def build_page() -> str:
        if diagram_type == "component":
            if diff_mode:
                base_files, base_manifests, head_files, head_manifests = _collect_component_diff(
                    path, _fork_point(path, base, head), head
                )
                scope_path = _component_scope_path(path)
                effective_classes = _effective_classes_for_diff(classes)
                graph_json = _core.component_json_diff(
                    base_files,
                    base_manifests,
                    head_files,
                    head_manifests,
                    classes=effective_classes,
                    scope=scope_path,
                    **selection,
                )
                page_title = title or f"changes vs {base} (live)"
            else:
                effective_classes = classes if classes is not None else True
                graph_json = _core.component_json_from_dir(str(path), classes=effective_classes, **selection)
                page_title = title or f"{path.resolve().name} — component diagram (live)"
            return build_component_html(graph_json, title=page_title, include_externals=externals)
        if diff_mode:
            base_files, head_files = _collect_diff_files(path, _fork_point(path, base, head), head)
            graph_json = _core.graph_json_diff(base_files, head_files, **selection)
            page_title = title or f"changes vs {base} (live)"
        else:
            graph_json = _core.graph_json_from_dir(str(path), **selection)
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
