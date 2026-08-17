"""Render mermaid sources to images via mermaid-cli.

The markdown and `.mmd` sources stay the truth — GitHub, IDEs and agent clients
render fences natively — so images are produced on demand and usually
gitignored. See docs/curated-diagrams.md §8.

Nothing is vendored: `mmdc` is resolved from PATH, else run ephemerally through
bunx or npx. This module is pure orchestration, which is why it lives in the CLI
and not the core.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

# Mermaid refuses a diagram past 50,000 characters and renders a small error
# graphic instead of failing, which is how a broken example sat committed in
# this repo unnoticed. A whole-repo class diagram is well past it.
CONFIG = {"maxTextSize": 5_000_000, "maxEdges": 20_000}

_BROWSERS = ("google-chrome", "google-chrome-stable", "chromium", "chromium-browser")


class RenderError(Exception):
    """mermaid-cli could not be run, or refused a source."""


def _mmdc() -> list[str]:
    if shutil.which("mmdc"):
        return ["mmdc"]
    if shutil.which("bun"):
        return ["bunx", "-p", "@mermaid-js/mermaid-cli", "mmdc"]
    if shutil.which("npx"):
        return ["npx", "-y", "-p", "@mermaid-js/mermaid-cli", "mmdc"]
    raise RenderError("no mmdc, bun or npx on PATH — install one, or `npm i -g @mermaid-js/mermaid-cli`")


def _browser_env() -> dict[str, str]:
    """Point puppeteer at a browser we already have, and skip its download.

    mermaid-cli pulls puppeteer, whose postinstall fetches a Chrome. When one is
    already present that download is redundant *and* a hard failure mode: it
    exits non-zero behind a proxy or a read-only cache and takes the render with
    it. Finding none leaves the defaults alone, so a genuine first run still
    provisions a browser normally.
    """
    env = dict(os.environ)
    if env.get("PUPPETEER_EXECUTABLE_PATH"):
        env["PUPPETEER_SKIP_DOWNLOAD"] = "true"
        return env
    cache = Path(env.get("PUPPETEER_CACHE_DIR") or Path.home() / ".cache" / "puppeteer")
    if cache.is_dir() and any(cache.glob("chrome*")):
        env["PUPPETEER_SKIP_DOWNLOAD"] = "true"
        return env
    for browser in _BROWSERS:
        found = shutil.which(browser)
        if found:
            env["PUPPETEER_EXECUTABLE_PATH"] = found
            env["PUPPETEER_SKIP_DOWNLOAD"] = "true"
            return env
    return env


def sources(src: Path) -> list[Path]:
    """The sources under `src`. A directory contributes its diagrams, not its README."""
    if src.is_file():
        return [src]
    found = sorted(
        p
        for p in src.iterdir()
        if p.is_file() and p.suffix in (".md", ".mmd") and p.name != "README.md"
    )
    if not found:
        raise RenderError(f"no diagram sources in {src}")
    return found


def render(src: Path, out_dir: Path, *, fmt: str = "png", scale: int = 2, background: str = "white") -> list[Path]:
    """Render every mermaid fence in `src` into `out_dir`, returning what was written."""
    out_dir.mkdir(parents=True, exist_ok=True)
    base = src.stem
    target = out_dir / f"{base}.{fmt}"

    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
        json.dump(CONFIG, handle)
        config = Path(handle.name)
    try:
        command = [
            *_mmdc(),
            "--quiet",
            "-c", str(config),
            "-i", str(src),
            "-o", str(target),
            "--scale", str(scale),
            "--backgroundColor", background,
        ]
        result = subprocess.run(command, env=_browser_env(), capture_output=True)
    finally:
        config.unlink(missing_ok=True)

    if result.returncode != 0:
        raise RenderError(
            f"mmdc failed for {src}\n{result.stderr.decode(errors='replace').strip()}\n"
            "If that was a puppeteer/Chrome error, install a browser once with\n"
            "  npx puppeteer browsers install chrome\n"
            "or point PUPPETEER_EXECUTABLE_PATH at an existing one."
        )

    # Markdown input emits one image per fence, suffixed -1, -2, … A single-fence
    # source — the norm for a managed document — gets the clean name back.
    numbered = sorted(out_dir.glob(f"{base}-[0-9]*.{fmt}"))
    if len(numbered) == 1:
        numbered[0].replace(target)
        return [target]
    return numbered or ([target] if target.exists() else [])
