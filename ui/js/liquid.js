// Flowing monochrome light field behind the glass. Rendered at low
// resolution (CSS blurs it), so it costs almost nothing.
export class LiquidBackground {
  constructor(canvas, audio) {
    this.c = canvas;
    this.g = canvas.getContext('2d');
    this.audio = audio;
    this.t = Math.random() * 100;
    this.light = false;
    this.blobs = Array.from({ length: 6 }, (_, i) => ({
      ax: 0.2 + Math.random() * 0.6, ay: 0.2 + Math.random() * 0.6,
      fx: 0.05 + Math.random() * 0.09, fy: 0.04 + Math.random() * 0.08,
      ph: Math.random() * 6.28, r: 0.28 + Math.random() * 0.22,
      a: i < 2 ? 0.34 : 0.2, band: [0, 4, 14, 26, 40, 58][i],
    }));
    this.resize();
    new ResizeObserver(() => this.resize()).observe(canvas);
  }

  resize() {
    const r = this.c.getBoundingClientRect();
    this.w = this.c.width = Math.max(32, Math.round(r.width / 6));
    this.h = this.c.height = Math.max(32, Math.round(r.height / 6));
  }

  draw(dt) {
    const { g, w, h, audio } = this;
    this.t += dt * (0.35 + audio.energy * 1.6);
    g.globalCompositeOperation = 'source-over';
    g.clearRect(0, 0, w, h);
    g.globalCompositeOperation = this.light ? 'multiply' : 'lighter';
    const base = this.light ? '0,0,0' : '255,255,255';
    const m = Math.max(w, h);
    for (const b of this.blobs) {
      const x = (b.ax + Math.sin(this.t * b.fx * 6 + b.ph) * 0.28) * w;
      const y = (b.ay + Math.cos(this.t * b.fy * 6 + b.ph * 1.3) * 0.28) * h;
      const lvl = audio.bands[b.band] || 0;
      const r = m * b.r * (1 + lvl * 0.45 + audio.beat * 0.12);
      const a = b.a * (0.35 + lvl * 0.9) * (this.light ? 0.45 : 1);
      const grd = g.createRadialGradient(x, y, 0, x, y, r);
      grd.addColorStop(0, `rgba(${base},${a})`);
      grd.addColorStop(0.45, `rgba(${base},${a * 0.35})`);
      grd.addColorStop(1, `rgba(${base},0)`);
      g.fillStyle = grd;
      g.fillRect(0, 0, w, h);
    }
  }
}
