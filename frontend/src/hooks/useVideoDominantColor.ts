import { useEffect, useRef } from 'react';

const SAMPLE_W = 64;
const SAMPLE_H = 48;
const INTERVAL_MS = 300;
const TRIM = 0.15; // trim 15% from each end of luminance distribution

function gammaDecode(c: number): number {
  const s = c / 255;
  return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function luminance(r: number, g: number, b: number): number {
  return 0.2126 * gammaDecode(r) + 0.7152 * gammaDecode(g) + 0.0722 * gammaDecode(b);
}

const DARK = '#1a1a2e';
const LIGHT = '#f0e6f0';
const DARK_LUM = luminance(26, 26, 46);
const LIGHT_LUM = luminance(240, 230, 240);

function bestTextColor(bgLum: number): string {
  const darkCR = (Math.max(DARK_LUM, bgLum) + 0.05) / (Math.min(DARK_LUM, bgLum) + 0.05);
  const lightCR = (Math.max(LIGHT_LUM, bgLum) + 0.05) / (Math.min(LIGHT_LUM, bgLum) + 0.05);
  return darkCR >= lightCR ? DARK : LIGHT;
}

export function useVideoDominantColor(
  videoRef: React.RefObject<HTMLVideoElement | null>,
  bgVideo: string | null,
  videoPlaying: boolean,
  onColorChange: (color: string) => void,
) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval>>(0);
  const lastColorRef = useRef<string>('');

  const cbRef = useRef(onColorChange);
  cbRef.current = onColorChange;

  useEffect(() => {
    const el = videoRef.current;
    if (!el || !bgVideo || !videoPlaying) {
      clearInterval(intervalRef.current);
      return;
    }

    if (!canvasRef.current) {
      canvasRef.current = document.createElement('canvas');
      canvasRef.current.width = SAMPLE_W;
      canvasRef.current.height = SAMPLE_H;
    }
    if (!ctxRef.current) {
      ctxRef.current = canvasRef.current.getContext('2d', { willReadFrequently: true });
    }
    const ctx = ctxRef.current;
    if (!ctx) return;

    // Pre-compute center-weighted gaussian
    const n = SAMPLE_W * SAMPLE_H;
    const weights = new Float32Array(n);
    const cx = SAMPLE_W / 2, cy = SAMPLE_H / 2;
    const sigma = Math.max(SAMPLE_W, SAMPLE_H) * 0.45;
    for (let y = 0; y < SAMPLE_H; y++) {
      for (let x = 0; x < SAMPLE_W; x++) {
        const dx = (x - cx) / sigma, dy = (y - cy) / sigma;
        weights[y * SAMPLE_W + x] = Math.exp(-(dx * dx + dy * dy) / 2);
      }
    }

    // Pre-allocate arrays (avoid GC pressure)
    const lumArr = new Float64Array(n);
    const idxArr = Array.from({ length: n }, (_, i) => i);

    const sample = () => {
      if (el.readyState < 2) return;
      ctx.drawImage(el, 0, 0, SAMPLE_W, SAMPLE_H);
      const data = ctx.getImageData(0, 0, SAMPLE_W, SAMPLE_H).data;

      // Phase 1: compute per-pixel luminance
      for (let i = 0; i < n; i++) {
        const pi = i * 4;
        lumArr[i] = 0.2126 * data[pi] + 0.7152 * data[pi + 1] + 0.0722 * data[pi + 2];
      }

      // Phase 2: sort indices by luminance
      const sorted = idxArr;
      sorted.sort((a, b) => lumArr[a] - lumArr[b]);

      // Phase 3: trimmed weighted average (cut TRIM% from each end)
      const lo = Math.floor(n * TRIM);
      const hi = n - lo;
      let rW = 0, gW = 0, bW = 0, wSum = 0;
      for (let i = lo; i < hi; i++) {
        const idx = sorted[i];
        const pi = idx * 4;
        const w = weights[idx];
        rW += data[pi] * w;
        gW += data[pi + 1] * w;
        bW += data[pi + 2] * w;
        wSum += w;
      }

      const bgLum = luminance(rW / wSum, gW / wSum, bW / wSum);
      const color = bestTextColor(bgLum);

      if (color !== lastColorRef.current) {
        lastColorRef.current = color;
        cbRef.current(color);
      }
    };

    if (el.readyState >= 2) sample();
    el.addEventListener('loadeddata', sample);
    intervalRef.current = setInterval(sample, INTERVAL_MS);

    return () => {
      clearInterval(intervalRef.current);
      el.removeEventListener('loadeddata', sample);
    };
  }, [videoRef, bgVideo, videoPlaying]);
}
