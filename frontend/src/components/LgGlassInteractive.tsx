import { useRef, useCallback, useEffect, type ReactNode, type MouseEvent } from 'react';

interface Props {
  children: ReactNode;
  className?: string;
  elasticity?: number;
  activationZone?: number;
}

// Shared mousemove listener registry — one global listener for all instances
type Subscriber = (mouseX: number, mouseY: number) => void;
const subscribers = new Set<Subscriber>();
let globalListenerAttached = false;
let lastMouseX = 0;
let lastMouseY = 0;

function ensureGlobalListener() {
  if (globalListenerAttached) return;
  globalListenerAttached = true;
  const onMove = (e: globalThis.MouseEvent) => {
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
    for (const sub of subscribers) {
      sub(e.clientX, e.clientY);
    }
  };
  document.addEventListener('mousemove', onMove, { passive: true });
}

function subscribe(cb: Subscriber): () => void {
  ensureGlobalListener();
  subscribers.add(cb);
  return () => { subscribers.delete(cb); };
}

/**
 * Liquid glass mouse interactivity wrapper.
 * Adds elastic translation, directional scaling, and hover/active overlays
 * matching the reference liquid-glass-react-master behavior.
 */
export function LgGlassInteractive({
  children,
  className = '',
  elasticity = 0.25,
  activationZone = 200,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const rafRef = useRef(0);

  const applyEffect = useCallback((mouseX: number, mouseY: number) => {
    const el = ref.current;
    if (!el) return;

    const rect = el.getBoundingClientRect();
    const centerX = rect.left + rect.width / 2;
    const centerY = rect.top + rect.height / 2;
    const halfW = rect.width / 2;
    const halfH = rect.height / 2;

    const edgeDistX = Math.max(0, Math.abs(mouseX - centerX) - halfW);
    const edgeDistY = Math.max(0, Math.abs(mouseY - centerY) - halfH);
    const edgeDist = Math.sqrt(edgeDistX * edgeDistX + edgeDistY * edgeDistY);

    if (edgeDist > activationZone) {
      el.style.transform = '';
      return;
    }

    const fadeIn = 1 - edgeDist / activationZone;
    const dx = mouseX - centerX;
    const dy = mouseY - centerY;
    const dist = Math.sqrt(dx * dx + dy * dy);
    if (dist === 0) return;

    const nx = dx / dist;
    const ny = dy / dist;
    const intensity = Math.min(dist / 300, 1) * elasticity * fadeIn;
    const scaleX = Math.max(0.75, 1 + Math.abs(nx) * intensity * 0.45 - Math.abs(ny) * intensity * 0.2);
    const scaleY = Math.max(0.75, 1 + Math.abs(ny) * intensity * 0.45 - Math.abs(nx) * intensity * 0.2);
    const tx = dx * elasticity * 0.15 * fadeIn;
    const ty = dy * elasticity * 0.15 * fadeIn;

    el.style.transform = `translate(${tx}px, ${ty}px) scaleX(${scaleX}) scaleY(${scaleY})`;
  }, [elasticity, activationZone]);

  useEffect(() => {
    const unsub = subscribe((mx, my) => {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = requestAnimationFrame(() => applyEffect(mx, my));
    });
    return () => {
      unsub();
      cancelAnimationFrame(rafRef.current);
    };
  }, [applyEffect]);

  const handleMouseEnter = useCallback((e: MouseEvent) => {
    (e.currentTarget as HTMLElement).style.transition = 'transform 0.2s ease-out';
  }, []);

  const handleMouseLeave = useCallback(() => {
    const el = ref.current;
    if (el) {
      el.style.transition = 'transform 0.4s cubic-bezier(0, 0, 0.6, 1)';
      el.style.transform = '';
    }
  }, []);


  return (
    <div
      ref={ref}
      className={`lg-interactive ${className}`}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        willChange: 'transform',
        position: 'relative',
      }}
    >
      {children}
    </div>
  );
}
