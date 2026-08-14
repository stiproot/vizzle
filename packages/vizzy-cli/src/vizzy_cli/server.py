"""Lightweight live-reload server: stdlib http.server + SSE + watchfiles.

`vizzy serve` regenerates the diagram on every page load and pushes a
reload event to connected browsers whenever a watched source file changes.
"""

from __future__ import annotations

import threading
from collections.abc import Callable
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from .git import SUPPORTED_SUFFIXES

LIVE_RELOAD_SNIPPET = (
    '<script>new EventSource("/events").addEventListener("message", () => location.reload());</script>'
)


class ReloadHub:
    """Fan-out of change notifications to any number of waiting SSE handlers."""

    def __init__(self) -> None:
        self._cond = threading.Condition()
        self._generation = 0

    @property
    def generation(self) -> int:
        with self._cond:
            return self._generation

    def notify(self) -> None:
        with self._cond:
            self._generation += 1
            self._cond.notify_all()

    def wait_for_change(self, seen_generation: int, timeout: float) -> int:
        """Block until the generation moves past `seen_generation` (or timeout)."""
        with self._cond:
            self._cond.wait_for(lambda: self._generation != seen_generation, timeout)
            return self._generation


def inject_live_reload(page: str) -> str:
    if "</body>" in page:
        return page.replace("</body>", f"{LIVE_RELOAD_SNIPPET}\n</body>", 1)
    return page + LIVE_RELOAD_SNIPPET


def make_server(
    build_page: Callable[[], str],
    hub: ReloadHub,
    host: str,
    port: int,
) -> ThreadingHTTPServer:
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 (http.server API)
            if self.path == "/events":
                self._serve_events()
            elif self.path == "/":
                self._serve_page()
            else:
                self.send_error(404)

        def _serve_page(self) -> None:
            body = inject_live_reload(build_page()).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def _serve_events(self) -> None:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            generation = hub.generation
            try:
                while True:
                    new_generation = hub.wait_for_change(generation, timeout=15.0)
                    if new_generation != generation:
                        generation = new_generation
                        self.wfile.write(b"data: reload\n\n")
                    else:
                        self.wfile.write(b": keep-alive\n\n")
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                return  # browser navigated away or reloaded

        def log_message(self, fmt: str, *args) -> None:  # quiet the request log
            pass

    return ThreadingHTTPServer((host, port), Handler)


def is_watched_source(path: str) -> bool:
    return path.endswith(SUPPORTED_SUFFIXES) and not path.endswith(".d.ts")


def start_watcher(root: Path, hub: ReloadHub, stop: threading.Event) -> threading.Thread:
    """Watch `root` for source changes on a daemon thread; notify `hub`."""
    from watchfiles import watch

    def run() -> None:
        for changes in watch(root, stop_event=stop, debounce=200):
            if any(is_watched_source(changed_path) for _, changed_path in changes):
                hub.notify()

    thread = threading.Thread(target=run, daemon=True, name="vizzy-watcher")
    thread.start()
    return thread


def serve(
    build_page: Callable[[], str],
    watch_root: Path,
    host: str,
    port: int,
    on_ready: Callable[[str], None] | None = None,
) -> None:
    """Run the server until interrupted. `on_ready` receives the URL."""
    hub = ReloadHub()
    stop = threading.Event()
    start_watcher(watch_root, hub, stop)
    server = make_server(build_page, hub, host, port)
    try:
        if on_ready is not None:
            actual_port = server.server_address[1]
            on_ready(f"http://{host}:{actual_port}/")
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        stop.set()
        server.server_close()
