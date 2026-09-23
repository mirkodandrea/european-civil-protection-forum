// Shared helpers for the TTX web UIs: API calls, live updates, the map.

export async function api(path, body, method) {
  const opts = { method: method || (body === undefined ? 'GET' : 'POST'), headers: {} };
  if (body !== undefined) {
    opts.headers['content-type'] = 'application/json';
    opts.body = JSON.stringify(body);
  }
  const r = await fetch(path, opts);
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
}

/// Call `onChange` whenever the exercise changes (WebSocket, with polling fallback).
export function live(onChange) {
  let timer = null;
  const fire = () => { clearTimeout(timer); timer = setTimeout(onChange, 60); };
  function connect() {
    const ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`);
    ws.onmessage = fire;
    ws.onclose = () => setTimeout(connect, 1500);
  }
  connect();
  setInterval(onChange, 10000);
  onChange();
}

export function esc(s) {
  return String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
}

export function b64bytes(s) {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export const LANGS = { en: 'English', it: 'Italiano', fr: 'Français', de: 'Deutsch', es: 'Español', hy: 'Հայերեն', ka: 'ქართული', uk: 'Українська', ro: 'Română', az: 'Azərbaycan', el: 'Ελληνικά', pt: 'Português' };

const FUEL_RGB = {
  1: [107, 142, 78], 2: [154, 165, 90], 3: [214, 208, 196], 4: [217, 210, 138],
  5: [63, 107, 58], 6: [194, 183, 110], 7: [127, 163, 122],
};

/// A canvas map of the scenario: fuel, fire, zones, roads, buildings, actions.
export class MapView {
  constructor(canvas, stat, opts = {}) {
    this.c = canvas;
    this.ctx = canvas.getContext('2d');
    this.s = stat;
    this.opts = opts;
    this.view = null;
    this.hover = null;
    this.pending = null; // first click of a two-click tool
    this.W = stat.grid.cols * stat.grid.cellsize;
    this.H = stat.grid.rows * stat.grid.cellsize;
    this.fuelImg = this.buildFuel();
    this.fireCanvas = document.createElement('canvas');
    this.fireCanvas.width = stat.grid.cols;
    this.fireCanvas.height = stat.grid.rows;
    canvas.addEventListener('mousemove', e => { this.hover = this.toWorld(e); this.draw(); });
    canvas.addEventListener('mouseleave', () => { this.hover = null; this.draw(); });
    canvas.addEventListener('click', e => opts.onClick && opts.onClick(this.toWorld(e), this));
    new ResizeObserver(() => this.resize()).observe(canvas.parentElement);
    this.resize();
  }

  buildFuel() {
    const { rows, cols } = this.s.grid;
    const fuel = b64bytes(this.s.fuel);
    const cv = document.createElement('canvas');
    cv.width = cols; cv.height = rows;
    const g = cv.getContext('2d');
    const img = g.createImageData(cols, rows);
    for (let i = 0; i < rows * cols; i++) {
      const [r, gg, b] = FUEL_RGB[fuel[i]] || [200, 200, 200];
      img.data.set([r, gg, b, 255], i * 4);
    }
    g.putImageData(img, 0, 0);
    return cv;
  }

  resize() {
    const box = this.c.parentElement.getBoundingClientRect();
    const size = Math.max(200, Math.min(box.width, this.opts.maxHeight || box.width));
    const dpr = window.devicePixelRatio || 1;
    this.c.style.width = size + 'px';
    this.c.style.height = size + 'px';
    this.c.width = size * dpr;
    this.c.height = size * dpr;
    this.scale = (size * dpr) / this.W;
    this.draw();
  }

  toWorld(e) {
    const r = this.c.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * this.W;
    const y = this.H - ((e.clientY - r.top) / r.height) * this.H;
    return { x: Math.round(x), y: Math.round(y), zone: this.zoneOf(x, y) };
  }

  zoneOf(x, y) {
    const z = this.s.zones;
    const c = Math.floor(x / z.size_m), r = Math.floor((this.H - y) / z.size_m);
    if (c < 0 || r < 0 || c >= z.cols || r >= z.rows) return null;
    return String.fromCharCode(65 + c) + (r + 1);
  }

  px(p) { return [p[0] * this.scale, (this.H - p[1]) * this.scale]; }

  setView(view) {
    this.view = view;
    const { rows, cols } = this.s.grid;
    const fire = b64bytes(view.fire);
    const g = this.fireCanvas.getContext('2d');
    const img = g.createImageData(cols, rows);
    for (let i = 0; i < rows * cols; i++) {
      const v = fire[i];
      const rgba = v === 1 ? [255, 70, 20, 235] : v === 2 ? [35, 30, 28, 215] : v === 3 ? [150, 90, 30, 230] : v === 4 ? [60, 150, 255, 110] : v === 5 ? [255, 150, 40, 80] : [0, 0, 0, 0];
      img.data.set(rgba, i * 4);
    }
    g.putImageData(img, 0, 0);
    this.draw();
  }

  draw() {
    const ctx = this.ctx, k = this.scale;
    if (!k) return;
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(this.fuelImg, 0, 0, this.W * k, this.H * k);
    ctx.drawImage(this.fireCanvas, 0, 0, this.W * k, this.H * k);
    const dpr = window.devicePixelRatio || 1;
    const v = this.view;

    // Zone grid.
    const z = this.s.zones;
    ctx.strokeStyle = 'rgba(0,0,0,0.25)';
    ctx.lineWidth = 1 * dpr;
    ctx.font = `${11 * dpr}px system-ui`;
    for (let c = 0; c <= z.cols; c++) { ctx.beginPath(); ctx.moveTo(c * z.size_m * k, 0); ctx.lineTo(c * z.size_m * k, this.H * k); ctx.stroke(); }
    for (let r = 0; r <= z.rows; r++) { ctx.beginPath(); ctx.moveTo(0, r * z.size_m * k); ctx.lineTo(this.W * k, r * z.size_m * k); ctx.stroke(); }
    const warned = new Set(v ? v.warned : []);
    const burning = new Set(v ? v.burning_zones : []);
    for (let r = 0; r < z.rows; r++) for (let c = 0; c < z.cols; c++) {
      const name = String.fromCharCode(65 + c) + (r + 1);
      if (warned.has(name)) { ctx.fillStyle = 'rgba(255,200,0,0.13)'; ctx.fillRect(c * z.size_m * k, r * z.size_m * k, z.size_m * k, z.size_m * k); }
      ctx.fillStyle = burning.has(name) ? 'rgba(200,30,0,0.9)' : 'rgba(0,0,0,0.45)';
      ctx.fillText(name, c * z.size_m * k + 4 * dpr, r * z.size_m * k + 13 * dpr);
    }

    // Roads.
    for (const road of this.s.roads) {
      const st = v && v.roads[road.id];
      ctx.strokeStyle = st && st.cut_by_fire ? '#c00' : st && st.closed ? '#e08000' : '#fff';
      ctx.lineWidth = 3 * dpr;
      ctx.setLineDash(st && st.closed ? [6 * dpr, 4 * dpr] : []);
      ctx.beginPath();
      road.points.forEach((p, i) => { const [x, y] = this.px(p); i ? ctx.lineTo(x, y) : ctx.moveTo(x, y); });
      ctx.stroke();
      ctx.setLineDash([]);
    }

    // Buildings.
    const bv = v ? v.buildings : null;
    this.s.buildings.forEach((b, i) => {
      const st = bv && bv[i];
      const [x, y] = this.px(b.pos);
      const w = Math.max(3 * dpr, b.size[0] * k), h = Math.max(3 * dpr, b.size[1] * k);
      let fill = b.name ? '#223' : '#555';
      if (st) {
        if (st.damage === 2) fill = '#000';
        else if (st.damage === 1) fill = '#ff7a00';
        else if (st.evacuating) fill = '#00c8ff';
        else if (st.inside === 0 && st.evacuated > 0) fill = '#2e6bff';
      }
      ctx.fillStyle = fill;
      ctx.fillRect(x - w / 2, y - h / 2, w, h);
      if (st && st.threatened && st.damage === 0) { ctx.strokeStyle = '#ffb000'; ctx.lineWidth = 1.5 * dpr; ctx.strokeRect(x - w / 2 - 1, y - h / 2 - 1, w + 2, h + 2); }
      if (st && st.damage === 2) { ctx.strokeStyle = '#f33'; ctx.lineWidth = 1 * dpr; ctx.beginPath(); ctx.moveTo(x - w / 2, y - h / 2); ctx.lineTo(x + w / 2, y + h / 2); ctx.moveTo(x + w / 2, y - h / 2); ctx.lineTo(x - w / 2, y + h / 2); ctx.stroke(); }
    });

    // Actions: fire lines and drops.
    if (v) {
      for (const l of v.lines) {
        const [x1, y1] = this.px(l.from), [x2, y2] = this.px(l.to);
        ctx.strokeStyle = '#ffe000'; ctx.lineWidth = 2 * dpr; ctx.setLineDash([5 * dpr, 3 * dpr]);
        ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke(); ctx.setLineDash([]);
      }
      for (const d of v.drops) {
        const [x, y] = this.px(d.at);
        ctx.strokeStyle = '#1e90ff'; ctx.lineWidth = 2 * dpr;
        ctx.beginPath(); ctx.arc(x, y, d.radius_m * k, 0, Math.PI * 2); ctx.stroke();
      }
    }

    // Named facilities and places.
    ctx.font = `${11 * dpr}px system-ui`;
    ctx.fillStyle = '#111';
    for (const b of this.s.buildings) if (b.name) { const [x, y] = this.px(b.pos); label(ctx, b.name, x + 6 * dpr, y - 4 * dpr, dpr); }
    for (const p of this.s.places) if (!p.name.includes(' edge') && !p.name.includes('junction')) { const [x, y] = this.px(p.pos); ctx.fillStyle = '#333'; label(ctx, p.name, x, y, dpr, true); }

    // Two-click tool marker and hover readout.
    if (this.pending) {
      const [x, y] = this.px([this.pending.x, this.pending.y]);
      ctx.fillStyle = '#ffe000'; ctx.beginPath(); ctx.arc(x, y, 5 * dpr, 0, Math.PI * 2); ctx.fill();
      if (this.hover) { const [hx, hy] = this.px([this.hover.x, this.hover.y]); ctx.strokeStyle = '#ffe000'; ctx.beginPath(); ctx.moveTo(x, y); ctx.lineTo(hx, hy); ctx.stroke(); }
    }
    if (this.hover) {
      const t = `${this.hover.zone || ''}  (${this.hover.x}, ${this.hover.y})`;
      ctx.fillStyle = 'rgba(255,255,255,0.85)';
      ctx.fillRect(4 * dpr, this.H * k - 20 * dpr, ctx.measureText(t).width + 10 * dpr, 16 * dpr);
      ctx.fillStyle = '#000';
      ctx.fillText(t, 9 * dpr, this.H * k - 8 * dpr);
    }
  }

  nearestBuilding(p, maxDist = 60) {
    let best = null, bd = maxDist * maxDist;
    for (const b of this.s.buildings) {
      const d = (b.pos[0] - p.x) ** 2 + (b.pos[1] - p.y) ** 2;
      if (d < bd) { bd = d; best = b; }
    }
    return best;
  }
}

function label(ctx, text, x, y, dpr, centred) {
  const w = ctx.measureText(text).width;
  const x0 = centred ? x - w / 2 : x;
  ctx.fillStyle = 'rgba(255,255,255,0.75)';
  ctx.fillRect(x0 - 2 * dpr, y - 10 * dpr, w + 4 * dpr, 13 * dpr);
  ctx.fillStyle = '#111';
  ctx.fillText(text, x0, y);
}

export function statsLine(st) {
  return `burned ${st.burned_ha.toFixed(1)} ha · buildings damaged ${st.damaged}, destroyed ${st.destroyed}, threatened ${st.threatened} · evacuated ${st.evacuated_people} · at risk ${st.people_at_risk} · sheltered ${st.sheltered}`;
}

export function kindBadge(kind) {
  const names = { inject: 'INJECT', reply: 'REPLY', feedback: 'EXCON', clarification: 'QUESTION', situation: 'SITUATION', role_message: 'MESSAGE', system: 'SYSTEM' };
  return `<span class="badge k-${kind}">${names[kind] || kind}</span>`;
}
