(function () {
  "use strict";

  /** Rejects malformed, unordered or unbounded data before it reaches the chart. */
  function validHistory(data, range) {
    if (!data || data.range !== range || !Number.isSafeInteger(data.from) || data.from < 0
      || !Number.isSafeInteger(data.to) || data.to <= data.from || data.to - data.from > 31 * 86400000
      || !Number.isSafeInteger(data.interval_ms) || data.interval_ms < 60000
      || !Array.isArray(data.points) || data.points.length > 1441) return false;
    let previous = data.from - 1;
    return data.points.every(function (point) {
      if (!point || !Number.isSafeInteger(point.at) || point.at <= previous
        || point.at < data.from || point.at > data.to || point.at % data.interval_ms !== 0
        || !Number.isFinite(point.online) || point.online < 0
        || !Number.isSafeInteger(point.peak) || point.peak < point.online
        || !Number.isSafeInteger(point.samples) || point.samples < 1
        || point.samples > data.interval_ms / 60000) return false;
      previous = point.at;
      return true;
    });
  }

  /** Maps observed buckets onto the full window and splits lines at missing buckets. */
  function geometry(data, width, height) {
    const left = 32, right = width - 8, top = 12, bottom = height - 28;
    const peak = Math.max(1, ...data.points.map(function (point) { return point.peak; }));
    const step = Math.max(1, Math.pow(10, Math.floor(Math.log10(peak))) / 2);
    const ceiling = Math.ceil(peak / step) * step;
    const x = function (at) { return left + (at - data.from) / (data.to - data.from) * (right - left); };
    const y = function (online) { return bottom - online / ceiling * (bottom - top); };
    const segments = [];
    data.points.forEach(function (point, index) {
      if (!index || point.at - data.points[index - 1].at > data.interval_ms) segments.push([]);
      segments[segments.length - 1].push({ x: x(point.at), y: y(point.online), point: point });
    });
    return { left: left, right: right, top: top, bottom: bottom, ceiling: ceiling, x: x, y: y, segments: segments };
  }

  if (typeof module !== "undefined") module.exports = { validHistory: validHistory, geometry: geometry };
  if (typeof document === "undefined") return;
  const plot = document.getElementById("history-plot");
  if (!plot) return;
  const message = document.getElementById("history-message");
  const summary = document.getElementById("history-summary");
  const detail = document.getElementById("history-detail");
  const buttons = Array.from(document.querySelectorAll("[data-history-range]"));
  const number = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });
  const timestamp = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  let range = "24h", data, coordinates, cursor, marker, selected = -1;
  let timer, controller, sequence = 0;

  /** Creates chart elements with text content, without interpreting response data as HTML. */
  function svg(name, attributes, text) {
    const node = document.createElementNS("http://www.w3.org/2000/svg", name);
    Object.keys(attributes).forEach(function (key) { node.setAttribute(key, attributes[key]); });
    if (text !== undefined) node.textContent = text;
    plot.appendChild(node);
    return node;
  }

  /** Describes the current resolution and avoids implying continuous measurement. */
  function caption() {
    return (data.interval_ms / 60000) + "-minute " + (data.interval_ms === 60000 ? "samples" : "averages")
      + " · Local time · Gaps mean no data";
  }

  /** Shows one observed bucket's value for pointer and keyboard users. */
  function inspect(index) {
    if (!data || !coordinates || !data.points.length) return;
    selected = Math.max(0, Math.min(index, data.points.length - 1));
    const point = data.points[selected];
    const x = coordinates.x(point.at), y = coordinates.y(point.online);
    cursor.setAttribute("x1", x); cursor.setAttribute("x2", x);
    marker.setAttribute("cx", x); marker.setAttribute("cy", y);
    cursor.setAttribute("visibility", "visible"); marker.setAttribute("visibility", "visible");
    detail.textContent = timestamp.format(point.at) + " · " + number.format(point.online) + " players"
      + (data.interval_ms > 60000 ? " average · Peak " + number.format(point.peak) : "");
  }

  /** Restores the chart's reading guide when a sample is no longer selected. */
  function clearSelection() {
    selected = -1;
    if (cursor) cursor.setAttribute("visibility", "hidden");
    if (marker) marker.setAttribute("visibility", "hidden");
    if (data && data.points.length) detail.textContent = caption();
  }

  /** Draws a responsive chart with an honest zero baseline and a full time window. */
  function draw() {
    if (!data || !data.points.length) return;
    const width = Math.max(240, plot.clientWidth);
    const height = Math.max(100, plot.clientHeight);
    coordinates = geometry(data, width, height);
    const g = coordinates;
    plot.setAttribute("viewBox", "0 0 " + width + " " + height);
    plot.replaceChildren();
    [0, g.ceiling / 2, g.ceiling].forEach(function (value) {
      svg("line", { x1: g.left, x2: g.right, y1: g.y(value), y2: g.y(value), class: "chart-grid" });
      svg("text", { x: g.left - 8, y: g.y(value) + 4, "text-anchor": "end" }, number.format(value));
    });
    const format = new Intl.DateTimeFormat(undefined, range === "24h" ? { hour: "2-digit", minute: "2-digit" } : { month: "short", day: "numeric" });
    [data.from, (data.from + data.to) / 2, data.to].forEach(function (at, index) {
      svg("text", { x: g.x(at), y: height - 8, "text-anchor": ["start", "middle", "end"][index] }, format.format(at));
    });
    g.segments.forEach(function (segment) {
      if (segment.length === 1) {
        svg("circle", { cx: segment[0].x, cy: segment[0].y, r: 3, class: "chart-point" });
      } else {
        svg("path", { d: segment.map(function (point, index) { return (index ? "L" : "M") + point.x + "," + point.y; }).join(" "), class: "chart-line" });
      }
    });
    cursor = svg("line", { y1: g.top, y2: g.bottom, class: "chart-cursor", visibility: "hidden" });
    marker = svg("circle", { r: 4, class: "chart-point", visibility: "hidden" });
    if (selected >= 0) inspect(selected);
    else clearSelection();
  }

  /** Clears old readings when a range is empty, loading, or unavailable. */
  function showMessage(text) {
    data = undefined; coordinates = undefined; selected = -1;
    plot.toggleAttribute("hidden", true); plot.replaceChildren();
    message.hidden = false; message.textContent = text;
    summary.textContent = ""; detail.textContent = "";
  }

  /** Loads the selected range; only the newest request may change the display or retry timer. */
  async function load() {
    window.clearTimeout(timer);
    const request = ++sequence;
    if (controller) controller.abort();
    if (document.visibilityState === "hidden") return;
    controller = new AbortController();
    try {
      const url = new URL(window.CINNABAR_DOWNLOADS.online_history_url);
      url.searchParams.set("range", range);
      const response = await fetch(url, { credentials: "omit", referrerPolicy: "no-referrer",
        signal: AbortSignal.any([controller.signal, AbortSignal.timeout(5000)]) });
      if (!response.ok) throw new Error("History unavailable");
      const result = await response.json();
      if (request !== sequence) return;
      if (!validHistory(result, range)) throw new Error("Invalid history");
      if (!result.points.length) {
        showMessage("No history yet. Player counts will appear as samples are collected.");
        return;
      }
      data = result;
      selected = -1;
      message.hidden = true; plot.toggleAttribute("hidden", false);
      let sum = 0, samples = 0, peak = 0;
      data.points.forEach(function (point) { sum += point.online * point.samples; samples += point.samples; peak = Math.max(peak, point.peak); });
      summary.textContent = "Peak " + number.format(peak) + " · Average " + number.format(sum / samples)
        + " · " + number.format(samples) + " sampled minutes";
      draw();
    } catch (error) {
      if (request === sequence) showMessage("Player history is unavailable. Trying again shortly.");
    } finally {
      if (request === sequence && document.visibilityState !== "hidden") timer = window.setTimeout(load, 60000);
    }
  }

  buttons.forEach(function (button) {
    button.addEventListener("click", function () {
      if (button.dataset.historyRange === range) return;
      range = button.dataset.historyRange;
      buttons.forEach(function (item) { item.setAttribute("aria-pressed", String(item === button)); });
      showMessage("Loading player history…");
      load();
    });
  });
  /** Selects the nearest observation for a hover, drag, or tap. */
  function inspectPointer(event) {
    if (!coordinates || !data.points.length) return;
    const x = event.clientX - plot.getBoundingClientRect().left;
    let nearest = 0;
    data.points.forEach(function (point, index) {
      if (Math.abs(coordinates.x(point.at) - x) < Math.abs(coordinates.x(data.points[nearest].at) - x)) nearest = index;
    });
    inspect(nearest);
  }
  plot.addEventListener("pointermove", inspectPointer);
  plot.addEventListener("pointerdown", inspectPointer);
  plot.addEventListener("pointerleave", function (event) { if (event.pointerType !== "touch") clearSelection(); });
  plot.addEventListener("blur", clearSelection);
  plot.addEventListener("focus", function () { if (data && selected < 0) inspect(data.points.length - 1); });
  plot.addEventListener("keydown", function (event) {
    if (!data || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    inspect(event.key === "Home" ? 0 : event.key === "End" ? data.points.length - 1
      : (selected < 0 ? data.points.length - 1 : selected) + (event.key === "ArrowLeft" ? -1 : 1));
  });
  document.addEventListener("visibilitychange", load);
  if (typeof ResizeObserver !== "undefined") new ResizeObserver(draw).observe(plot);
  load();
})();
