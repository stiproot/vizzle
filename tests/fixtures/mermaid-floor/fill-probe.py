"""Print the drawn fill of named boxes in a classDiagram, for one Mermaid version or more.

Why this exists: whose colour a box shows is decided by the browser, so comparing two
outputs means reading the computed fill, not the colour strings (the stylesheet holds
every colour either way). Manual, like scripts/check-mermaid-floor.sh: needs Chrome and
a network (Mermaid from jsdelivr).

    python3 tests/fixtures/mermaid-floor/fill-probe.py tests/fixtures/mermaid-floor/zoom.mmd id1,id2 10.7.0 11.17.2

Set CHROME to the Chrome binary when it is not at the macOS default.
"""

import html
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

CHROME = os.environ.get("CHROME", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")


def probe(version, text, ids):
    page = f"""<!doctype html><html><head><meta charset="utf-8"></head><body>
<div id="c"></div><pre id="out">PENDING</pre>
<script type="module">
import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@{version}/dist/mermaid.esm.min.mjs';
const out = document.getElementById('out');
try {{
  mermaid.initialize({{startOnLoad:false}});
  const x = await mermaid.render('g', {json.dumps(text)});
  if (/Syntax error in/.test(x.svg)) throw new Error('error-svg');
  document.getElementById('c').innerHTML = x.svg;
  const res = {{}};
  for (const id of {json.dumps(ids)}) {{
    // Node ids end `-<id>-<n>`; vizzle ids have no `-`, so this match is exact.
    const g = [...document.querySelectorAll('#c g')]
      .find(n => (n.id || '').replace(/-\\d+$/, '').endsWith('-' + id));
    const shape = g && (g.querySelector('rect, path, polygon'));
    res[id] = shape ? getComputedStyle(shape).fill + " " + getComputedStyle(shape).strokeDasharray : 'NOT-FOUND';
  }}
  out.textContent = JSON.stringify(res);
}} catch (e) {{ out.textContent = 'ERR ' + (e.message || e); }}
</script></body></html>"""
    with tempfile.NamedTemporaryFile("w", suffix=".html", delete=False) as f:
        f.write(page)
        p = f.name
    dom = subprocess.run(
        [CHROME, "--headless=new", "--disable-gpu", "--virtual-time-budget=40000", "--dump-dom", "file://" + p],
        capture_output=True,
        text=True,
        timeout=150,
    ).stdout
    m = re.search(r'<pre id="out">(.*?)</pre>', dom, re.S)
    return html.unescape(m.group(1)).strip()[:400] if m else "NO-DOM"


if __name__ == "__main__":
    text = pathlib.Path(sys.argv[1]).read_text()
    ids = sys.argv[2].split(",")
    for v in sys.argv[3:]:
        print(f"{v:8} {probe(v, text, ids)}", flush=True)
