import { useEffect, useRef, useState } from 'react';

export function SplashScreen({ onComplete }: { onComplete: () => void }) {
  const [phase, setPhase] = useState<'enter' | 'exit'>('enter');
  const [ready, setReady] = useState(false);
  const [strokeLen, setStrokeLen] = useState(0);
  const textRef = useRef<SVGTextElement>(null);

  // Wait for Caveat font to load before measuring text
  useEffect(() => {
    document.fonts.ready.then(() => {
      if (textRef.current) {
        const len = textRef.current.getComputedTextLength();
        setStrokeLen(len);
      }
      setReady(true);
    });
  }, []);

  useEffect(() => {
    const showTimer = setTimeout(() => setPhase('exit'), 1500);
    const hideTimer = setTimeout(() => onComplete(), 2000);
    return () => { clearTimeout(showTimer); clearTimeout(hideTimer); };
  }, [onComplete]);

  return (
    <div className={`splash-screen ${phase === 'exit' ? 'splash-exit' : ''}`}>
      {/* Subtle background blobs — drift gently */}
      <div className="splash-blob splash-blob-1" />
      <div className="splash-blob splash-blob-2" />
      <div className="splash-blob splash-blob-3" />

      {/* "Hello" drawn by stroke animation */}
      <svg className="splash-hello-svg" viewBox="0 0 520 140">
        {/* Stroke layer — the "pen" writing effect */}
        <text
          ref={textRef}
          className="splash-hello-text"
          x="260"
          y="105"
          textAnchor="middle"
          fill="none"
          stroke="var(--text-primary)"
          strokeWidth="2.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          style={ready && strokeLen > 0 ? {
            strokeDasharray: strokeLen,
            strokeDashoffset: strokeLen,
            animation: 'splash-stroke-draw 1s cubic-bezier(0.22, 1, 0.36, 1) forwards',
          } : { opacity: 0 }}
        >
          Hello
        </text>
        {/* Fill layer — fades in after stroke finishes (or immediately if font didn't load) */}
        <text
          className="splash-hello-text"
          x="260"
          y="105"
          textAnchor="middle"
          fill="var(--text-primary)"
          stroke="none"
          style={ready ? {
            opacity: 0,
            animation: strokeLen > 0
              ? 'splash-fill-in 0.4s cubic-bezier(0.22, 1, 0.36, 1) 1s forwards'
              : 'splash-fill-in 0.6s cubic-bezier(0.22, 1, 0.36, 1) 0.2s forwards',
          } : { opacity: 0 }}
        >
          Hello
        </text>
      </svg>

      {/* Sparkle decorations */}
      <div className="splash-sparkle splash-sparkle-1" />
      <div className="splash-sparkle splash-sparkle-2" />
      <div className="splash-sparkle splash-sparkle-3" />
      <div className="splash-sparkle splash-sparkle-4" />
      <div className="splash-sparkle splash-sparkle-5" />
    </div>
  );
}
