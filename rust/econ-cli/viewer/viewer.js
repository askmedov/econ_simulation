// The viewer: a finished run of econ-sim, replayed month by month. The data
// is in the page (written by `econ-sim view`); nothing here changes it.
(function () {
  "use strict";

  const DATA = JSON.parse(document.getElementById("run").textContent);
  const P = DATA.places.name.length;
  const M = DATA.months;
  const ARMS = DATA.arms.length;
  const PAIRED = ARMS > 1;
  const SERIES = DATA.series;
  const SI = Object.fromEntries(SERIES.map((s, i) => [s.key, i]));
  const REC = DATA.records;
  const MONTH_NAMES = ["", "January", "February", "March", "April", "May", "June", "July", "August",
    "September", "October", "November", "December"];
  const SHORT_MONTHS = ["", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const FAMINE = DATA.thresholds.famine;
  const SHORT = DATA.thresholds.short;

  const $ = (id) => document.getElementById(id);
  const el = (tag, attrs, ...children) => {
    const node = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v === null || v === undefined || v === false) continue;
      if (k === "class") node.className = v;
      else if (k === "text") node.textContent = v;
      else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
      else node.setAttribute(k, v === true ? "" : v);
    }
    for (const c of children) if (c !== null && c !== undefined) node.append(c);
    return node;
  };
  const SVG_NS = "http://www.w3.org/2000/svg";
  const svg = (tag, attrs) => {
    const node = document.createElementNS(SVG_NS, tag);
    for (const [k, v] of Object.entries(attrs || {})) node.setAttribute(k, v);
    return node;
  };

  // ---------------------------------------------------------------- state

  const firstEvent = DATA.events.length ? Math.min(...DATA.events.map((e) => e.month)) : null;
  const state = {
    month: 1,                    // 1 = the first month of the run
    view: PAIRED ? "with" : "run", // "without", "with", "gap" or "run"
    measure: "food_price" in SI ? "food_price" : SERIES[0].key,
    selected: -1,                // a place, chosen on the map
    lit: [],                     // places of the chosen chronicle entry
    litEntry: null,
    rankMax: 3,
    span: {},                    // per measure: colours span the whole "run", or this "month"
    playing: false,
    speed: 2,
    roads: true,
    resources: false,
    villages: false,
  };
  // The arm a view reads: the gap's chronicle and region numbers are the
  // event's run.
  const armOf = (view) => (view === "without" ? 0 : PAIRED ? 1 : 0);

  // ------------------------------------------------------------- numbers

  const fmtInt = (v) => (Number.isFinite(v) ? Math.round(v).toLocaleString("en-GB") : "–");
  function fmtLevel(v) {
    if (!Number.isFinite(v)) return "–";
    const a = Math.abs(v);
    if (a >= 1000) return fmtInt(v);
    if (a >= 100) return v.toFixed(0);
    if (a >= 10) return v.toFixed(1);
    if (a >= 1) return v.toFixed(2);
    if (a === 0) return "0";
    return v.toFixed(3);
  }
  function fmtPct(v, digits) {
    if (!Number.isFinite(v)) return "–";
    const d = digits === undefined ? (Math.abs(v) < 0.1 && v !== 0 ? 1 : 0) : digits;
    return (100 * v).toFixed(d) + "%";
  }
  function fmtSigned(text, v) {
    if (!Number.isFinite(v)) return "–";
    if (v > 0) return "+" + text;
    if (v < 0) return "−" + text.replace(/^-/, "");
    return text;
  }
  function fmtRate(v) {
    if (!Number.isFinite(v)) return "–";
    return Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(1);
  }

  // A measure's value as the map shows it, and how it is written.
  function formatShown(series, v, gap) {
    if (!Number.isFinite(v)) return "–";
    const show = series.show;
    // (a Gini coefficient is written as it is, 0 to 1)
    if (series.unit.startsWith("Gini")) return gap ? fmtSigned(Math.abs(v).toFixed(2), v) : v.toFixed(2);
    if (gap) {
      if (show === "change" || show === "level") return fmtSigned(fmtPct(Math.abs(v)), v);
      if (show === "share") return fmtSigned((100 * Math.abs(v)).toFixed(1) + " pts", v);
      if (show === "rate") return fmtSigned(fmtRate(Math.abs(v)), v);
      return fmtSigned(fmtInt(Math.abs(v)), v);
    }
    if (show === "change") return fmtSigned(fmtPct(Math.abs(v)), v);
    if (show === "share") return fmtPct(v);
    if (show === "rate") return fmtRate(v);
    if (show === "count") return fmtInt(v);
    return fmtLevel(v);
  }

  // ------------------------------------------------------- the packed data

  // values[arm][series] holds month by month (rows), place by place.
  let values = null;

  function bytesOf(base64) {
    const text = atob(base64);
    const out = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) out[i] = text.charCodeAt(i);
    return out;
  }

  async function unpack() {
    if (typeof DecompressionStream === "undefined") {
      throw new Error("This browser can't unpack the run's data: it needs DecompressionStream (Chrome 80, Firefox 113, Safari 16.4 or later).");
    }
    const stream = new Blob([bytesOf(DATA.packed)]).stream().pipeThrough(new DecompressionStream("deflate"));
    const raw = new Uint8Array(await new Response(stream).arrayBuffer());
    let at = 0;
    const firstArm = [];
    values = [];
    for (let a = 0; a < ARMS; a++) {
      const arm = [];
      for (let s = 0; s < SERIES.length; s++) {
        const kept = SERIES[s].kept;
        const out = new Float32Array(M * P);
        const whole = a === 0 ? new Int32Array(M * P) : null;
        for (let p = 0; p < P; p++) {
          let q = 0;
          for (let t = 0; t < M; t++) {
            let x = 0, mult = 1, b;
            do { b = raw[at++]; x += (b & 0x7f) * mult; mult *= 128; } while (b >= 0x80);
            q += x % 2 === 1 ? -(x + 1) / 2 : x / 2;
            const i = t * P + p;
            const w = a === 0 ? q : q + firstArm[s][i];
            if (whole) whole[i] = q;
            out[i] = kept.scale ? w / kept.scale : Math.sign(w) * kept.unit * Math.expm1(Math.abs(w) * kept.step);
          }
        }
        if (whole) firstArm.push(whole);
        arm.push(out);
      }
      values.push(arm);
    }
    if (at !== raw.length) throw new Error("The run's data is not what this page expects.");
  }

  const raw = (a, s, t, p) => values[a][s][(t - 1) * P + p];
  const peopleIn = (a, t, p) => raw(a, SI.population, t, p);

  // The value of measure s at place p in month t, as the map shows it.
  function shown(s, a, t, p) {
    const series = SERIES[s];
    const v = raw(a, s, t, p);
    const people = peopleIn(a, t, p);
    switch (series.show) {
      case "change": {
        const start = DATA.places.people[p];
        return start > 0 ? v / start - 1 : NaN;
      }
      case "rate": return people > 0 ? (1000 * v) / people : NaN;
      case "count": return v;
      default: return people > 0 ? v : NaN;
    }
  }
  // The gap the forced events made: with them against without.
  function gapShown(s, t, p) {
    const series = SERIES[s];
    if (series.show === "change") {
      const b = peopleIn(0, t, p), w = peopleIn(1, t, p);
      return b > 0 ? w / b - 1 : NaN;
    }
    const b = shown(s, 0, t, p), w = shown(s, 1, t, p);
    if (series.show === "level") return b > 0 ? w / b - 1 : NaN;
    return w - b;
  }
  function mapValue(s, t, p) {
    return state.view === "gap" ? gapShown(s, t, p) : shown(s, armOf(state.view), t, p);
  }

  // ------------------------------------------------------------- colours

  let tokens = {};
  function readTokens() {
    const css = getComputedStyle(document.documentElement);
    const get = (name) => css.getPropertyValue(name).trim();
    tokens = {
      seq: get("--seq").split(/\s+/).map(parseHex),
      hot: get("--hot").split(/\s+/).map(parseHex),
      mid: parseHex(get("--mid")),
      empty: parseHex(get("--empty")),
      surface: get("--surface"),
      ink: get("--ink"),
      ink2: get("--ink-2"),
      muted: get("--muted"),
      marker: get("--marker"),
      halo: get("--halo"),
      river: get("--river"),
      road: get("--road"),
      focus: get("--focus"),
      withColour: get("--with"),
      terrain: {
        plain: parseHex(get("--terrain-plain")), valley: parseHex(get("--terrain-valley")),
        upland: parseHex(get("--terrain-upland")), forest: parseHex(get("--terrain-forest")),
        coast: parseHex(get("--terrain-coast")),
      },
      resource: { iron: get("--iron"), lead: get("--lead"), salt: get("--salt"), clay: get("--clay") },
    };
    tokens.diverging = [...tokens.seq.slice(2).reverse().slice(0, 4), tokens.mid, ...tokens.hot.slice(2, 6)];
    tokens.mapBg = parseHex(get("--surface"));
  }
  function parseHex(text) {
    if (!text) return [128, 128, 128];
    if (text.startsWith("rgb")) {
      const n = text.match(/[\d.]+/g).map(Number);
      return [n[0], n[1], n[2]];
    }
    const h = text.replace("#", "");
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }
  // Colours are mixed in OKLab, so a ramp's steps look evenly spaced.
  function toLab([r, g, b]) {
    const lin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
    const R = lin(r), G = lin(g), B = lin(b);
    const l = Math.cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B);
    const m = Math.cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B);
    const s = Math.cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B);
    return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
      1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
      0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
  }
  function fromLab([L, A, B]) {
    const l = Math.pow(L + 0.3963377774 * A + 0.2158037573 * B, 3);
    const m = Math.pow(L - 0.1055613458 * A - 0.0638541728 * B, 3);
    const s = Math.pow(L - 0.0894841775 * A - 1.291485548 * B, 3);
    const gam = (c) => 255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);
    const clamp = (c) => Math.max(0, Math.min(255, Math.round(c)));
    return [clamp(gam(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s)),
      clamp(gam(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s)),
      clamp(gam(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s))];
  }
  // A table of 256 colours along the stops.
  function ramp(stops) {
    const labs = stops.map(toLab);
    const out = [];
    for (let i = 0; i < 256; i++) {
      const x = (i / 255) * (labs.length - 1);
      const k = Math.min(labs.length - 2, Math.floor(x));
      const f = x - k;
      out.push(fromLab(labs[k].map((c, j) => c + (labs[k + 1][j] - c) * f)));
    }
    return out;
  }
  const css = (rgb) => `rgb(${rgb[0]},${rgb[1]},${rgb[2]})`;

  // ------------------------------------------------------- colour scales

  // A scale turns a value into a colour; its domain is fixed over the
  // whole run, so a colour means the same in every month.
  const scaleCache = new Map();
  function percentile(sorted, q) {
    if (!sorted.length) return NaN;
    const at = q * (sorted.length - 1);
    const lo = Math.floor(at), hi = Math.ceil(at);
    return sorted[lo] + (sorted[hi] - sorted[lo]) * (at - lo);
  }
  // (prices and other levels take their range from the villages: a town,
  // which grows no grain, would stretch it and pale the countryside)
  const isVillage = DATA.places.kind.map((k) => k === "village");
  const hasVillages = isVillage.some(Boolean);
  function sample(fn, arms, month, villagesOnly) {
    const out = [];
    const months = month ? 1 : M;
    const step = Math.max(1, Math.floor((months * P * arms.length) / 120000));
    let k = 0;
    for (const a of arms) {
      for (let t = month || 1; t <= (month || M); t++) {
        for (let p = 0; p < P; p++) {
          if (k++ % step) continue;
          if (villagesOnly && !isVillage[p]) continue;
          const v = fn(a, t, p);
          if (Number.isFinite(v)) out.push(v);
        }
      }
    }
    return out.sort((x, y) => x - y);
  }
  function niceStep(span, count) {
    const rough = span / count;
    const mag = Math.pow(10, Math.floor(Math.log10(rough)));
    const f = rough / mag;
    return (f < 1.5 ? 1 : f < 3 ? 2 : f < 7 ? 5 : 10) * mag;
  }
  // Prices and sums are coloured by this month's spread at first (where it
  // is dear now); famine, deaths, leaving and people by the whole run's, so
  // a colour means the same in every month.
  function spanOf(s) {
    const key = SERIES[s].key;
    return state.span[key] || (SERIES[s].show === "level" ? "month" : "run");
  }
  function scaleFor(s, view) {
    const gap = view === "gap";
    // (this month's colours: the month shown, in the run shown)
    const month = spanOf(s) === "month" ? state.month : 0;
    const arms = month ? [armOf(view)] : PAIRED ? [0, 1] : [0];
    const key = `${SERIES[s].key}:${gap ? "gap" : "level"}:${month}:${month ? arms[0] : ""}`;
    if (scaleCache.has(key)) return scaleCache.get(key);
    const series = SERIES[s];
    const worseHigh = series.worse === "high";
    let scale;
    if (!gap && series.key === "ration") {
      // Hunger: fed is grey; short and famine grow red.
      const table = ramp(tokens.hot.slice(1));
      scale = {
        kind: "hunger", lo: 0.5, hi: 1,
        colour(v) {
          if (!Number.isFinite(v)) return tokens.empty;
          if (v >= SHORT) return tokens.mid;
          const f = Math.min(1, (SHORT - v) / (SHORT - 0.5));
          return table[Math.round(f * 255)];
        },
        position: (v) => (Math.min(1.15, Math.max(0.5, v)) - 0.5) / 0.65,
      };
    } else if (gap || series.show === "change") {
      const vals = gap ? sample((a, t, p) => gapShown(s, t, p), [0], month)
        : sample((a, t, p) => shown(s, a, t, p), arms, month);
      const floor = series.show === "share" ? 0.01 : series.show === "rate" ? 0.5 : series.show === "count" ? 1 : 0.02;
      let d = Math.max(Math.abs(percentile(vals, 0.02)), Math.abs(percentile(vals, 0.98)), floor);
      const step = niceStep(d, 2);
      d = Math.ceil(d / step) * step;
      // red is the worse side
      const stops = worseHigh ? tokens.diverging : [...tokens.diverging].reverse();
      const table = ramp(stops);
      scale = {
        kind: "diverging", lo: -d, hi: d, worseHigh,
        colour(v) {
          if (!Number.isFinite(v)) return tokens.empty;
          const f = (Math.max(-d, Math.min(d, v)) + d) / (2 * d);
          return table[Math.round(f * 255)];
        },
        position: (v) => (Math.max(-d, Math.min(d, v)) + d) / (2 * d),
      };
    } else {
      const vals = sample((a, t, p) => shown(s, a, t, p), arms, month, series.show === "level" && hasVillages);
      let lo = percentile(vals, 0.02), hi = percentile(vals, 0.98);
      if (series.show === "count" || series.show === "rate") lo = 0;
      if (series.show === "share") { lo = Math.min(lo, hi - 0.05); hi = Math.min(Math.max(hi, lo + 0.05), Math.max(1, hi)); }
      if (!(hi > lo)) hi = lo + (Math.abs(lo) || 1);
      const log = series.show === "level" && lo > 0 && hi / lo > 4;
      const table = ramp(worseHigh ? tokens.hot : tokens.seq);
      const f = log ? (v) => (Math.log(v) - Math.log(lo)) / (Math.log(hi) - Math.log(lo)) : (v) => (v - lo) / (hi - lo);
      scale = {
        kind: "sequential", lo, hi, log,
        colour(v) {
          if (!Number.isFinite(v) || (log && v <= 0)) return Number.isFinite(v) ? table[0] : tokens.empty;
          return table[Math.round(Math.max(0, Math.min(1, f(v))) * 255)];
        },
        position: (v) => Math.max(0, Math.min(1, log && v <= 0 ? 0 : f(v))),
      };
    }
    scaleCache.set(key, scale);
    return scale;
  }

  // ------------------------------------------------------------------ map

  const map = {
    canvas: $("map"),
    frame: $("map-frame"),
    width: 0, height: 0, dpr: 1,
    index: null,    // the place each pixel of the map belongs to (-1: none)
    edge: null,     // pixels on a boundary between places
    image: null,
    offscreen: document.createElement("canvas"),
    lines: document.createElement("canvas"),
    px: null, py: null, // places on the canvas, in CSS pixels
    spacing: 1,
  };
  const X = DATA.places.x, Y = DATA.places.y;
  const minX = Math.min(...X), maxX = Math.max(...X), minY = Math.min(...Y), maxY = Math.max(...Y);
  const spanKm = Math.max(maxX - minX, maxY - minY, 1);
  const spacingKm = Math.sqrt(((maxX - minX + 1) * (maxY - minY + 1)) / Math.max(P, 1));

  function layoutMap() {
    const frameWidth = $("map-body").clientWidth;
    const pad = spacingKm * 0.9;
    const kmW = maxX - minX + 2 * pad, kmH = maxY - minY + 2 * pad;
    const tallest = Math.max(280, window.innerHeight * 0.62);
    let width = frameWidth, height = (frameWidth * kmH) / kmW;
    if (height > tallest) { height = tallest; width = (height * kmW) / kmH; }
    width = Math.floor(width); height = Math.floor(height);
    if (width === map.width && height === map.height && map.index) return;
    map.width = width; map.height = height;
    map.dpr = Math.min(window.devicePixelRatio || 1, 2);
    map.canvas.width = Math.round(width * map.dpr);
    map.canvas.height = Math.round(height * map.dpr);
    map.canvas.style.width = width + "px";
    map.canvas.style.height = height + "px";
    map.frame.style.width = width + "px";
    const k = width / kmW;
    map.kmToPx = k;
    map.px = new Float32Array(P); map.py = new Float32Array(P);
    for (let p = 0; p < P; p++) {
      map.px[p] = (X[p] - minX + pad) * k;
      map.py[p] = height - (Y[p] - minY + pad) * k;
    }
    map.spacing = spacingKm * k;
    buildIndex();
    drawLines();
  }

  // Each pixel belongs to its nearest place, within reach of one.
  function buildIndex() {
    const W = map.width, H = map.height;
    const cell = Math.max(4, map.spacing * 1.5);
    const cols = Math.ceil(W / cell) + 1, rows = Math.ceil(H / cell) + 1;
    const buckets = Array.from({ length: cols * rows }, () => []);
    for (let p = 0; p < P; p++) {
      const c = Math.min(cols - 1, Math.max(0, Math.floor(map.px[p] / cell)));
      const r = Math.min(rows - 1, Math.max(0, Math.floor(map.py[p] / cell)));
      buckets[r * cols + c].push(p);
    }
    const reach = map.spacing * 1.35;
    const reach2 = reach * reach;
    const index = new Int32Array(W * H).fill(-1);
    for (let y = 0; y < H; y++) {
      const r = Math.floor((y + 0.5) / cell);
      for (let x = 0; x < W; x++) {
        const c = Math.floor((x + 0.5) / cell);
        let best = -1, bestD = reach2;
        for (let dr = -1; dr <= 1; dr++) {
          const rr = r + dr;
          if (rr < 0 || rr >= rows) continue;
          for (let dc = -1; dc <= 1; dc++) {
            const cc = c + dc;
            if (cc < 0 || cc >= cols) continue;
            const list = buckets[rr * cols + cc];
            for (let i = 0; i < list.length; i++) {
              const p = list[i];
              const dx = map.px[p] - x - 0.5, dy = map.py[p] - y - 0.5;
              const d = dx * dx + dy * dy;
              if (d < bestD) { bestD = d; best = p; }
            }
          }
        }
        index[y * W + x] = best;
      }
    }
    const edge = new Uint8Array(W * H);
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * W + x, here = index[i];
        if (here < 0) continue;
        if ((x + 1 < W && index[i + 1] !== here) || (y + 1 < H && index[i + W] !== here)) edge[i] = 1;
      }
    }
    map.index = index;
    map.edge = edge;
    map.offscreen.width = W; map.offscreen.height = H;
    map.image = map.offscreen.getContext("2d").createImageData(W, H);
  }

  // Roads and the river, drawn once for each size and theme.
  function drawLines() {
    const c = map.lines;
    c.width = map.canvas.width; c.height = map.canvas.height;
    const g = c.getContext("2d");
    g.setTransform(map.dpr, 0, 0, map.dpr, 0, 0);
    g.lineCap = "round";
    g.strokeStyle = tokens.road;
    g.lineWidth = DATA.roads.length > 2000 ? 0.5 : 0.8;
    g.beginPath();
    for (const [a, b] of DATA.roads) { g.moveTo(map.px[a], map.py[a]); g.lineTo(map.px[b], map.py[b]); }
    g.stroke();
    g.strokeStyle = tokens.river;
    g.lineWidth = 2.4;
    g.beginPath();
    for (const [a, b] of DATA.rivers) { g.moveTo(map.px[a], map.py[a]); g.lineTo(map.px[b], map.py[b]); }
    g.stroke();
  }

  const terrainOf = DATA.places.terrain;
  function placeColours() {
    const out = new Array(P);
    if (state.measure === "terrain") {
      for (let p = 0; p < P; p++) out[p] = tokens.terrain[terrainOf[p]] || tokens.mid;
      return out;
    }
    const s = SI[state.measure];
    const scale = scaleFor(s, state.view);
    for (let p = 0; p < P; p++) out[p] = scale.colour(mapValue(s, state.month, p));
    return out;
  }

  function drawMap() {
    if (!values || !map.index) return;
    const colours = placeColours();
    const data = map.image.data;
    const index = map.index, edge = map.edge;
    const [br, bg, bb] = tokens.mapBg;
    for (let i = 0, n = index.length; i < n; i++) {
      const p = index[i];
      let r, g, b;
      if (p < 0) { r = br; g = bg; b = bb; } else {
        const c = colours[p];
        if (edge[i]) { r = c[0] * 0.86 + br * 0.14; g = c[1] * 0.86 + bg * 0.14; b = c[2] * 0.86 + bb * 0.14; } else { r = c[0]; g = c[1]; b = c[2]; }
      }
      const k = i * 4;
      data[k] = r; data[k + 1] = g; data[k + 2] = b; data[k + 3] = 255;
    }
    map.offscreen.getContext("2d").putImageData(map.image, 0, 0);
    const g = map.canvas.getContext("2d");
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.imageSmoothingEnabled = false;
    g.drawImage(map.offscreen, 0, 0, map.canvas.width, map.canvas.height);
    if (state.roads) g.drawImage(map.lines, 0, 0);
    g.setTransform(map.dpr, 0, 0, map.dpr, 0, 0);
    if (state.villages) drawVillages(g);
    if (state.resources) drawResources(g);
    drawTowns(g);
    drawRings(g);
  }

  function drawVillages(g) {
    const a = armOf(state.view);
    g.fillStyle = tokens.marker;
    g.globalAlpha = 0.55;
    const unit = Math.max(1.2, map.spacing * 0.09);
    for (let p = 0; p < P; p++) {
      if (DATA.places.kind[p] !== "village") continue;
      const people = peopleIn(a, state.month, p);
      if (!(people > 0)) continue;
      const r = Math.max(0.8, Math.min(map.spacing * 0.3, unit * Math.sqrt(people / 300)));
      g.beginPath(); g.arc(map.px[p], map.py[p], r, 0, 2 * Math.PI); g.fill();
    }
    g.globalAlpha = 1;
  }

  const RESOURCE_ORDER = ["iron", "lead", "salt", "clay"];
  function glyph(g, kind, x, y, r) {
    g.beginPath();
    if (kind === "iron" || kind === "lead") {
      g.moveTo(x, y - r); g.lineTo(x + r, y + r * 0.8); g.lineTo(x - r, y + r * 0.8); g.closePath();
    } else if (kind === "salt") {
      g.arc(x, y, r * 0.85, 0, 2 * Math.PI);
    } else {
      g.rect(x - r * 0.75, y - r * 0.75, r * 1.5, r * 1.5);
    }
    g.fillStyle = tokens.resource[kind] || tokens.ink2;
    g.fill();
    g.lineWidth = 1;
    g.strokeStyle = kind === "salt" ? tokens.ink : tokens.halo;
    g.stroke();
  }
  function drawResources(g) {
    const r = Math.max(2.5, Math.min(5, map.spacing * 0.28));
    for (let p = 0; p < P; p++) {
      const list = DATA.places.resources[p];
      if (!list.length) continue;
      const kinds = RESOURCE_ORDER.filter((k) => list.includes(k));
      kinds.forEach((kind, i) => glyph(g, kind, map.px[p] + (i - (kinds.length - 1) / 2) * r * 2.1, map.py[p] - r * 1.6, r));
    }
  }

  // Half the side of a town's marker: bigger for more people, but never
  // hiding the places around it.
  function townSize(p) {
    // (shown by town, each cell is the town and its villages: a mark will do)
    if (DATA.by_town) return DATA.places.stronghold[p] ? 4.5 : 2.5;
    const people = DATA.places.people[p];
    const most = Math.max(2.5, map.spacing * (DATA.places.stronghold[p] ? 0.75 : 0.42));
    return Math.max(2.5, Math.min(most, 10, 2.2 * Math.sqrt(people / 1000) + 2.5));
  }
  function drawTowns(g) {
    const labelAll = DATA.places.kind.filter((k) => k === "town").length <= 14;
    g.lineJoin = "round";
    for (let p = 0; p < P; p++) {
      if (DATA.places.kind[p] !== "town") continue;
      const x = map.px[p], y = map.py[p], r = townSize(p);
      g.beginPath();
      if (DATA.places.border[p]) {
        g.moveTo(x, y - r * 1.25); g.lineTo(x + r * 1.25, y); g.lineTo(x, y + r * 1.25); g.lineTo(x - r * 1.25, y); g.closePath();
      } else {
        g.rect(x - r, y - r, 2 * r, 2 * r);
      }
      g.fillStyle = tokens.marker;
      g.strokeStyle = tokens.halo;
      g.lineWidth = 2;
      g.stroke();
      g.fill();
      if (DATA.places.stronghold[p]) {
        g.beginPath(); g.arc(x, y, r * 1.9, 0, 2 * Math.PI);
        g.strokeStyle = tokens.halo; g.lineWidth = 4; g.stroke();
        g.strokeStyle = tokens.marker; g.lineWidth = 1.5; g.stroke();
      }
      if (labelAll || DATA.places.stronghold[p] || DATA.places.border[p]) {
        label(g, p, r + 4, DATA.places.stronghold[p]);
      }
    }
  }
  // A place's name beside it (on its left if it would run off the map).
  function label(g, p, gap, strong) {
    const text = DATA.places.name[p];
    g.font = `${strong ? 600 : 500} 11px ${getComputedStyle(document.body).fontFamily}`;
    const w = g.measureText(text).width;
    let x = map.px[p] + gap;
    if (x + w > map.width - 2) x = Math.max(2, map.px[p] - gap - w);
    const y = map.py[p] + 4;
    g.lineWidth = 3;
    g.strokeStyle = tokens.halo;
    g.strokeText(text, x, y);
    g.fillStyle = tokens.ink;
    g.fillText(text, x, y);
  }
  function ring(g, p, colour, width, r) {
    const x = map.px[p], y = map.py[p];
    g.beginPath(); g.arc(x, y, r, 0, 2 * Math.PI);
    g.strokeStyle = tokens.halo; g.lineWidth = width + 3; g.stroke();
    g.strokeStyle = colour; g.lineWidth = width; g.stroke();
  }
  function drawRings(g) {
    const r = Math.max(6, Math.min(14, map.spacing * 0.75));
    for (const p of state.lit) ring(g, p, tokens.ink, 2, r);
    if (hoverPlace >= 0 && hoverPlace !== state.selected) ring(g, hoverPlace, tokens.ink2, 1.2, r);
    if (state.selected >= 0) {
      ring(g, state.selected, tokens.focus, 2.5, r + 2);
      if (DATA.places.kind[state.selected] !== "town") label(g, state.selected, r + 6, true);
    }
  }

  // The place under the pointer.
  let hoverPlace = -1;
  function placeAt(event) {
    const rect = map.canvas.getBoundingClientRect();
    const x = Math.floor(event.clientX - rect.left), y = Math.floor(event.clientY - rect.top);
    if (x < 0 || y < 0 || x >= map.width || y >= map.height || !map.index) return -1;
    let p = map.index[y * map.width + x];
    // a town's marker is easier to hit than its cell
    for (let q = 0; q < P; q++) {
      if (DATA.places.kind[q] !== "town") continue;
      const r = townSize(q) + 3;
      if (Math.abs(map.px[q] - x) <= r && Math.abs(map.py[q] - y) <= r) { p = q; break; }
    }
    return p;
  }
  function showTip(event, p) {
    const tip = $("map-tip");
    if (p < 0) { tip.hidden = true; return; }
    tip.replaceChildren(...tipContent(p));
    tip.hidden = false;
    const frame = map.frame.getBoundingClientRect();
    const w = tip.offsetWidth, h = tip.offsetHeight;
    let x = event.clientX - frame.left + 14, y = event.clientY - frame.top + 14;
    if (x + w > frame.width) x = event.clientX - frame.left - w - 14;
    if (y + h > frame.height) y = event.clientY - frame.top - h - 14;
    tip.style.left = Math.max(0, x) + "px";
    tip.style.top = Math.max(0, y) + "px";
  }
  function kindName(p) {
    if (DATA.places.stronghold[p]) return "City";
    if (DATA.places.kind[p] === "town") return DATA.by_town ? "Market town and its countryside" : "Market town";
    return "Village";
  }
  function tipContent(p) {
    const out = [el("strong", { text: DATA.places.name[p] })];
    const what = [kindName(p), DATA.places.terrain[p]];
    if (DATA.places.resources[p].length) what.push(DATA.places.resources[p].join(", "));
    out.push(el("div", { class: "tip-line", text: what.join(" · ") }));
    if (state.measure === "terrain") return out;
    const s = SI[state.measure];
    const series = SERIES[s];
    out.push(el("div", { class: "tip-line strong", text: `${series.label}, ${unitText(series, false)}` }));
    if (PAIRED) {
      for (const a of [0, 1]) {
        out.push(el("div", { class: "tip-row" },
          el("span", { class: "tip-label" }, el("i", { class: "swatch " + (a ? "with" : "without") }), DATA.arms[a].label),
          el("b", { text: formatShown(series, shown(s, a, state.month, p), false) })));
      }
      out.push(el("div", { class: "tip-row" }, el("span", { class: "tip-label", text: "The gap" }),
        el("b", { text: formatShown(series, gapShown(s, state.month, p), true) })));
    } else {
      out.push(el("div", { class: "tip-row" }, el("span", { class: "tip-label", text: "This month" }),
        el("b", { text: formatShown(series, shown(s, 0, state.month, p), false) })));
    }
    out.push(el("div", { class: "tip-row" }, el("span", { class: "tip-label", text: "People" }),
      el("span", { text: fmtInt(peopleIn(armOf(state.view), state.month, p)) })));
    return out;
  }

  // -------------------------------------------------------------- legend

  function unitText(series, gap) {
    if (!gap) {
      if (series.show === "change") return "against the start";
      return series.unit;
    }
    if (series.show === "change" || series.show === "level") return "higher or lower, %";
    if (series.show === "share") return "the difference, in points";
    return `the difference, ${series.unit}`;
  }
  function drawLegend() {
    const box = $("legend");
    const about = $("measure-about");
    if (state.measure === "terrain") {
      box.replaceChildren(el("div", { class: "legend-title", text: "Terrain" }),
        el("div", { class: "legend-swatches" }, ...Object.keys(tokens.terrain)
          .filter((t) => terrainOf.includes(t))
          .map((t) => el("span", {}, el("i", { style: `background:${css(tokens.terrain[t])}` }), t))));
      about.textContent = "What each place's land is. Valleys have the best fields, the uplands three and a half times the pasture (the wool country, where iron and lead are found) and the dearest carting, and the forest two and a half times the woods. Brine springs and clay lie in the lowlands.";
      $("scale").disabled = true;
      return;
    }
    const s = SI[state.measure];
    const series = SERIES[s];
    const gap = state.view === "gap";
    const scale = scaleFor(s, state.view);
    const stops = [];
    for (let i = 0; i <= 24; i++) {
      const f = i / 24;
      let v;
      if (scale.kind === "hunger") v = 0.5 + f * 0.65;
      else if (scale.kind === "diverging") v = scale.lo + f * (scale.hi - scale.lo);
      else v = scale.log ? Math.exp(Math.log(scale.lo) + f * (Math.log(scale.hi) - Math.log(scale.lo))) : scale.lo + f * (scale.hi - scale.lo);
      stops.push(`${css(scale.colour(v))} ${(f * 100).toFixed(1)}%`);
    }
    const bar = el("div", { class: "legend-bar", style: `background: linear-gradient(90deg, ${stops.join(", ")})` });
    const ticks = el("div", { class: "legend-ticks" });
    const marks = el("div", { class: "legend-marks" });
    const tick = (v, text, cls) => {
      const pos = scale.position(v) * 100;
      ticks.append(el("span", { class: cls || "", style: `left:${pos}%`, text }));
    };
    if (scale.kind === "hunger") {
      tick(0.5, "50%", "first");
      tick(FAMINE, `${fmtPct(FAMINE)} famine`);
      tick(SHORT, `${fmtPct(SHORT)} short`);
      tick(1.15, "fed", "last");
      for (const v of [FAMINE, SHORT]) marks.append(el("i", { style: `left:${scale.position(v) * 100}%` }));
      bar.append(marks);
    } else if (scale.kind === "diverging") {
      tick(scale.lo, formatShown(series, scale.lo, true), "first");
      tick(0, gap ? "no gap" : "as at the start");
      tick(scale.hi, formatShown(series, scale.hi, true), "last");
    } else {
      const fmt = (v) => formatShown(series, v, false);
      tick(scale.lo, fmt(scale.lo), "first");
      const mid = scale.log ? Math.sqrt(scale.lo * scale.hi) : (scale.lo + scale.hi) / 2;
      tick(mid, fmt(mid));
      tick(scale.hi, fmt(scale.hi), "last");
    }
    const title = gap ? `${series.label}, ${DATA.arms[1].label.toLowerCase()} against without` : series.label;
    box.replaceChildren(el("div", { class: "legend-title", text: `${title} (${unitText(series, gap)})` }), bar, ticks);
    let text = series.about;
    if (gap) {
      text += series.show === "level" || series.show === "change"
        ? " Coloured by how much higher or lower it is with the forced events than without, in the same month; red is worse."
        : " Coloured by the difference with the forced events, in the same month; red is worse.";
    } else if (scale.kind === "sequential") {
      const whose = series.show === "level" && hasVillages ? "villages" : "places";
      text += ` Colours run from the 2nd to the 98th percentile of the ${whose} ${spanOf(s) === "month" ? "this month" : "over the whole run"}` + (scale.log ? ", on a log scale." : ".");
    }
    $("scale").disabled = scale.kind === "hunger";
    $("scale").value = spanOf(s);
    about.textContent = text;
  }

  function drawSymbols() {
    const box = $("symbols");
    const item = (shape, text) => el("span", {}, shape, text);
    const icon = (draw) => {
      const s = svg("svg", { viewBox: "0 0 14 14", "aria-hidden": "true" });
      draw(s);
      return s;
    };
    const items = [
      item(icon((s) => s.append(svg("rect", { x: 3, y: 3, width: 8, height: 8, fill: "var(--marker)" }))), "Market town"),
    ];
    if (DATA.places.stronghold.some(Boolean)) {
      items.push(item(icon((s) => {
        s.append(svg("circle", { cx: 7, cy: 7, r: 6, fill: "none", stroke: "var(--marker)", "stroke-width": 1.3 }));
        s.append(svg("rect", { x: 4, y: 4, width: 6, height: 6, fill: "var(--marker)" }));
      }), "The city (the state's stronghold)"));
    }
    if (DATA.places.border.some(Boolean)) {
      items.push(item(icon((s) => s.append(svg("path", { d: "M7 1.5L12.5 7L7 12.5L1.5 7Z", fill: "var(--marker)" }))), "Trades beyond the region"));
    }
    items.push(item(icon((s) => s.append(svg("path", { d: "M1 10L13 4", stroke: "var(--river)", "stroke-width": 2.4, "stroke-linecap": "round" }))), "River"));
    if (state.resources) {
      const shapes = {
        iron: "M7 2L12 11H2Z", lead: "M7 2L12 11H2Z", salt: null, clay: "M3 3H11V11H3Z",
      };
      for (const kind of RESOURCE_ORDER) {
        if (!DATA.places.resources.some((r) => r.includes(kind))) continue;
        items.push(item(icon((s) => {
          if (shapes[kind]) s.append(svg("path", { d: shapes[kind], fill: `var(--${kind})` }));
          else s.append(svg("circle", { cx: 7, cy: 7, r: 4.5, fill: "var(--salt)", stroke: "var(--ink)", "stroke-width": 1 }));
        }), kind[0].toUpperCase() + kind.slice(1)));
      }
    }
    box.replaceChildren(...items);
  }

  // ---------------------------------------------------------- the month

  const recordAt = (a, key, t) => (REC[a][key] ? REC[a][key][t - 1] : NaN);
  function dateOf(t) {
    return { year: REC[0].year[t - 1], month: REC[0].month[t - 1] };
  }
  function seasonOf(month) {
    const c = DATA.calendar;
    const parts = [];
    if (c.shearing === month) parts.push("shearing");
    if (c.harvest.includes(month)) parts.push("harvest");
    if (c.sowing.includes(month)) parts.push("sowing");
    if (c.tax === month) parts.push("the state's tax");
    const season = [12, 1, 2].includes(month) ? "Winter" : month <= 5 ? "Spring" : month <= 8 ? "Summer" : "Autumn";
    return parts.length ? `${season}: ${parts.join(", ")}` : season;
  }

  function drawDate() {
    const { year, month } = dateOf(state.month);
    $("date").textContent = `${MONTH_NAMES[month]}, year ${year}`;
    $("season").textContent = seasonOf(month);
    $("month-label").textContent = `Month ${state.month} of ${M}`;
    $("month").value = String(state.month);
    $("month").setAttribute("aria-valuetext", `${MONTH_NAMES[month]}, year ${year}`);
  }

  function famineChip(v) {
    if (!Number.isFinite(v)) return null;
    if (v < FAMINE) return el("span", { class: "chip famine", text: "Famine" });
    if (v < SHORT) return el("span", { class: "chip short", text: "Short" });
    return null;
  }
  const GLANCE = [
    { key: "population", label: "People", fmt: fmtInt },
    { key: "ration", label: "Food eaten", fmt: (v) => fmtPct(v), chip: true },
    { key: "town_ration", label: "In the towns", fmt: (v) => fmtPct(v), chip: true },
    { key: "food_price", label: "Grain, a ration", fmt: fmtLevel },
    { key: "price_tools", label: "Tools", fmt: fmtLevel },
    { key: "price_salt", label: "Salt", fmt: fmtLevel },
    { key: "wage", label: "Pay, a month", fmt: fmtLevel },
    { key: "real_wage", label: "Real wage", fmt: fmtLevel },
    { key: "deaths", label: "Deaths", fmt: fmtInt },
    { key: "emigrants", label: "Left", fmt: fmtInt },
    { key: "life_expectancy", label: "Life expectancy", fmt: (v) => (Number.isFinite(v) ? v.toFixed(1) + " years" : "–") },
  ].filter((r) => REC[0][r.key]);
  function drawGlance() {
    const table = $("glance");
    const now = armOf(state.view);
    const head = PAIRED
      ? el("tr", {}, el("th", { text: "This month" }),
        el("th", {}, el("i", { class: "swatch without" }), "Without"),
        el("th", {}, el("i", { class: "swatch with" }), "With"))
      : el("tr", {}, el("th", { text: "This month" }), el("th", { text: "The region" }));
    const rows = GLANCE.map((r) => {
      const cells = [el("td", { text: r.label })];
      for (let a = 0; a < ARMS; a++) {
        const v = recordAt(a, r.key, state.month);
        const strong = !PAIRED || a === now || state.view === "gap";
        cells.push(el("td", { class: strong ? "now" : "other" }, r.fmt(v), r.chip ? famineChip(v) : null));
      }
      return el("tr", {}, ...cells);
    });
    table.replaceChildren(el("thead", {}, head), el("tbody", {}, ...rows));
  }

  // ----------------------------------------------------------- chronicle

  const CHRON = DATA.chronicle;
  function entriesOf(a, t, rankMax) {
    return CHRON[a].entries.filter((e) => e.m === t && e.r <= rankMax)
      .sort((x, y) => x.r - y.r || y.n - x.n);
  }
  function drawChronicle() {
    const a = armOf(state.view);
    const list = $("entries");
    const here = entriesOf(a, state.month, state.rankMax);
    if (!here.length) {
      list.replaceChildren(el("li", { class: "empty-note", text: `Nothing of ${state.rankMax === 4 ? "any rank" : "rank " + (state.rankMax === 1 ? "1" : "1 to " + state.rankMax)} this month.` }));
    } else {
      list.replaceChildren(...here.map((e) => {
        const where = [];
        e.p.forEach(([p, name, what], i) => {
          if (i) where.push("; ");
          where.push(p >= 0
            ? el("button", { type: "button", onclick: (ev) => { ev.stopPropagation(); select(p, true); }, text: name })
            : name);
          where.push(`: ${what}`);
        });
        if (e.more) where.push(`; and ${fmtInt(e.more)} more`);
        const pressed = state.litEntry === e;
        return el("li", {
          class: "entry",
          tabindex: 0,
          role: "button",
          "aria-pressed": pressed ? "true" : "false",
          title: e.p.length ? "Light up its places on the map" : null,
          onclick: () => light(e),
          onkeydown: (ev) => { if (ev.key === "Enter" || ev.key === " ") { ev.preventDefault(); light(e); } },
        },
        el("span", { class: `rank r${e.r}`, "aria-label": `rank ${e.r}`, text: String(e.r) }),
        el("div", {}, el("p", { class: "what", text: e.w }), where.length ? el("p", { class: "where" }, ...where) : null));
      }));
    }
    const { year } = dateOf(state.month);
    const y = CHRON[a].years.find((x) => x.year === year);
    const brief = $("year-brief");
    if (!y) { brief.replaceChildren(); return; }
    const items = y.brief.map((line) => {
      const m = line.match(/^\[(\d)\]\s*(.*)$/);
      const rank = m ? m[1] : "";
      return el("li", {}, el("span", { class: `rank r${rank}`, text: rank }), el("span", { text: m ? m[2] : line }));
    });
    brief.replaceChildren(el("h3", { text: `Year ${year} in brief` }), el("p", { class: "totals", text: y.line.replace(/^Year \d+ /, "") }),
      items.length ? el("ul", {}, ...items) : null);
    const news = newsMonths(a);
    $("news-prev").disabled = !news.some((t) => t < state.month);
    $("news-next").disabled = !news.some((t) => t > state.month);
  }
  const newsCache = [];
  function newsMonths(a) {
    if (!newsCache[a]) newsCache[a] = [...new Set(CHRON[a].entries.filter((e) => e.r <= 2).map((e) => e.m))].sort((x, y) => x - y);
    return newsCache[a];
  }
  function light(entry) {
    if (state.litEntry === entry) { state.litEntry = null; state.lit = []; } else {
      state.litEntry = entry;
      state.lit = entry.p.map(([p]) => p).filter((p) => p >= 0);
    }
    drawMap();
    drawChronicle();
  }

  // --------------------------------------------------------------- place

  const PLACE_CHARTS = [
    { key: "population", label: "People", fmt: fmtInt },
    { key: "ration", label: "Food eaten", fmt: (v) => fmtPct(v), lines: [SHORT, FAMINE] },
    { key: "food_price", label: "Grain", fmt: fmtLevel },
    { key: "price_tools", label: "Tools", fmt: fmtLevel },
    { key: "price_salt", label: "Salt", fmt: fmtLevel },
    { key: "store", label: "Grain in store", fmt: fmtLevel },
    { key: "pay", label: "Pay", fmt: fmtLevel },
    { key: "coins", label: "Coins a head", fmt: fmtLevel },
    { key: "debt", label: "Debt a family", fmt: fmtLevel },
    { key: "real_wage", label: "Real wage", fmt: fmtLevel },
    { key: "cost_of_living", label: "Cost of living", fmt: fmtLevel },
    { key: "wealth", label: "Wealth a head", fmt: fmtLevel },
    { key: "deaths", label: "Deaths", fmt: fmtInt },
    { key: "left", label: "Leaving", fmt: fmtInt },
    { key: "arrived", label: "Arriving", fmt: fmtInt },
  ].filter((c) => c.key in SI);
  let placeCharts = [];
  function drawPlace() {
    const panel = $("place-panel");
    const p = state.selected;
    if (p < 0) {
      placeCharts = [];
      panel.replaceChildren(el("p", { class: "empty-note", text: "Choose a place on the map, or a place named in the chronicle, to see how it fared." }));
      return;
    }
    const facts = [el("li", { text: kindName(p) }), el("li", { text: DATA.places.terrain[p] })];
    for (const r of DATA.places.resources[p]) facts.push(el("li", { text: r }));
    const market = DATA.places.market[p];
    if (market >= 0 && market !== p) {
      facts.push(el("li", {}, "Market: ", el("button", { type: "button", onclick: () => select(market, true), text: DATA.places.name[market] })));
    }
    if (DATA.places.border[p]) facts.push(el("li", { text: "Trades beyond the region" }));
    if (DATA.by_town && DATA.places.villages[p]) facts.push(el("li", { text: `With its ${DATA.places.villages[p]} villages` }));
    const start = DATA.places.people[p];
    const now = peopleIn(armOf(state.view), state.month, p);
    const stats = el("p", { class: "place-stats", text: `${fmtInt(start)} people at the start, ${fmtInt(now)} now (${fmtSigned(fmtPct(Math.abs(now / start - 1)), now / start - 1)})` });
    const grid = el("div", { class: "place-charts" });
    placeCharts = PLACE_CHARTS.map((c) => {
      const s = SI[c.key];
      const lines = [];
      for (let a = 0; a < ARMS; a++) {
        const arr = new Float64Array(M);
        for (let t = 1; t <= M; t++) arr[t - 1] = raw(a, s, t, p);
        lines.push(arr);
      }
      const chart = makeChart({ title: c.label, unit: c.key === "deaths" || c.key === "left" || c.key === "arrived" ? "people a month" : SERIES[s].unit, lines, fmt: c.fmt, guides: c.lines, compact: true });
      grid.append(chart.node);
      return chart;
    });
    panel.replaceChildren(
      el("div", { class: "place-head" }, el("h3", { text: DATA.places.name[p] }),
        el("button", { type: "button", class: "quiet", onclick: () => select(-1), text: "Clear" })),
      el("ul", { class: "facts" }, ...facts), stats, grid);
    placeCharts.forEach((c) => c.draw());
  }
  function select(p, show) {
    state.selected = p;
    drawPlace();
    drawMap();
    if (show) showTab("place");
  }
  function showTab(tab) {
    const place = tab === "place";
    $("tab-place").setAttribute("aria-selected", String(place));
    $("tab-chronicle").setAttribute("aria-selected", String(!place));
    $("place-panel").hidden = !place;
    $("chronicle-panel").hidden = place;
    if (place) placeCharts.forEach((c) => c.draw());
  }

  // -------------------------------------------------------------- charts

  // A small chart of a measure over the run: the run without the forced
  // events, and with them; a cursor at the month shown.
  function makeChart(spec) {
    const node = el("div", { class: "chart" });
    const values = el("div", { class: "chart-values" });
    node.append(el("div", { class: "chart-head" },
      el("div", { class: "chart-title" }, spec.title, spec.unit ? el("span", { class: "chart-unit", text: spec.unit }) : null), values));
    const plot = svg("svg", { role: "img" });
    const titleNode = svg("title", {});
    titleNode.textContent = `${spec.title}${spec.unit ? ", " + spec.unit : ""}, month by month`;
    plot.append(titleNode);
    node.append(plot);
    const tip = el("div", { class: "chart-tip", hidden: true });
    node.append(tip);
    let geom = null;
    const H = spec.compact ? 72 : 100;
    const top = 6, bottom = 16;
    function domain() {
      let lo = Infinity, hi = -Infinity;
      for (const line of spec.lines) for (const v of line) if (Number.isFinite(v)) { lo = Math.min(lo, v); hi = Math.max(hi, v); }
      if (spec.guides) for (const g of spec.guides) { lo = Math.min(lo, g); hi = Math.max(hi, g); }
      if (!Number.isFinite(lo)) { lo = 0; hi = 1; }
      if (hi === lo) { hi = lo + (Math.abs(lo) || 1); lo = Math.min(0, lo); }
      const step = niceStep(hi - lo, 2);
      return { lo: Math.floor(lo / step) * step, hi: Math.ceil(hi / step) * step, step };
    }
    function draw() {
      const W = Math.max(120, node.clientWidth - 20);
      if (!node.isConnected || node.clientWidth === 0) return;
      const d = domain();
      const labels = [];
      for (let v = d.lo; v <= d.hi + d.step / 2; v += d.step) labels.push([v, spec.fmt(v)]);
      // room on the left for the longest label
      const left = 6 + 5.6 * Math.max(...labels.map(([, text]) => text.length));
      const right = W - 2;
      const x = (t) => left + ((t - 1) / Math.max(1, M - 1)) * (right - left);
      const y = (v) => top + (1 - (v - d.lo) / (d.hi - d.lo)) * (H - top - bottom);
      geom = { x, y, W, left, right };
      plot.setAttribute("viewBox", `0 0 ${W} ${H}`);
      plot.replaceChildren(titleNode);
      for (const [v, text] of labels) {
        plot.append(svg("line", { class: "grid", x1: left, x2: right, y1: y(v), y2: y(v) }));
        const t = svg("text", { class: "axis-label", x: left - 4, y: y(v) + 3, "text-anchor": "end" });
        t.textContent = text;
        plot.append(t);
      }
      const years = Math.ceil(M / 12);
      const every = years > 6 ? 2 : 1;
      for (let yr = 0; yr < years; yr += every) {
        const t = svg("text", { class: "axis-label", x: x(yr * 12 + 1), y: H - 3 });
        t.textContent = `Y${yr + 1}`;
        plot.append(t);
      }
      if (spec.guides) {
        for (const g of spec.guides) plot.append(svg("line", { class: "event-line", x1: left, x2: right, y1: y(g), y2: y(g), style: "stroke: var(--muted)" }));
      }
      if (PAIRED) for (const e of DATA.events) plot.append(svg("line", { class: "event-line", x1: x(e.month), x2: x(e.month), y1: top, y2: H - bottom }));
      spec.lines.forEach((line, a) => {
        let path = "";
        let pen = false;
        for (let t = 1; t <= M; t++) {
          const v = line[t - 1];
          if (!Number.isFinite(v)) { pen = false; continue; }
          path += `${pen ? "L" : "M"}${x(t).toFixed(1)},${y(v).toFixed(1)}`;
          pen = true;
        }
        const cls = PAIRED ? (a ? "with" : "without") : "with";
        plot.append(svg("path", { class: `line ${cls}`, d: path }));
      });
      geom.cursor = svg("line", { class: "cursor", y1: top, y2: H - bottom });
      plot.append(geom.cursor);
      geom.dots = spec.lines.map((_, a) => {
        const dot = svg("circle", { class: `dot ${PAIRED ? (a ? "with" : "without") : "with"}`, r: 3.5 });
        plot.append(dot);
        return dot;
      });
      geom.hover = svg("line", { class: "hover-line", y1: top, y2: H - bottom, visibility: "hidden" });
      plot.append(geom.hover);
      setMonth(state.month);
    }
    function valueLine(a, v) {
      const cls = PAIRED ? (a ? "with" : "without") : "with";
      return el("span", {}, el("i", { class: `swatch ${cls}` }), spec.fmt(v));
    }
    function setMonth(t) {
      if (!geom) return;
      const cx = geom.x(t);
      geom.cursor.setAttribute("x1", cx); geom.cursor.setAttribute("x2", cx);
      spec.lines.forEach((line, a) => {
        const v = line[t - 1];
        const dot = geom.dots[a];
        if (Number.isFinite(v)) { dot.setAttribute("cx", cx); dot.setAttribute("cy", geom.y(v)); dot.setAttribute("visibility", "visible"); } else dot.setAttribute("visibility", "hidden");
      });
      values.replaceChildren(...spec.lines.map((line, a) => valueLine(a, line[t - 1])));
    }
    function monthAt(event) {
      const rect = plot.getBoundingClientRect();
      const fx = ((event.clientX - rect.left) / rect.width) * geom.W;
      return Math.max(1, Math.min(M, Math.round(1 + ((fx - geom.left) / (geom.right - geom.left)) * (M - 1))));
    }
    plot.addEventListener("pointermove", (event) => {
      if (!geom) return;
      const t = monthAt(event);
      const cx = geom.x(t);
      geom.hover.setAttribute("x1", cx); geom.hover.setAttribute("x2", cx); geom.hover.setAttribute("visibility", "visible");
      const { year, month } = dateOf(t);
      tip.replaceChildren(el("div", { text: `${SHORT_MONTHS[month]} year ${year}` }), ...spec.lines.map((line, a) => valueLine(a, line[t - 1])));
      tip.hidden = false;
      const rect = node.getBoundingClientRect();
      const px = event.clientX - rect.left;
      tip.style.left = (px > rect.width / 2 ? px - tip.offsetWidth - 10 : px + 10) + "px";
    });
    plot.addEventListener("pointerleave", () => { if (geom) geom.hover.setAttribute("visibility", "hidden"); tip.hidden = true; });
    plot.addEventListener("click", (event) => { if (geom) goTo(monthAt(event)); });
    return { node, draw, setMonth };
  }

  // A measure over the last twelve months, month by month (in the first
  // year, the months so far as a year's worth).
  function lastYear(values) {
    const out = [];
    let sum = 0;
    values.forEach((v, t) => { sum += v; if (t >= 12) sum -= values[t - 12]; out.push((sum * 12) / Math.min(t + 1, 12)); });
    return out;
  }
  const ratio = (a, b) => a.map((v, t) => (b[t] > 0 ? v / b[t] : NaN));
  const REGION_CHARTS = [
    { group: "Economy and welfare", note: "The cost of living is a grown-up's grain, firewood and salt for a month. Output (everything the trades made, at each month's prices) and families' income are in months of those necessities, so that a dearth or more coins don't make them look bigger. Wealth is what families own (land, animals, grain, coins and claims) less what they owe. Life expectancy is at birth, at the last twelve months' chances of dying.", items: [
      { label: "Output a head", unit: "the last twelve months, in months of necessities", make: (r) => ratio(lastYear(ratio(r.output, r.cost_of_living)), r.population), fmt: fmtLevel },
      { label: "Families' income a head", unit: "the last twelve months, in months of necessities", make: (r) => ratio(lastYear(ratio(r.family_income, r.cost_of_living)), r.population), fmt: fmtLevel },
      { key: "cost_of_living", label: "Cost of living", unit: "coins a month for a grown-up", fmt: fmtLevel },
      { key: "real_wage", label: "Real wage", unit: "months of necessities a month's pay buys", fmt: fmtLevel },
      { key: "coins_held", label: "Coins in the region", unit: "held by families, trades, councils and dealers", fmt: fmtInt },
      { key: "coins_gained", label: "Coins gained or lost", unit: "a month, with the world beyond, lords and state", fmt: fmtInt },
      { key: "wealth", label: "Wealth a head", unit: "coins", fmt: fmtLevel },
      { key: "wealth_gini", label: "Inequality of wealth", unit: "Gini, among people", fmt: (v) => (Number.isFinite(v) ? v.toFixed(2) : "–") },
      { key: "top_tenth_wealth", label: "The richest tenth's share", unit: "of all families' wealth", fmt: (v) => fmtPct(v) },
      { key: "life_expectancy", label: "Life expectancy", unit: "years, at birth", fmt: (v) => (Number.isFinite(v) ? v.toFixed(1) : "–") },
      { key: "under_five_deaths", label: "Deaths before five", unit: "of 1,000 born", fmt: fmtInt },
      { label: "Underfed", unit: "share of people", make: (r) => ratio(r.underfed, r.population), fmt: (v) => fmtPct(v) },
      { label: "Landless", unit: "share of people", make: (r) => ratio(r.landless, r.population), fmt: (v) => fmtPct(v) },
      { label: "Debt against income", unit: "debts over the last twelve months' income", make: (r) => ratio(r.debt, lastYear(r.family_income)), fmt: (v) => fmtPct(v) },
    ] },
    { group: "People and food", items: [
      { key: "population", label: "People", fmt: fmtInt },
      { key: "ration", label: "Food eaten", unit: "share of need", fmt: (v) => fmtPct(v), guides: [SHORT, FAMINE] },
      { key: "town_ration", label: "Food eaten in the towns", unit: "share of need", fmt: (v) => fmtPct(v), guides: [SHORT, FAMINE] },
      { key: "deaths", label: "Deaths", unit: "a month", fmt: fmtInt },
      { key: "births", label: "Births", unit: "a month", fmt: fmtInt },
      { key: "emigrants", label: "Leaving their homes", unit: "a month", fmt: fmtInt },
      { key: "food_stock", label: "Grain in store", unit: "rations", fmt: fmtInt },
      { key: "underfed", label: "Underfed", unit: "people below 90% of their need", fmt: fmtInt },
    ] },
    { group: "Prices", note: "Coins a unit. Grain is weighed by what each place needs; the other goods are a plain mean over places.", items: [
      { key: "food_price", label: "Grain", unit: "a ration", fmt: fmtLevel },
      { key: "price_tools", label: "Tools", fmt: fmtLevel },
      { key: "price_salt", label: "Salt", fmt: fmtLevel },
      { key: "price_clothing", label: "Cloth", fmt: fmtLevel },
      { key: "price_pottery", label: "Pots", fmt: fmtLevel },
      { key: "price_firewood", label: "Firewood", fmt: fmtLevel },
      { key: "price_iron", label: "Iron", fmt: fmtLevel },
      { key: "price_lead", label: "Lead", fmt: fmtLevel },
      { key: "town_price", label: "Grain beyond the region", unit: "a ration, the outside price", fmt: fmtLevel },
    ] },
    { group: "Money and credit", items: [
      { key: "wage", label: "Pay", unit: "coins a worker, a month", fmt: fmtLevel },
      { key: "wage_cover", label: "What a month's pay buys", unit: "food, against a worker's need", fmt: (v) => fmtPct(v) },
      { key: "savings", label: "Coins families hold", unit: "coins", fmt: fmtInt },
      { key: "debt", label: "Debts owed", unit: "coins", fmt: fmtInt },
      { key: "debtors", label: "Families in debt", fmt: fmtInt },
      { key: "interest", label: "Interest", unit: "coins added to debts a month", fmt: fmtInt },
      { key: "borrowed", label: "Borrowed", unit: "coins a month", fmt: fmtInt },
      { key: "land_price", label: "A plot of land", unit: "coins", fmt: fmtLevel },
      { key: "land_foreclosed", label: "Land taken for debts", unit: "plots a month", fmt: fmtLevel },
      { key: "landless", label: "Landless", unit: "people in families with no land", fmt: fmtInt },
    ] },
    { group: "Towns, trades and beyond", items: [
      { key: "town_people", label: "People in the towns", fmt: fmtInt },
      { key: "staples_bought", label: "Salt got", unit: "share of need", fmt: (v) => fmtPct(v) },
      { key: "jobs_ironworks", label: "Ironworkers", fmt: fmtInt },
      { key: "jobs_smithing", label: "Smiths", fmt: fmtInt },
      { key: "jobs_saltworks", label: "Salt boilers", fmt: fmtInt },
      { key: "jobs_potting", label: "Potters", fmt: fmtInt },
      { key: "jobs_leadworks", label: "Leadworkers", fmt: fmtInt },
      { key: "crafts_sold", label: "Crafts sold beyond", unit: "coins a month", fmt: fmtInt },
      { key: "wool_sold", label: "Wool sold beyond", unit: "coins a month", fmt: fmtInt },
      { key: "woods", label: "Woods", unit: "against the start", fmt: (v) => fmtPct(v) },
    ] },
  ];
  let regionCharts = [];
  function buildRegionCharts() {
    const box = $("charts");
    const legend = $("chart-legend");
    if (PAIRED) {
      legend.replaceChildren(
        el("span", {}, el("i", { class: "swatch without" }), DATA.arms[0].label),
        el("span", {}, el("i", { class: "swatch with" }), DATA.arms[1].label),
        DATA.events.length ? el("span", {}, el("i", { class: "event-mark" }), "The forced events begin") : null);
    }
    regionCharts = [];
    const groups = [];
    for (const group of REGION_CHARTS) {
      // (each chart's runs, read or made from the records; a measure the
      // records lack, or nothing but zeros, gets no chart)
      const items = group.items.map((c) => {
        try {
          return { ...c, lines: REC.map((r) => (c.make ? c.make(r) : r[c.key])) };
        } catch (error) {
          return { ...c, lines: null };
        }
      }).filter((c) => c.lines && c.lines.every(Array.isArray) && c.lines.some((line) => line.some((v) => v !== 0 && Number.isFinite(v))));
      if (!items.length) continue;
      const grid = el("div", { class: "chart-grid" });
      for (const c of items) {
        const chart = makeChart({ title: c.label, unit: c.unit, lines: c.lines, fmt: c.fmt, guides: c.guides });
        grid.append(chart.node);
        regionCharts.push(chart);
      }
      groups.push(el("section", { class: "chart-group" }, el("h3", { text: group.group }), group.note ? el("p", { class: "note", text: group.note }) : null, grid, yearTable(items)));
    }
    box.replaceChildren(...groups);
    regionCharts.forEach((c) => c.draw());
  }
  // The same numbers as a table: each year's mean.
  function yearTable(items) {
    const years = [...new Set(REC[0].year)];
    const head = el("tr", {}, el("th", { text: "Year" }));
    for (const c of items) {
      for (let a = 0; a < ARMS; a++) head.append(el("th", { text: PAIRED ? `${c.label} (${a ? "with" : "without"})` : c.label }));
    }
    const rows = years.map((yr) => {
      const tr = el("tr", {}, el("th", { text: String(yr) }));
      for (const c of items) {
        for (let a = 0; a < ARMS; a++) {
          const vals = c.lines[a].filter((v, i) => REC[0].year[i] === yr && Number.isFinite(v));
          tr.append(el("td", { text: vals.length ? c.fmt(vals.reduce((s, v) => s + v, 0) / vals.length) : "–" }));
        }
      }
      return tr;
    });
    return el("details", { class: "numbers" }, el("summary", { text: "These numbers by year (the mean of each year's months)" }),
      el("div", { class: "table-wrap" }, el("table", {}, el("thead", {}, head), el("tbody", {}, ...rows))));
  }

  // ----------------------------------------------------------- timeline

  const strip = $("strip");
  function drawStrip() {
    const w = strip.clientWidth, h = strip.clientHeight;
    if (!w) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    strip.width = Math.round(w * dpr); strip.height = Math.round(h * dpr);
    const g = strip.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    // the thumb's centre runs from 8px in at each end
    const x = (t) => 8 + ((t - 1) / Math.max(1, M - 1)) * (w - 16);
    const a = armOf(state.view);
    const hunger = scaleFor(SI.ration, "with");
    const cw = (w - 16) / Math.max(1, M - 1);
    for (let t = 1; t <= M; t++) {
      const v = recordAt(a, "ration", t);
      g.fillStyle = css(hunger.colour(v));
      g.fillRect(x(t) - cw / 2, 5, cw + 0.5, 6);
    }
    g.fillStyle = tokens.muted;
    for (let t = 1; t <= M; t++) if (dateOf(t).month === 1 && t > 1) g.fillRect(x(t) - 0.5 - cw / 2, 3, 1, 10);
    // the chronicle's biggest news
    g.fillStyle = tokens.ink;
    for (const t of new Set(CHRON[a].entries.filter((e) => e.r === 1).map((e) => e.m))) g.fillRect(x(t) - 1, 0, 2, 4);
    g.fillStyle = tokens.withColour;
    if (PAIRED && state.view !== "without") {
      for (const e of DATA.events) {
        g.beginPath(); g.moveTo(x(e.month) - 4, 16); g.lineTo(x(e.month) + 4, 16); g.lineTo(x(e.month), 11); g.fill();
      }
    }
  }

  let lastFrame = 0, carried = 0;
  function tick(now) {
    if (!state.playing) return;
    if (lastFrame) carried += ((now - lastFrame) / 1000) * state.speed;
    lastFrame = now;
    if (carried >= 1) {
      const step = Math.floor(carried);
      carried -= step;
      if (state.month >= M) { setPlaying(false); return; }
      goTo(Math.min(M, state.month + step));
    }
    requestAnimationFrame(tick);
  }
  function setPlaying(on) {
    state.playing = on;
    $("play").setAttribute("aria-label", on ? "Pause" : "Play");
    $("play-icon").setAttribute("d", on ? "M4 2.5h3v11H4zM9 2.5h3v11H9z" : "M4 2.5v11l9-5.5z");
    if (on) {
      if (state.month >= M) goTo(1);
      lastFrame = 0; carried = 0;
      requestAnimationFrame(tick);
    }
  }

  function goTo(t) {
    state.month = Math.max(1, Math.min(M, t));
    if (state.litEntry && state.litEntry.m !== state.month) { state.litEntry = null; state.lit = []; }
    drawDate();
    if (state.measure !== "terrain" && spanOf(SI[state.measure]) === "month") { if (scaleCache.size > 200) scaleCache.clear(); drawLegend(); }
    drawMap();
    drawGlance();
    drawChronicle();
    regionCharts.forEach((c) => c.setMonth(state.month));
    placeCharts.forEach((c) => c.setMonth(state.month));
    if (state.selected >= 0) {
      const stats = document.querySelector(".place-stats");
      if (stats) {
        const p = state.selected, start = DATA.places.people[p], now = peopleIn(armOf(state.view), state.month, p);
        stats.textContent = `${fmtInt(start)} people at the start, ${fmtInt(now)} now (${fmtSigned(fmtPct(Math.abs(now / start - 1)), now / start - 1)})`;
      }
    }
  }

  // ------------------------------------------------------------ controls

  function buildControls() {
    const sw = $("arm-switch");
    if (PAIRED) {
      const views = [["without", DATA.arms[0].label, "Without"], ["with", DATA.arms[1].label, "With"], ["gap", "The gap between them", "Gap"]];
      sw.replaceChildren(...views.map(([key, text, short]) => el("button", {
        type: "button", role: "radio", "aria-checked": String(state.view === key), "data-view": key,
        "aria-label": text, onclick: () => setView(key),
      }, el("i", { class: `swatch ${key}` }), el("span", { class: "long", text }), el("span", { class: "short", text: short }))));
    } else {
      sw.hidden = true;
    }
    const picker = $("measure");
    const groups = {};
    for (const s of SERIES) (groups[s.group] = groups[s.group] || []).push(s);
    picker.replaceChildren(...Object.entries(groups).map(([name, list]) =>
      el("optgroup", { label: name }, ...list.map((s) => el("option", { value: s.key, text: s.label })))),
    el("optgroup", { label: "The land" }, el("option", { value: "terrain", text: "Terrain" })));
    picker.value = state.measure;
    picker.addEventListener("change", () => { state.measure = picker.value; drawLegend(); drawMap(); });
    const layer = (id, key) => $(id).addEventListener("change", (e) => { state[key] = e.target.checked; drawSymbols(); drawMap(); });
    $("layer-villages").parentElement.hidden = !hasVillages;
    layer("layer-roads", "roads"); layer("layer-resources", "resources"); layer("layer-villages", "villages");
    $("month").max = String(M);
    $("month").addEventListener("input", (e) => goTo(Number(e.target.value)));
    $("play").addEventListener("click", () => setPlaying(!state.playing));
    $("speed").addEventListener("change", (e) => { state.speed = Number(e.target.value); });
    $("rank").addEventListener("change", (e) => { state.rankMax = Number(e.target.value); drawChronicle(); });
    $("scale").addEventListener("change", (e) => {
      if (state.measure !== "terrain") state.span[state.measure] = e.target.value;
      scaleCache.clear(); drawLegend(); drawMap();
    });
    $("news-prev").addEventListener("click", () => {
      const before = newsMonths(armOf(state.view)).filter((t) => t < state.month);
      if (before.length) goTo(before[before.length - 1]);
    });
    $("news-next").addEventListener("click", () => {
      const after = newsMonths(armOf(state.view)).find((t) => t > state.month);
      if (after) goTo(after);
    });
    $("tab-chronicle").addEventListener("click", () => showTab("chronicle"));
    $("tab-place").addEventListener("click", () => showTab("place"));
    map.canvas.addEventListener("pointermove", (e) => {
      const p = placeAt(e);
      if (p !== hoverPlace) { hoverPlace = p; drawMap(); }
      showTip(e, p);
    });
    map.canvas.addEventListener("pointerleave", () => { hoverPlace = -1; $("map-tip").hidden = true; drawMap(); });
    map.canvas.addEventListener("click", (e) => { const p = placeAt(e); if (p >= 0) select(p, true); });
    document.addEventListener("keydown", (e) => {
      const tag = (e.target.tagName || "").toLowerCase();
      if (["input", "select", "textarea", "button"].includes(tag) || e.target.getAttribute("role") === "button") return;
      if (e.key === " ") { e.preventDefault(); setPlaying(!state.playing); }
      if (e.key === "ArrowRight") { e.preventDefault(); goTo(state.month + 1); }
      if (e.key === "ArrowLeft") { e.preventDefault(); goTo(state.month - 1); }
    });
  }
  function setView(view) {
    state.view = view;
    for (const b of $("arm-switch").querySelectorAll("button")) b.setAttribute("aria-checked", String(b.dataset.view === view));
    drawLegend();
    drawStrip();
    goTo(state.month);
    drawPlace();
  }

  function writeColophon() {
    const about = DATA.about;
    const parts = [
      el("p", { text: about.map }),
      el("p", { text: `Seed ${about.seed}; ${M} months; ${about.random_events ? "random events on" : "only the forced events"}; families ${about.minds ? "with minds of their own" : "following the rules"}.${about.start ? ` Started from “${about.start}”: ${about.start_about}` : ""}${about.changes.length ? ` Changed: ${about.changes.join(", ")}.` : ""}` }),
      el("p", {}, "Made by ", el("code", { text: about.command }), ". The chronicle's ranks: ", DATA.ranks.join("; "), "."),
    ];
    if (DATA.by_town) parts.push(el("p", { text: "Each market town is shown with the villages that market there: counts are added up, the rest weighed by people." }));
    $("colophon").replaceChildren(...parts);
    const events = DATA.events.map((e) => e.label).join(", ");
    const years = M % 12 === 0 ? `${M / 12} year${M === 12 ? "" : "s"}` : `${M} months`;
    const sub = [`${fmtInt(about.people)} people in ${fmtInt(about.places)} places`, years];
    if (about.start) sub.push(`from the “${about.start}” start`);
    if (events) sub.push(`${events}, beside the same seed without it`);
    $("subtitle").textContent = sub.join(" · ");
  }

  // The month to open at: the hungriest after the forced events begin.
  function openingMonth() {
    const a = PAIRED ? 1 : 0;
    const from = firstEvent || 1;
    let best = from, worst = Infinity;
    for (let t = from; t <= M; t++) {
      const v = recordAt(a, "ration", t);
      if (v < worst) { worst = v; best = t; }
    }
    return best;
  }

  function redrawAll() {
    readTokens();
    scaleCache.clear();
    drawLines();
    drawLegend();
    drawSymbols();
    drawStrip();
    drawMap();
    regionCharts.forEach((c) => c.draw());
    placeCharts.forEach((c) => c.draw());
  }

  async function start() {
    readTokens();
    buildControls();
    writeColophon();
    drawSymbols();
    try {
      await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));
      await unpack();
    } catch (error) {
      $("loading").textContent = error.message;
      throw error;
    }
    $("loading").hidden = true;
    state.month = openingMonth();
    layoutMap();
    drawLegend();
    buildRegionCharts();
    drawPlace();
    drawStrip();
    goTo(state.month);
    let timer = 0;
    new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => { layoutMap(); drawStrip(); drawMap(); regionCharts.forEach((c) => c.draw()); placeCharts.forEach((c) => c.draw()); }, 120);
    }).observe(document.body);
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", redrawAll);
    new MutationObserver(redrawAll).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  }
  start();
})();
