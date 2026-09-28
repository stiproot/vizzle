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

# The mermaid-cli fetched through bunx/npx when `mmdc` is not on PATH. Pinned: an unpinned
# fetch follows every major, and 12.0.0 (2026-09) removed `-w`, which broke a consumer's
# render script overnight. Move it after a render of a class and a sequence diagram passes
# in both themes.
MERMAID_CLI = "@mermaid-js/mermaid-cli@12.0.0"

# What a reader in each theme sees: mermaid's own theme, on the page background GitHub
# draws behind a fence (the dark value is GitHub's dark canvas).
THEMES = {"light": ("default", "white"), "dark": ("dark", "#0d1117")}

_BROWSERS = ("google-chrome", "google-chrome-stable", "chromium", "chromium-browser")


class RenderError(Exception):
    """mermaid-cli could not be run, or refused a source."""


def _mmdc() -> list[str]:
    if shutil.which("mmdc"):
        return ["mmdc"]
    if shutil.which("bun"):
        return ["bunx", "-p", MERMAID_CLI, "mmdc"]
    if shutil.which("npx"):
        return ["npx", "-y", "-p", MERMAID_CLI, "mmdc"]
    raise RenderError(f"no mmdc, bun or npx on PATH — install one, or `npm i -g {MERMAID_CLI}`")


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
    cached = _cached_browser(cache)
    if cached:
        env["PUPPETEER_EXECUTABLE_PATH"] = str(cached)
        env["PUPPETEER_SKIP_DOWNLOAD"] = "true"
        return env
    for browser in _BROWSERS:
        found = shutil.which(browser)
        if found:
            env["PUPPETEER_EXECUTABLE_PATH"] = found
            env["PUPPETEER_SKIP_DOWNLOAD"] = "true"
            return env
    return env


# Where `puppeteer browsers install` puts an executable, per browser: headless-shell first,
# because it is what mermaid-cli launches by default.
_CACHED_BROWSERS = (
    "chrome-headless-shell/*/*/chrome-headless-shell",
    "chrome-headless-shell/*/*/chrome-headless-shell.exe",
    "chrome/*/*/chrome",
    "chrome/*/*/chrome.exe",
    "chrome/*/*/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
)


def _cached_browser(cache: Path) -> Path | None:
    """The newest browser already in puppeteer's cache, whatever version it is.

    Skipping the download is not enough on its own: puppeteer then looks for the exact
    build its own release pins, and a cache holding any other build fails the render with
    "Could not find chrome-headless-shell". Pointing it at the cached executable works
    across versions.
    """

    def version(path: Path) -> tuple[int, ...]:
        release = path.relative_to(cache).parts[1].rpartition("-")[2]
        return tuple(int(part) for part in release.split(".") if part.isdigit())

    for pattern in _CACHED_BROWSERS:
        found = [p for p in cache.glob(pattern) if p.is_file() and os.access(p, os.X_OK)]
        if found:
            return max(found, key=version)
    return None


def sources(src: Path) -> list[Path]:
    """The sources under `src`. A directory contributes its diagrams, not its README."""
    if src.is_file():
        return [src]
    found = sorted(p for p in src.iterdir() if p.is_file() and p.suffix in (".md", ".mmd") and p.name != "README.md")
    if not found:
        raise RenderError(f"no diagram sources in {src}")
    return found


def render(
    src: Path,
    out_dir: Path,
    *,
    fmt: str = "png",
    scale: int = 2,
    background: str | None = None,
    theme: str = "light",
) -> list[Path]:
    """Render every mermaid fence in `src` into `out_dir`, returning what was written.

    A dark render is written beside the light one as `<name>.dark.<fmt>`, so both can be
    looked at: a fence on GitHub is drawn in the reader's theme, and a diagram that reads
    well on white can be unreadable on dark. `background` overrides the theme's page colour.
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    mermaid_theme, default_background = THEMES[theme]
    suffix = "" if theme == "light" else f".{theme}"
    base = f"{src.stem}{suffix}"
    target = out_dir / f"{base}.{fmt}"

    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
        json.dump(CONFIG, handle)
        config = Path(handle.name)
    try:
        command = [
            *_mmdc(),
            "--quiet",
            "-c",
            str(config),
            "-i",
            str(src),
            "-o",
            str(target),
            "--theme",
            mermaid_theme,
            "--scale",
            str(scale),
            "--backgroundColor",
            background or default_background,
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
    if suffix:
        # mmdc numbers after the whole stem (`doc.dark-1.png`); keep the theme last
        # (`doc-1.dark.png`) so each dark image sorts beside its light twin.
        renamed = []
        for path in numbered:
            fence = path.name[len(base) : -len(f".{fmt}")]
            renamed.append(path.replace(out_dir / f"{src.stem}{fence}{suffix}.{fmt}"))
        numbered = renamed
    return numbered or ([target] if target.exists() else [])
