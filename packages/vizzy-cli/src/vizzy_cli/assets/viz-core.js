/* Shared d3 plumbing for every vizzy HTML view.
 *
 * Diagram templates own what a node looks like; everything here is the part
 * that is identical whether you are reading classes or components — the
 * palette, box geometry, zoom/pan/fit with a viewport that survives reloads,
 * the filter box, and the header readout.
 *
 * Exposed as `window.vizzy`.
 */
"use strict";
(function () {
  const MARK = { added: " ✚", removed: " ✖", modified: " ✱", unchanged: "" };

  const PALETTE = {
    added: { fill: "var(--added-fill)", stroke: "var(--added-stroke)" },
    removed: { fill: "var(--removed-fill)", stroke: "var(--removed-stroke)" },
    modified: { fill: "var(--modified-fill)", stroke: "var(--modified-stroke)" },
    unchanged: { fill: "var(--box-fill)", stroke: "var(--box-stroke)" },
    // In diff mode an unchanged element is context, not a subject.
    context: { fill: "var(--context-fill)", stroke: "var(--context-stroke)" },
    external: { fill: "var(--panel)", stroke: "var(--muted)" },
  };

  /* Colors for one element. Under the diff lens unchanged elements recede,
   * so the eye lands on what actually changed. */
  function colorsFor(change, diffMode) {
    if (change && change !== "unchanged") return PALETTE[change];
    return diffMode ? PALETTE.context : PALETTE.unchanged;
  }

  function inkFor(change, diffMode) {
    if (change && change !== "unchanged") return PALETTE[change].stroke;
    return diffMode ? "var(--context-ink)" : "var(--ink)";
  }

  function mark(change) {
    return MARK[change] || "";
  }

  function truncate(text, chars) {
    return text.length > chars ? text.slice(0, Math.max(1, Math.floor(chars) - 1)) + "…" : text;
  }

  /* Where an edge between two boxes meets the boundary of `node`. */
  function edgePoint(node, other) {
    const dx = other.x - node.x;
    const dy = other.y - node.y;
    if (dx === 0 && dy === 0) return { x: node.x, y: node.y };
    const sx = node.w / 2 / Math.abs(dx || 1e-9);
    const sy = node.h / 2 / Math.abs(dy || 1e-9);
    const s = Math.min(sx, sy);
    return { x: node.x + dx * s, y: node.y + dy * s };
  }

  /* Lay out `keys` on a square-ish grid of cells, for seeding a simulation
   * so related things start near each other. */
  function gridCells(keys, cell) {
    const span = Math.ceil(Math.sqrt(keys.length)) || 1;
    return new Map(
      keys.map((key, i) => [
        key,
        {
          x: ((i % span) - (span - 1) / 2) * cell,
          y: (Math.floor(i / span) - (span - 1) / 2) * cell,
        },
      ])
    );
  }

  /* A compact arrowhead marker. The default d3/mermaid heads read as oversized
   * once a graph has more than a handful of edges. */
  function arrowMarker(id, stroke, width = 6) {
    return `<marker id="${id}" viewBox="0 0 10 10" refX="9" refY="5"
      markerWidth="${width}" markerHeight="${width}" orient="auto-start-reverse">
      <path d="M1.5,1.5 L9,5 L1.5,8.5" fill="none" stroke="${stroke}" stroke-width="1.6"
        stroke-linecap="round" stroke-linejoin="round"/>
    </marker>`;
  }

  /* Zoom, pan, and fit-to-view. The viewport is remembered per page so a
   * live-reload edit (or F5) does not throw away where you were looking.
   *
   * Returns `{ zoom, fit, focus }`; `focus(box)` frames one region — used when
   * opening a detail view, so the thing you just opened is the thing you see. */
  function attachViewport(svg, view, { fitButton } = {}) {
    const key = `vizzy-view:${location.pathname}:${document.title}`;
    const zoom = d3
      .zoom()
      .scaleExtent([0.02, 6])
      .on("zoom", (event) => view.attr("transform", event.transform))
      .on("end", (event) => {
        const { k, x, y } = event.transform;
        try {
          sessionStorage.setItem(key, JSON.stringify({ k, x, y }));
        } catch {}
      });
    svg.call(zoom);

    function fit() {
      const box = view.node().getBBox();
      const { width, height } = svg.node().getBoundingClientRect();
      if (!box.width || !box.height) return;
      const k = Math.min(width / (box.width + 80), height / (box.height + 80), 1.5);
      const tx = width / 2 - k * (box.x + box.width / 2);
      const ty = height / 2 - k * (box.y + box.height / 2);
      svg.transition().duration(300).call(zoom.transform, d3.zoomIdentity.translate(tx, ty).scale(k));
    }

    /* Frame `{x, y, w, h}` in graph coordinates, never zooming further out
     * than the caller already is — opening a detail should not shrink the view. */
    function focus(box, { maxScale = 1.1, padding = 90, duration = 400 } = {}) {
      const { width, height } = svg.node().getBoundingClientRect();
      if (!box.w || !box.h) return;
      const current = d3.zoomTransform(svg.node()).k;
      const k = Math.min(width / (box.w + padding), height / (box.h + padding), maxScale);
      const scale = Math.max(k, Math.min(current, maxScale));
      const tx = width / 2 - scale * box.x;
      const ty = height / 2 - scale * box.y;
      svg.transition().duration(duration)
        .call(zoom.transform, d3.zoomIdentity.translate(tx, ty).scale(scale));
    }

    if (fitButton) document.getElementById(fitButton).addEventListener("click", fit);

    const saved = (() => {
      try {
        return JSON.parse(sessionStorage.getItem(key));
      } catch {
        return null;
      }
    })();
    if (saved && [saved.k, saved.x, saved.y].every(Number.isFinite)) {
      svg.call(zoom.transform, d3.zoomIdentity.translate(saved.x, saved.y).scale(saved.k));
    } else {
      requestAnimationFrame(fit);
    }
    return { zoom, fit, focus };
  }

  /* Dim whatever does not match the filter, rather than removing it, so the
   * shape of the graph stays readable while you search. */
  function attachFilter(inputId, { nodes, edges, matches }) {
    const input = document.getElementById(inputId);
    if (!input) return;
    input.addEventListener("input", (event) => {
      const query = event.target.value.trim().toLowerCase();
      nodes.attr("opacity", (d) => (!query || matches(d, query) ? 1 : 0.12));
      if (edges) {
        edges.attr("opacity", (d) =>
          !query || matches(d.source, query) || matches(d.target, query) ? 1 : 0.06
        );
      }
    });
  }

  function setHeader(statsText, diffMode) {
    const stats = document.getElementById("stats");
    if (stats) stats.textContent = statsText;
    const legend = document.getElementById("legend");
    if (legend && diffMode) legend.hidden = false;
  }

  window.vizzy = {
    MARK,
    PALETTE,
    colorsFor,
    inkFor,
    mark,
    truncate,
    edgePoint,
    gridCells,
    arrowMarker,
    attachViewport,
    attachFilter,
    setHeader,
  };
})();
