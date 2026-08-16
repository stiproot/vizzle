/* Shared d3 plumbing for every vizzle HTML view.
 *
 * Diagram templates own what a node looks like; everything here is the part
 * that is identical whether you are reading classes or components — the
 * palette, box geometry, zoom/pan/fit with a viewport that survives reloads,
 * the filter box, and the header readout.
 *
 * Exposed as `window.vizzle`.
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

  /* ---- UML class box ---------------------------------------------------
   * One renderer, used by the class diagram and by a component exploded into
   * its classes, so a class looks the same wherever you meet it. */

  const BOX = { charWidth: 7.25, rowH: 16, padX: 10, headerH: 34, sep: 6, maxWidth: 460 };

  /* `+name: Type`, `-run(arg: T) Ret`, with UML classifiers. */
  function memberText(m) {
    const signature = m.isMethod
      ? `${m.name}(${m.detail})`
      : m.detail
        ? `${m.name}: ${m.detail}`
        : m.name;
    const returns = m.isMethod && m.returns ? ` ${m.returns}` : "";
    const classifier = m.isAbstract ? "*" : m.isStatic ? "$" : "";
    return `${m.visibility}${signature}${classifier}${returns}${mark(m.change)}`;
  }

  /* Size a class box to its contents; `scale` shrinks it for nested use. */
  function classBoxLayout(cls, { showMembers = true, showModule = true, scale = 1, maxWidth } = {}) {
    const charWidth = BOX.charWidth * scale;
    const rowH = BOX.rowH * scale;
    const members = showMembers ? cls.members || [] : [];
    const fields = members.filter((m) => !m.isMethod);
    const methods = members.filter((m) => m.isMethod);
    const rows = [...fields, ...methods];
    const header = cls.name + mark(cls.change);
    const widest = Math.max(
      header.length + 4,
      showModule && cls.module ? cls.module.length : 0,
      ...rows.map((m) => memberText(m).length),
      12
    );
    const w = Math.min(maxWidth || BOX.maxWidth, widest * charWidth + BOX.padX * 2);
    const h =
      BOX.headerH * scale +
      (cls.annotation ? 14 * scale : 0) +
      (fields.length ? BOX.sep + fields.length * rowH : 0) +
      (methods.length ? BOX.sep + methods.length * rowH : 0) +
      (rows.length ? 6 : 2);
    return { w, h, fields, methods, scale, showModule, charWidth, rowH };
  }

  /* Draw the box into `g`, which is assumed empty. */
  function drawClassBox(g, cls, layout, { diff = false, external = false } = {}) {
    const { w, h, fields, methods, scale, showModule, charWidth, rowH } = layout;
    const colors = external ? PALETTE.external : colorsFor(cls.change, diff);
    const ink = external ? "var(--muted)" : inkFor(cls.change, diff);
    const font = (size) => size * scale;

    g.append("title").text(cls.qualified || cls.name);
    g.append("rect")
      .attr("width", w)
      .attr("height", h)
      .attr("rx", 6 * scale)
      .attr("fill", colors.fill)
      .attr("stroke", colors.stroke)
      .attr("stroke-width", cls.change === "unchanged" && !external ? 1.2 : 2)
      .attr("stroke-dasharray", cls.change === "removed" || external ? "6 4" : null);

    let y = 15 * scale;
    if (external || cls.annotation) {
      g.append("text")
        .attr("x", w / 2).attr("y", y).attr("text-anchor", "middle")
        .attr("font-size", font(10.5)).attr("fill", "var(--muted)")
        .text(`«${external ? "external" : cls.annotation}»`);
      y += 14 * scale;
    }
    g.append("text")
      .attr("x", w / 2).attr("y", y).attr("text-anchor", "middle")
      .attr("font-size", font(12.5)).attr("font-weight", 700).attr("fill", ink)
      .text(cls.name + mark(cls.change));
    y += 13 * scale;
    if (showModule && cls.module) {
      g.append("text")
        .attr("x", w / 2).attr("y", y).attr("text-anchor", "middle")
        .attr("font-size", font(9.5)).attr("fill", "var(--muted)")
        .text(cls.module);
      y += 6 * scale;
    }

    for (const section of [fields, methods]) {
      if (!section.length) continue;
      y += 3;
      g.append("line")
        .attr("x1", 0).attr("x2", w).attr("y1", y).attr("y2", y)
        .attr("stroke", colors.stroke).attr("stroke-width", 0.8);
      y += 3;
      for (const m of section) {
        y += rowH - 3 * scale;
        g.append("text")
          .attr("x", BOX.padX).attr("y", y)
          .attr("font-size", font(11.5))
          .attr("fill", inkFor(m.change, diff))
          .attr("font-style", m.isAbstract ? "italic" : null)
          .attr("text-decoration", m.change === "removed" ? "line-through" : null)
          .text(truncate(memberText(m), (w - BOX.padX * 2) / charWidth));
        y += 3 * scale;
      }
    }
  }

  /* Stroke style per relation kind, matching UML convention: inheritance and
   * association solid, implements and dependency dashed. */
  function relationStyle(kind) {
    return {
      inherits: { dash: null, marker: "hollow" },
      implements: { dash: "6 4", marker: "hollow" },
      association: { dash: null, marker: "open" },
      dependency: { dash: "4 3", marker: "open" },
    }[kind] || { dash: null, marker: "open" };
  }

  /* Nodes carry a centre (x, y) and a size (w, h); SVG groups are positioned
   * by their top-left corner. One conversion, used everywhere. */
  function nodeTransform(d) {
    return `translate(${d.x - d.w / 2},${d.y - d.h / 2})`;
  }

  /* Straight edge between two boxes, clipped to both boundaries. */
  function linkPath(d) {
    const a = edgePoint(d.source, d.target);
    const b = edgePoint(d.target, d.source);
    return `M${a.x},${a.y}L${b.x},${b.y}`;
  }

  /* Drag a node by moving its centre. `onMove` lets a view update whatever
   * else depends on the position (edges, group boxes). */
  function draggableNodes(selection, { onMove } = {}) {
    return selection.call(
      d3.drag().on("drag", function (event, d) {
        d.x += event.dx;
        d.y += event.dy;
        d3.select(this).attr("transform", nodeTransform(d));
        if (onMove) onMove(d);
      })
    );
  }

  /* A stub node for something outside the parsed set (an npm/PyPI package, an
   * unresolved base type). `payloadKey` is what the view calls its datum. */
  function externalStub(name, { charWidth, padX, payloadKey, height = 30 }) {
    const w = Math.max(name.length + 4, 10) * charWidth + padX * 2;
    return {
      id: `ext:${name}`,
      [payloadKey]: { name, path: "", group: "", module: "", members: [], change: "unchanged" },
      w,
      h: height,
      baseW: w,
      external: true,
    };
  }

  /* Resolve `{from, to, external}` records into d3 links, dropping any whose
   * endpoints are not on the canvas. `extra` copies through per-view fields. */
  function buildLinks(records, byId, includeExternals, extra = () => ({})) {
    return records
      .filter((r) => (r.external ? includeExternals : true))
      .map((r) => ({
        source: r.from,
        target: r.external ? `ext:${r.to}` : r.to,
        ...extra(r),
      }))
      .filter((l) => byId.has(l.source) && byId.has(l.target));
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
  function arrowMarker(id, stroke, width = 6, shape = "open") {
    const head =
      shape === "hollow"
        ? `<path d="M1,1 L9,5 L1,9 Z" fill="var(--bg)" stroke="${stroke}" stroke-width="1.4"/>`
        : `<path d="M1.5,1.5 L9,5 L1.5,8.5" fill="none" stroke="${stroke}" stroke-width="1.6"
             stroke-linecap="round" stroke-linejoin="round"/>`;
    return `<marker id="${id}" viewBox="0 0 10 10" refX="9" refY="5"
      markerWidth="${width}" markerHeight="${width}" orient="auto-start-reverse">${head}</marker>`;
  }

  /* Zoom, pan, and fit-to-view. The viewport is remembered per page so a
   * live-reload edit (or F5) does not throw away where you were looking.
   *
   * Returns `{ zoom, fit, focus }`; `focus(box)` frames one region — used when
   * opening a detail view, so the thing you just opened is the thing you see. */
  function attachViewport(svg, view, { fitButton } = {}) {
    const key = `vizzle-view:${location.pathname}:${document.title}`;
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

  /* Stats readout, plus the diff legend — built here so every view labels the
   * change colors identically, and only when there is a diff to explain. */
  function setHeader(statsText, diffMode) {
    const stats = document.getElementById("stats");
    if (stats) stats.textContent = statsText;
    const legend = document.getElementById("legend");
    if (!legend || !diffMode) return;
    legend.innerHTML = ["added", "removed", "modified"]
      .map(
        (change) =>
          `<span><span class="chip" style="background:var(--${change}-fill);` +
          `border-color:var(--${change}-stroke)"></span>${change}</span>`
      )
      .join("");
    legend.hidden = false;
  }

  window.vizzle = {
    PALETTE,
    BOX,
    colorsFor,
    inkFor,
    mark,
    truncate,
    memberText,
    classBoxLayout,
    drawClassBox,
    relationStyle,
    edgePoint,
    nodeTransform,
    linkPath,
    draggableNodes,
    externalStub,
    buildLinks,
    gridCells,
    arrowMarker,
    attachViewport,
    attachFilter,
    setHeader,
  };
})();
