import { useEffect, useState, memo } from 'react';

interface Props {
  onComplete: () => void;
  onMidpoint: () => void;
}

export const BgTransition = memo(function BgTransition({ onComplete, onMidpoint }: Props) {
  const [phase, setPhase] = useState<'enter' | 'exit'>('enter');

  useEffect(() => {
    const midTimer = setTimeout(() => {
      onMidpoint();
      setPhase('exit');
    }, 400);
    const endTimer = setTimeout(() => onComplete(), 800);
    return () => { clearTimeout(midTimer); clearTimeout(endTimer); };
  }, [onComplete, onMidpoint]);

  return (
    <div className={`bg-transition ${phase === 'exit' ? 'bg-transition-exit' : ''}`}>
      {/* Color wash sweep */}
      <div className="bg-trans-wash bg-trans-wash-1" />
      <div className="bg-trans-wash bg-trans-wash-2" />

      {/* Expanding rings from center */}
      <div className="bg-trans-ring bg-trans-ring-1" />
      <div className="bg-trans-ring bg-trans-ring-2" />
      <div className="bg-trans-ring bg-trans-ring-3" />

      {/* Paint drip marks */}
      <svg className="bg-trans-drips" viewBox="0 0 400 400" fill="none">
        <circle className="bg-trans-drip bg-trans-drip-1" cx="80" cy="60" r="12" />
        <circle className="bg-trans-drip bg-trans-drip-2" cx="320" cy="90" r="8" />
        <circle className="bg-trans-drip bg-trans-drip-3" cx="60" cy="320" r="10" />
        <circle className="bg-trans-drip bg-trans-drip-4" cx="340" cy="340" r="7" />
        <ellipse className="bg-trans-drip bg-trans-drip-5" cx="200" cy="50" rx="15" ry="6" />
        <ellipse className="bg-trans-drip bg-trans-drip-6" cx="50" cy="200" rx="6" ry="14" />
      </svg>

      {/* Floating micro particles */}
      <div className="bg-trans-particle bg-trans-particle-1" />
      <div className="bg-trans-particle bg-trans-particle-2" />
      <div className="bg-trans-particle bg-trans-particle-3" />
      <div className="bg-trans-particle bg-trans-particle-4" />
      <div className="bg-trans-particle bg-trans-particle-5" />
      <div className="bg-trans-particle bg-trans-particle-6" />
    </div>
  );
});
