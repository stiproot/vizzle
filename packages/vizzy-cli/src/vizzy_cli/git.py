"""Thin wrapper around the git CLI used by `vizzy diff`."""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path

SUPPORTED_SUFFIXES = (".py", ".ts", ".tsx", ".mts", ".cts")


class GitError(RuntimeError):
    pass


def _run(args: list[str], cwd: Path) -> bytes:
    result = subprocess.run(["git", *args], cwd=cwd, capture_output=True)
    if result.returncode != 0:
        raise GitError(result.stderr.decode(errors="replace").strip() or f"git {args[0]} failed")
    return result.stdout


def repo_root(path: Path) -> Path:
    out = _run(["rev-parse", "--show-toplevel"], cwd=path if path.is_dir() else path.parent)
    return Path(out.decode().strip())


@dataclass
class ChangedFile:
    status: str  # A, M, D, R, C, T
    path: str  # current (head-side) path; for D, the base-side path
    old_path: str | None = None  # set for renames/copies


def changed_files(root: Path, base: str, head: str | None, pathspec: str | None) -> list[ChangedFile]:
    """`git diff --name-status` between base and head (worktree if head is None)."""
    args = ["diff", "--name-status", "-M", "-z", base]
    if head:
        args.append(head)
    if pathspec:
        args += ["--", pathspec]
    out = _run(args, cwd=root)

    fields = [f.decode(errors="replace") for f in out.split(b"\0") if f]
    changes: list[ChangedFile] = []
    i = 0
    while i < len(fields):
        status = fields[i]
        if status.startswith(("R", "C")):
            changes.append(ChangedFile(status=status[0], path=fields[i + 2], old_path=fields[i + 1]))
            i += 3
        else:
            changes.append(ChangedFile(status=status[0], path=fields[i + 1]))
            i += 2
    return [c for c in changes if _supported(c.path) or (c.old_path and _supported(c.old_path))]


def _supported(path: str) -> bool:
    return path.endswith(SUPPORTED_SUFFIXES) and not path.endswith(".d.ts")


def file_at_ref(root: Path, ref: str, path: str) -> str | None:
    """Contents of `path` at `ref`, or None if it does not exist there."""
    try:
        return _run(["show", f"{ref}:{path}"], cwd=root).decode(errors="replace")
    except GitError:
        return None


def file_in_worktree(root: Path, path: str) -> str | None:
    try:
        return (root / path).read_text(errors="replace")
    except OSError:
        return None
