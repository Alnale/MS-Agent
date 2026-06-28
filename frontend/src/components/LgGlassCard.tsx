import { useEffect, memo } from 'react';
import type { LgCategory } from '../App';

interface Props { config: LgCategory; }

export const LgGlassCard = memo(function LgGlassCard({ config }: Props) {
  const { enabled, blurAmount, saturation, overLight, cornerRadius } = config;

  useEffect(() => {
    if (!enabled) return;

    const blurPx = (overLight ? 12 : 4) + blurAmount * 32;
    const bg = overLight
      ? 'linear-gradient(180deg, rgba(255,255,255,0.22) 0%, rgba(255,255,255,0.10) 100%)'
      : 'linear-gradient(180deg, rgba(255,255,255,0.18) 0%, rgba(255,255,255,0.08) 100%)';
    const radius = `${cornerRadius}px`;
    const shadow = overLight
      ? '0 16px 70px rgba(0,0,0,0.75)'
      : '0 12px 40px rgba(0,0,0,0.25), 0 4px 16px rgba(0,0,0,0.1)';
    const border = overLight
      ? '1px solid rgba(255,255,255,0.15)'
      : '1px solid rgba(255,255,255,0.2)';
    const edgeShadow = '0 0 0 0.5px rgba(255,255,255,0.5) inset, 0 1px 3px rgba(255,255,255,0.25) inset, 0 1px 4px rgba(0,0,0,0.35)';

    const id = 'lg-glass-card-dynamic-style';
    let el = document.getElementById(id) as HTMLStyleElement | null;
    if (!el) {
      el = document.createElement('style');
      el.id = id;
      document.head.appendChild(el);
    }
    el.textContent = `
      [data-lg-card="true"] .welcome-glass-layer-1 {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        box-shadow: ${edgeShadow}, ${shadow} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
      }
      [data-lg-card="true"] .welcome-glass-layer-2 { display: none !important; }
      [data-lg-card="true"] .settings-panel {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        box-shadow: ${edgeShadow}, ${shadow} !important;
        border: ${border} !important;
      }
      [data-lg-card="true"] .settings-overlay {
        backdrop-filter: blur(${Math.max(4, blurPx * 0.4)}px) saturate(${Math.max(100, saturation * 0.8)}%) !important;
        -webkit-backdrop-filter: blur(${Math.max(4, blurPx * 0.4)}px) saturate(${Math.max(100, saturation * 0.8)}%) !important;
      }
      [data-lg-card="true"] .sidebar {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: 0 ${radius} ${radius} 0 !important;
        box-shadow: ${edgeShadow}, 4px 0 40px rgba(0,0,0,0.15) !important;
        border-right: ${border} !important;
      }
      [data-lg-card="true"] .sidebar-overlay {
        backdrop-filter: blur(${Math.max(2, blurPx * 0.15)}px) !important;
        -webkit-backdrop-filter: blur(${Math.max(2, blurPx * 0.15)}px) !important;
      }
      [data-lg-card="true"] .changelog-card-glass {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        box-shadow: ${edgeShadow} !important;
      }
      [data-lg-companion="true"] .companion-panel {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        box-shadow: ${edgeShadow}, 0 4px 20px rgba(0,0,0,0.12) !important;
        border: ${border} !important;
      }
      [data-lg-companion="true"] .companion-mood-intensity {
        background: rgba(255,255,255,0.12) !important;
      }
      [data-lg-companion="true"] .companion-bar-track {
        background: rgba(255,255,255,0.12) !important;
      }
      [data-lg-companion="true"] .companion-reason {
        background: rgba(255,255,255,0.08) !important;
      }
    `;
    return () => { el?.remove(); };
  }, [enabled, blurAmount, saturation, overLight, cornerRadius]);

  if (!enabled) return null;
  return null;
});
