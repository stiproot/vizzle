"""Tests for the live-reload server."""

import threading
import time
import urllib.request
from pathlib import Path

from vizzy_cli import server


def test_inject_live_reload() -> None:
    page = "<html><body><p>hi</p></body></html>"
    injected = server.inject_live_reload(page)
    assert "EventSource" in injected
    assert injected.index("EventSource") < injected.index("</body>")


def test_hub_wait_times_out_without_change() -> None:
    hub = server.ReloadHub()
    generation = hub.generation
    assert hub.wait_for_change(generation, timeout=0.05) == generation


def test_hub_wakes_waiters_on_notify() -> None:
    hub = server.ReloadHub()
    seen: list[int] = []
    thread = threading.Thread(target=lambda: seen.append(hub.wait_for_change(hub.generation, timeout=5.0)))
    thread.start()
    time.sleep(0.05)
    hub.notify()
    thread.join(timeout=5.0)
    assert seen == [1]


def test_server_serves_page_and_pushes_reload_on_file_change(tmp_path: Path) -> None:
    (tmp_path / "app.py").write_text("class A:\n    pass\n")

    hub = server.ReloadHub()
    stop = threading.Event()
    httpd = server.make_server(lambda: "<html><body>ok</body></html>", hub, "127.0.0.1", 0)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    server.start_watcher(tmp_path, hub, stop)
    port = httpd.server_address[1]

    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=5) as response:
            body = response.read().decode()
        assert "ok" in body and "EventSource" in body

        events: list[bytes] = []

        def listen() -> None:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/events", timeout=10) as stream:
                events.append(stream.readline())

        listener = threading.Thread(target=listen, daemon=True)
        listener.start()
        time.sleep(0.5)  # let the watcher and the SSE subscription settle

        (tmp_path / "app.py").write_text("class A:\n    x: int\n")
        listener.join(timeout=10.0)
        assert events and events[0].startswith(b"data: reload")
    finally:
        stop.set()
        httpd.shutdown()
        httpd.server_close()
