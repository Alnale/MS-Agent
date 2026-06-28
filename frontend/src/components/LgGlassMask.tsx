import { useEffect, memo } from 'react';
import type { LgCategory } from '../App';

interface Props { config: LgCategory; className?: string; active?: boolean; }

export const LgGlassMask = memo(function LgGlassMask({ config, className = '', active = true }: Props) {
  const { enabled, blurAmount, saturation, overLight } = config;

  // Inject dynamic CSS for backdrop-filter on status bar / input bar
  useEffect(() => {
    if (!enabled) return;

    const blurPx = (overLight ? 12 : 4) + blurAmount * 32;
    const bg = overLight
      ? 'linear-gradient(180deg, rgba(255,255,255,0.18) 0%, rgba(255,255,255,0.08) 100%)'
      : 'linear-gradient(180deg, rgba(255,255,255,0.15) 0%, rgba(255,255,255,0.08) 100%)';

    const id = 'lg-glass-dynamic-style';
    let el = document.getElementById(id) as HTMLStyleElement | null;
    if (!el) {
      el = document.createElement('style');
      el.id = id;
      document.head.appendChild(el);
    }
    el.textContent = `
      /* ── Status bar: glass visible (reappear transition) ── */
      [data-lg-mask="true"] .status-bar {
        background: ${bg} !important;
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        border-bottom-color: transparent !important;
        box-shadow: inset 0 -1px 0 rgba(255,255,255,0.25), inset 0 1px 0 rgba(255,255,255,0.15) !important;
        animation: none !important;
        transition: backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1) 0.08s,
                    -webkit-backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1) 0.08s,
                    background 0.3s cubic-bezier(0, 0, 0.6, 1) 0.07s,
                    border-color 0.25s cubic-bezier(0, 0, 0.6, 1) 0.04s,
                    box-shadow 0.3s cubic-bezier(0, 0, 0.6, 1) 0.06s !important;
      }
      [data-lg-mask="true"] .status-bar::after { opacity: 0 !important; animation: none !important; }

      /* ── Status bar: focused (dissolve transition) ── */
      [data-lg-mask="true"] .status-bar.focused {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        border-color: transparent !important;
        box-shadow: none !important;
        transition: backdrop-filter 0.35s cubic-bezier(0.4, 0, 1, 1),
                    -webkit-backdrop-filter 0.35s cubic-bezier(0.4, 0, 1, 1),
                    background 0.3s cubic-bezier(0.4, 0, 1, 1) 0.05s,
                    border-color 0.25s cubic-bezier(0.4, 0, 1, 1) 0.08s,
                    box-shadow 0.3s cubic-bezier(0.4, 0, 1, 1) 0.06s !important;
      }

      /* ── Input bar: glass visible (reappear transition) ── */
      [data-lg-mask="true"] .input-bar-container::before {
        background: ${bg} !important;
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        box-shadow: inset 0 1px 0 rgba(255,255,255,0.25) !important;
        animation: none !important;
        transition: backdrop-filter 0.6s cubic-bezier(0, 0, 0.6, 1) 0.2s,
                    -webkit-backdrop-filter 0.6s cubic-bezier(0, 0, 0.6, 1) 0.2s,
                    background 0.5s cubic-bezier(0, 0, 0.6, 1) 0.28s,
                    box-shadow 0.45s cubic-bezier(0, 0, 0.6, 1) 0.36s !important;
      }
      [data-lg-mask="true"] .input-bar-container {
        border-top-color: transparent !important;
        transition: border-color 0.5s cubic-bezier(0, 0, 0.6, 1) 0.2s !important;
      }

      /* ── Input bar: focused (dissolve transition) ── */
      [data-lg-mask="true"] .input-bar-container:has(.input-bar.focused)::before {
        background: transparent !important;
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        box-shadow: none !important;
        transition: backdrop-filter 0.6s cubic-bezier(0.25, 0.1, 0.25, 1),
                    -webkit-backdrop-filter 0.6s cubic-bezier(0.25, 0.1, 0.25, 1),
                    background 0.5s cubic-bezier(0.25, 0.1, 0.25, 1) 0.08s,
                    box-shadow 0.45s cubic-bezier(0.25, 0.1, 0.25, 1) 0.12s !important;
      }
      [data-lg-mask="true"] .input-bar-container:has(.input-bar.focused) {
        border-color: transparent !important;
        transition: border-color 0.5s cubic-bezier(0.25, 0.1, 0.25, 1) 0.06s !important;
      }

      /* ── Custom-bg overrides (unfocused) ── */
      [data-lg-mask="true"].has-custom-bg .status-bar {
        background: ${bg} !important;
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
      }
      [data-lg-mask="true"].has-custom-bg .input-bar-container::before {
        background: ${bg} !important;
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
      }

      /* ── Custom-bg focused overrides (must come after unfocused to win cascade) ── */
      [data-lg-mask="true"].has-custom-bg .status-bar.focused {
        background: transparent !important;
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
      }
      [data-lg-mask="true"].has-custom-bg .input-bar-container:has(.input-bar.focused)::before {
        background: transparent !important;
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
      }
    `;
    return () => { el?.remove(); };
  }, [enabled, blurAmount, saturation, overLight]);

  if (!enabled) return null;

  return (
    <div
      className={`lg-glass-mask ${className}`}
      style={{
        position: 'absolute',
        inset: 0,
        pointerEvents: 'none',
        zIndex: 2,
        overflow: 'hidden',
        background: 'transparent',
        opacity: active ? 1 : 0,
        transition: 'opacity 0.35s cubic-bezier(0, 0, 0.6, 1)',
      }}
    >

      {/* Border layer 1 — screen blend, edge highlight */}
      <span
        style={{
          position: 'absolute',
          inset: 0,
          pointerEvents: 'none',
          mixBlendMode: 'screen',
          opacity: 0.2,
          padding: '1.5px',
          WebkitMask: 'linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0)',
          WebkitMaskComposite: 'xor',
          maskComposite: 'exclude',
          boxShadow: '0 0 0 0.5px rgba(255,255,255,0.5) inset, 0 1px 3px rgba(255,255,255,0.25) inset, 0 1px 4px rgba(0,0,0,0.35)',
          background: 'linear-gradient(135deg, rgba(255,255,255,0.0) 0%, rgba(255,255,255,0.12) 33%, rgba(255,255,255,0.4) 66%, rgba(255,255,255,0.0) 100%)',
        }}
      />

      {/* Border layer 2 — overlay blend, deeper highlight */}
      <span
        style={{
          position: 'absolute',
          inset: 0,
          pointerEvents: 'none',
          mixBlendMode: 'overlay',
          padding: '1.5px',
          WebkitMask: 'linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0)',
          WebkitMaskComposite: 'xor',
          maskComposite: 'exclude',
          boxShadow: '0 0 0 0.5px rgba(255,255,255,0.5) inset, 0 1px 3px rgba(255,255,255,0.25) inset, 0 1px 4px rgba(0,0,0,0.35)',
          background: 'linear-gradient(135deg, rgba(255,255,255,0.0) 0%, rgba(255,255,255,0.32) 33%, rgba(255,255,255,0.6) 66%, rgba(255,255,255,0.0) 100%)',
        }}
      />
    </div>
  );
});
