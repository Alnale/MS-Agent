import { useEffect, memo } from 'react';
import type { LgCategory } from '../App';

interface Props { config: LgCategory; }

export const LgGlassButton = memo(function LgGlassButton({ config }: Props) {
  const { enabled, blurAmount, saturation, overLight, cornerRadius } = config;

  useEffect(() => {
    if (!enabled) return;

    const blurPx = (overLight ? 12 : 4) + blurAmount * 32;
    const bg = overLight
      ? 'linear-gradient(135deg, rgba(255,255,255,0.25) 0%, rgba(255,255,255,0.12) 100%)'
      : 'linear-gradient(135deg, rgba(255,255,255,0.18) 0%, rgba(255,255,255,0.08) 100%)';
    const radius = `${cornerRadius}px`;
    const border = overLight
      ? '1.5px solid rgba(255,255,255,0.2)'
      : '1.5px solid rgba(255,255,255,0.25)';
    const edgeShadow = '0 0 0 0.5px rgba(255,255,255,0.5) inset, 0 1px 3px rgba(255,255,255,0.25) inset';

    const id = 'lg-glass-button-dynamic-style';
    let el = document.getElementById(id) as HTMLStyleElement | null;
    if (!el) {
      el = document.createElement('style');
      el.id = id;
      document.head.appendChild(el);
    }
    el.textContent = `
      /* Status bar buttons — glass visible */
      [data-lg-button="true"] .btn-clear {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${edgeShadow} !important;
        transition: backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
                    -webkit-backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
                    background 0.3s cubic-bezier(0, 0, 0.6, 1),
                    border-color 0.3s cubic-bezier(0, 0, 0.6, 1),
                    box-shadow 0.3s cubic-bezier(0, 0, 0.6, 1) !important;
      }
      [data-lg-button="true"] .btn-icon {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        transition: backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
                    -webkit-backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
                    background 0.3s cubic-bezier(0, 0, 0.6, 1) !important;
      }

      /* Status bar focused — dissolve glass */
      [data-lg-button="true"] .status-bar.focused .btn-clear {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        border-color: transparent !important;
        box-shadow: none !important;
        transition: backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
                    -webkit-backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
                    background 0.25s cubic-bezier(0.4, 0, 1, 1),
                    border-color 0.25s cubic-bezier(0.4, 0, 1, 1),
                    box-shadow 0.3s cubic-bezier(0.4, 0, 1, 1) !important;
      }
      [data-lg-button="true"] .status-bar.focused .btn-icon {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        transition: backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
                    -webkit-backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
                    background 0.25s cubic-bezier(0.4, 0, 1, 1) !important;
      }

      /* Welcome screen suggestion buttons */
      [data-lg-button="true"] .suggestion-btn {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${edgeShadow} !important;
      }

      /* Tool selector trigger */
      [data-lg-button="true"] .tool-selector-trigger {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${edgeShadow} !important;
      }

      /* Send button */
      [data-lg-button="true"] .send-btn {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${edgeShadow} !important;
      }

      /* Sidebar toggle — glass visible */
      [data-lg-button="true"] .sidebar-toggle {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        transition: backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
                    -webkit-backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
                    background 0.3s cubic-bezier(0, 0, 0.6, 1) !important;
      }

      /* Sidebar toggle focused — dissolve glass */
      [data-lg-button="true"] .status-bar.focused .sidebar-toggle {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        transition: backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
                    -webkit-backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
                    background 0.25s cubic-bezier(0.4, 0, 1, 1) !important;
      }

      /* Music entry button — glass visible */
      [data-lg-button="true"] .music-entry-btn {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        transition: backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
                    -webkit-backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
                    background 0.3s cubic-bezier(0, 0, 0.6, 1) !important;
      }

      /* Music entry button focused — dissolve glass */
      [data-lg-button="true"] .status-bar.focused .music-entry-btn {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        transition: backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
                    -webkit-backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
                    background 0.25s cubic-bezier(0.4, 0, 1, 1) !important;
      }
    `;
    return () => { el?.remove(); };
  }, [enabled, blurAmount, saturation, overLight, cornerRadius]);

  if (!enabled) return null;
  return null;
});
