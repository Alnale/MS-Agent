/* Idle waveform */
export function drawIdle(c: HTMLCanvasElement, ph: number) {
  const ctx = c.getContext('2d');
  if (!ctx) return;
  const w = c.width, h = c.height;
  ctx.clearRect(0, 0, w, h);
  const n = 32, bw = w / n;
  for (let i = 0; i < n; i++) {
    const t = i / n;
    const b = Math.sin(ph + t * Math.PI * 3) * 0.3 + 0.4;
    const bh = Math.max(2, b * h * 0.5);
    ctx.fillStyle = `rgba(184,169,232,${0.06 + b * 0.08})`;
    ctx.beginPath();
    ctx.roundRect(i * bw + 1, (h - bh) / 2, Math.max(1, bw - 2), bh, 1.5);
    ctx.fill();
  }
}

/* Live waveform */
const PRISM: [number, number, number][] = [[242, 128, 160], [184, 169, 232], [141, 216, 176], [245, 200, 160]];

export function drawLive(c: HTMLCanvasElement, data: Uint8Array) {
  const ctx = c.getContext('2d');
  if (!ctx) return;
  const w = c.width, h = c.height, n = data.length, bw = w / n;
  ctx.clearRect(0, 0, w, h);
  for (let i = 0; i < n; i++) {
    const v = data[i] / 255;
    const bh = Math.max(2, v * h * 0.85);
    const t = i / n * (PRISM.length - 1);
    const ci = Math.min(Math.floor(t), PRISM.length - 2);
    const f = t - ci;
    const r = Math.round(PRISM[ci][0] + (PRISM[ci + 1][0] - PRISM[ci][0]) * f);
    const g = Math.round(PRISM[ci][1] + (PRISM[ci + 1][1] - PRISM[ci][1]) * f);
    const b = Math.round(PRISM[ci][2] + (PRISM[ci + 1][2] - PRISM[ci][2]) * f);
    ctx.fillStyle = `rgba(${r},${g},${b},${0.3 + v * 0.7})`;
    ctx.beginPath();
    ctx.roundRect(i * bw + 1, (h - bh) / 2, Math.max(1, bw - 2), bh, 1.5);
    ctx.fill();
  }
}
