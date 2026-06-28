import { useEffect, useRef, useState } from 'react';

const FFT_SIZE = 256;
const SMOOTHING = 0.8;

export function useAudioAnalyser(
  videoRef: React.RefObject<HTMLVideoElement | null>,
  bgVideo: string | null,
  videoPlaying: boolean,
  videoMuted: boolean,
) {
  const frequencyDataRef = useRef<Uint8Array | null>(null);
  const [, setRenderTick] = useState(0);

  const ctxRef = useRef<AudioContext | null>(null);
  const sourceRef = useRef<MediaElementAudioSourceNode | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const gainRef = useRef<GainNode | null>(null);
  const connectedElRef = useRef<HTMLVideoElement | null>(null);
  const rafRef = useRef<number>(0);
  const lastRenderRef = useRef<number>(0);

  // Sync gain with mute state
  useEffect(() => {
    if (gainRef.current) {
      gainRef.current.gain.value = videoMuted ? 0 : 1;
    }
  }, [videoMuted]);

  useEffect(() => {
    const el = videoRef.current;
    if (!el || !bgVideo || !videoPlaying) {
      cancelAnimationFrame(rafRef.current);
      frequencyDataRef.current = null;
      setRenderTick(t => t + 1);
      return;
    }

    // Lazily create AudioContext
    if (!ctxRef.current) {
      ctxRef.current = new AudioContext();
    }
    const ctx = ctxRef.current;
    if (ctx.state === 'suspended') ctx.resume();

    // createMediaElementSource can only be called once per element
    if (connectedElRef.current !== el) {
      try { sourceRef.current?.disconnect(); } catch {}
      sourceRef.current = ctx.createMediaElementSource(el);
      connectedElRef.current = el;
    }
    const source = sourceRef.current;
    if (!source) return;

    if (!analyserRef.current) {
      analyserRef.current = ctx.createAnalyser();
      analyserRef.current.fftSize = FFT_SIZE;
      analyserRef.current.smoothingTimeConstant = SMOOTHING;
    }
    const analyser = analyserRef.current;

    if (!gainRef.current) {
      gainRef.current = ctx.createGain();
    }
    const gain = gainRef.current;
    gain.gain.value = videoMuted ? 0 : 1;

    // Wire: source → analyser → gain → destination
    try { source.disconnect(); } catch {}
    source.connect(analyser);
    analyser.connect(gain);
    gain.connect(ctx.destination);

    const buf = new Uint8Array(analyser.frequencyBinCount);

    const tick = () => {
      analyser.getByteFrequencyData(buf);
      frequencyDataRef.current = buf;
      const now = performance.now();
      if (now - lastRenderRef.current > 33) { // ~30fps
        lastRenderRef.current = now;
        setRenderTick(t => t + 1);
      }
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);

    return () => {
      cancelAnimationFrame(rafRef.current);
      try {
        source.disconnect();
        analyser.disconnect();
        gain.disconnect();
      } catch {}
    };
  }, [videoRef, bgVideo, videoPlaying]);

  return frequencyDataRef.current;
}
