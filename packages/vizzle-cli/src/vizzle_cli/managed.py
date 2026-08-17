"""Managed diagram documents: a gen:c4-code manifest plus one generated fence.

The core renders the diagram; this module knows what a document looks like —
where the manifest is, which fence to replace, and what to leave alone. See
docs/curated-diagrams.md.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

MARKER = "gen:c4-code"

# The manifest is JSON inside an HTML comment. Non-greedy up to the first `-->`,
# which is why the format forbids `--` anywhere inside the JSON.
_MANIFEST = re.compile(r"<!--\s*" + MARKER + r"\s*(?P<json>\{.*?\})\s*-->", re.DOTALL)
# The generated fence is the first mermaid block after the manifest.
_FENCE = re.compile(r"(?P<open>```mermaid\n)(?P<body>.*?)(?P<close>```)", re.DOTALL)


class ManagedDocError(Exception):
    """A document carries the marker but is not shaped like a managed doc."""


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
