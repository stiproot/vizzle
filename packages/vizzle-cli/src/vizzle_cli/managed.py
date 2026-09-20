"""Managed diagram documents: a gen:c4-code manifest plus one generated fence.

The core renders the diagram; this module knows what a document looks like —
where the manifest is, which fence to replace, and what to leave alone. See
docs/curated-diagrams.md.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path

MARKER = "gen:c4-code"

# Mermaid stops laying a diagram out past this many characters and renders an
# error graphic in its place. A generated document that crosses it therefore
# fails in the one way nobody reports: it looks rendered, and it is wrong. The
# warning band exists so a repository hears about it while there is still room
# to narrow the scope.
MERMAID_LIMIT = 50_000
MERMAID_WARN = 45_000

# The manifest is JSON inside an HTML comment. Non-greedy up to the first `-->`,
# which is why the format forbids `--` anywhere inside the JSON.
_MANIFEST = re.compile(r"<!--\s*" + MARKER + r"\s*(?P<json>\{.*?\})\s*-->", re.DOTALL)
# The generated fence is the first mermaid block after the manifest.
_FENCE = re.compile(r"(?P<open>```mermaid\n)(?P<body>.*?)(?P<close>```)", re.DOTALL)


class ManagedDocError(Exception):
    """A document carries the marker but is not shaped like a managed doc."""


@dataclass(frozen=True)
class Scope:
    """A path-derived diagram: every class under `path`, not a chosen few.

    The counterpart to a curated `classes` list. A curated manifest cannot
    catch an addition, because a class absent from the manifest is absent from
    the diagram and the check reports current. A scope can, which is what makes
    it the right shape for a gate rather than for a design document.
    """

    path: str
    lang: str | None = None
    group: str = "none"
    members: bool = True
    direction: str | None = None
    include: tuple[str, ...] = ()
    exclude: tuple[str, ...] = ()


def scope_of(manifest: str) -> Scope | None:
    """The manifest's `scope`, or None when it curates a `classes` list.

    Raises if it carries both: that would silently describe two diagrams.
    """
    try:
        data = json.loads(manifest)
    except json.JSONDecodeError as err:
        raise ManagedDocError(f"manifest is not valid JSON: {err}") from err
    if not isinstance(data, dict):
        raise ManagedDocError("manifest must be a JSON object")

    raw = data.get("scope")
    if raw is None:
        return None
    if "classes" in data:
        raise ManagedDocError(
            "manifest carries both `scope` and `classes`; they describe different diagrams, so pick one"
        )
    if not isinstance(raw, dict) or "path" not in raw:
        raise ManagedDocError("`scope` must be an object with a `path`")

    unknown = set(raw) - {"path", "lang", "group", "members", "include", "exclude"}
    if unknown:
        raise ManagedDocError(f"unknown `scope` key(s): {', '.join(sorted(unknown))}")

    return Scope(
        path=raw["path"],
        lang=raw.get("lang"),
        group=raw.get("group", "none"),
        members=raw.get("members", True),
        direction=data.get("direction"),
        include=tuple(raw.get("include", ())),
        exclude=tuple(raw.get("exclude", ())),
    )


@dataclass(frozen=True)
class ManagedDoc:
    path: Path
    text: str
    manifest: str

    def with_diagram(self, diagram: str) -> str:
        """The document with its fence replaced and everything else untouched."""
        match = _FENCE.search(self.text, self.manifest_end)
        if not match:
            raise ManagedDocError(f"{self.path}: no ```mermaid fence after the manifest")
        body = diagram if diagram.endswith("\n") else diagram + "\n"
        return self.text[: match.start("body")] + body + self.text[match.end("body") :]

    @property
    def manifest_end(self) -> int:
        match = _MANIFEST.search(self.text)
        assert match is not None  # only constructed from a matching document
        return match.end()


def read(path: Path) -> ManagedDoc | None:
    """Parse a managed document, or None if it is not one.

    A document without the marker is somebody's hand-authored diagram and is
    none of our business — a directory mixing both is expected.
    """
    text = path.read_text(encoding="utf-8")
    match = _MANIFEST.search(text)
    if not match:
        if MARKER in text:
            raise ManagedDocError(f"{path}: has a {MARKER} marker but no readable JSON manifest")
        return None
    return ManagedDoc(path=path, text=text, manifest=match.group("json"))


def discover(directory: Path) -> list[Path]:
    """Every markdown file under `directory`, sorted so output is diffable."""
    return sorted(p for p in directory.rglob("*.md") if p.is_file())
