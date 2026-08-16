"""vizzle: UML visualization for git — Mermaid class diagrams with diff highlighting."""

from importlib.metadata import PackageNotFoundError, version

try:
    # Read the installed metadata rather than restating the version here; the
    # packaging files are the one place it is declared.
    __version__ = version("vizzle")
except PackageNotFoundError:  # running from a source tree that was never installed
    __version__ = "0.0.0+unknown"
