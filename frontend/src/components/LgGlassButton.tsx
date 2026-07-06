import { useEffect, memo } from 'react';
import type { LgCategory } from '../App';

interface Props { config: LgCategory; }

export const LgGlassButton = memo(function LgGlassButton({ config }: Props) {
  const { enabled, blurAmount, saturation, overLight, cornerRadius } = config;

  useEffect(() => {
    if (!enabled) return;

    const blurPx = (overLight ? 12 : 4) + blurAmount * 32;
    const radius = `${cornerRadius}px`;

    // ── Liquid glass background: base frosted + diagonal highlight shine ──
    // Inspired by pure-CSS liquid glass technique: multi-layer gradient
    // with diagonal white highlights simulating light refraction on glass edges
    const bg = overLight
      ? `linear-gradient(135deg, rgba(255,255,255,0.1) 0%, rgba(255,255,255,0.3) 20%, rgba(255,255,255,0.15) 40%, rgba(255,255,255,0.08) 60%, rgba(255,255,255,0.25) 80%, rgba(255,255,255,0.1) 100%)`
      : `linear-gradient(135deg, rgba(255,255,255,0.06) 0%, rgba(255,255,255,0.22) 20%, rgba(255,255,255,0.1) 40%, rgba(255,255,255,0.05) 60%, rgba(255,255,255,0.18) 80%, rgba(255,255,255,0.06) 100%)`;
    const border = overLight
      ? '1.5px double rgba(51, 51, 51, 0.08)'
      : '1.5px double rgba(255,255,255,0.12)';

    // ── Multi-layered inset box-shadow: edge highlights + refraction depth ──
    // Layer 1-2: crisp top/left white edge (simulates glass rim catching light)
    // Layer 3-4: softer secondary edge glow
    // Layer 5: inner ambient shadow for depth
    // Layer 6: outer drop shadow for elevation
    const glassShadow = [
      'inset 2px -2px 1px -1px rgba(255,255,255,0.9)',
      'inset -2px 2px 1px -1px rgba(255,255,255,0.9)',
      'inset 5px -5px 1px -5px rgba(255,255,255,0.5)',
      'inset -5px 5px 1px -5px rgba(255,255,255,0.5)',
      'inset 0 0 3px rgba(0,0,0,0.15)',
      '0 4px 12px rgba(0,0,0,0.15)',
    ].join(', ');

    const glassFilter = `brightness(${overLight ? 0.95 : 0.92})`;

    // ── Transition tokens ──
    const tReappear = `
      backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
      -webkit-backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1),
      background 0.3s cubic-bezier(0, 0, 0.6, 1),
      border-color 0.3s cubic-bezier(0, 0, 0.6, 1),
      box-shadow 0.3s cubic-bezier(0, 0, 0.6, 1),
      filter 0.3s cubic-bezier(0, 0, 0.6, 1)`;
    const tDissolve = `
      backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
      -webkit-backdrop-filter 0.3s cubic-bezier(0.4, 0, 1, 1),
      background 0.25s cubic-bezier(0.4, 0, 1, 1),
      border-color 0.25s cubic-bezier(0.4, 0, 1, 1),
      box-shadow 0.3s cubic-bezier(0.4, 0, 1, 1),
      filter 0.3s cubic-bezier(0.4, 0, 1, 1)`;

    const id = 'lg-glass-button-dynamic-style';
    let el = document.getElementById(id) as HTMLStyleElement | null;
    if (!el) {
      el = document.createElement('style');
      el.id = id;
      document.head.appendChild(el);
    }
    el.textContent = `
      /* ═══════════════════════════════════════════
         Liquid Glass Buttons — pure CSS technique
         Multi-layer shadows + diagonal shine + refraction
         ═══════════════════════════════════════════ */

      /* Status bar buttons — glass visible */
      [data-lg-button="true"] .btn-clear {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${glassShadow} !important;
        filter: ${glassFilter} !important;
        transition: ${tReappear} !important;
      }
      [data-lg-button="true"] .btn-clear:hover {
        filter: brightness(1) !important;
        box-shadow:
          inset 2px -2px 1px -1px rgba(255,255,255,0.95),
          inset -2px 2px 1px -1px rgba(255,255,255,0.95),
          inset 5px -5px 1px -5px rgba(255,255,255,0.6),
          inset -5px 5px 1px -5px rgba(255,255,255,0.6),
          inset 0 0 3px rgba(0,0,0,0.1),
          0 6px 20px rgba(0,0,0,0.18) !important;
      }
      [data-lg-button="true"] .btn-icon {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        box-shadow: ${glassShadow} !important;
        filter: ${glassFilter} !important;
        transition: ${tReappear} !important;
      }

      /* Status bar focused — dissolve glass */
      [data-lg-button="true"] .status-bar.focused .btn-clear {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        border-color: transparent !important;
        box-shadow: none !important;
        filter: none !important;
        transition: ${tDissolve} !important;
      }
      [data-lg-button="true"] .status-bar.focused .btn-icon {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        box-shadow: none !important;
        filter: none !important;
        transition: ${tDissolve} !important;
      }

      /* Welcome screen suggestion buttons */
      [data-lg-button="true"] .suggestion-btn {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${glassShadow} !important;
        filter: ${glassFilter} !important;
      }
      [data-lg-button="true"] .suggestion-btn:hover {
        filter: brightness(1) !important;
      }

      /* Tool selector trigger */
      [data-lg-button="true"] .tool-selector-trigger {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${glassShadow} !important;
        filter: ${glassFilter} !important;
      }
      [data-lg-button="true"] .tool-selector-trigger:hover {
        filter: brightness(1) !important;
      }

      /* Send button */
      [data-lg-button="true"] .send-btn {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${glassShadow} !important;
        filter: ${glassFilter} !important;
      }
      [data-lg-button="true"] .send-btn:hover:not(:disabled) {
        filter: brightness(1) !important;
        box-shadow:
          inset 2px -2px 1px -1px rgba(255,255,255,0.95),
          inset -2px 2px 1px -1px rgba(255,255,255,0.95),
          inset 5px -5px 1px -5px rgba(255,255,255,0.6),
          inset -5px 5px 1px -5px rgba(255,255,255,0.6),
          inset 0 0 3px rgba(0,0,0,0.1),
          0 6px 20px rgba(0,0,0,0.18) !important;
      }

      /* Sidebar toggle — glass visible */
      [data-lg-button="true"] .sidebar-toggle {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${glassShadow} !important;
        filter: ${glassFilter} !important;
        transition: ${tReappear} !important;
      }
      [data-lg-button="true"] .sidebar-toggle:hover {
        filter: brightness(1) !important;
      }

      /* Sidebar toggle focused — dissolve glass */
      [data-lg-button="true"] .status-bar.focused .sidebar-toggle {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        border-color: transparent !important;
        box-shadow: none !important;
        filter: none !important;
        transition: ${tDissolve} !important;
      }

      /* Music entry button — glass visible */
      [data-lg-button="true"] .music-entry-btn {
        backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        -webkit-backdrop-filter: blur(${blurPx}px) saturate(${saturation}%) !important;
        background: ${bg} !important;
        border-radius: ${radius} !important;
        border: ${border} !important;
        box-shadow: ${glassShadow} !important;
        filter: ${glassFilter} !important;
        transition: ${tReappear} !important;
      }
      [data-lg-button="true"] .music-entry-btn:hover {
        filter: brightness(1) !important;
      }

      /* Music entry button focused — dissolve glass */
      [data-lg-button="true"] .status-bar.focused .music-entry-btn {
        backdrop-filter: none !important;
        -webkit-backdrop-filter: none !important;
        background: transparent !important;
        border-color: transparent !important;
        box-shadow: none !important;
        filter: none !important;
        transition: ${tDissolve} !important;
      }
    `;
    // Don't remove the style element on cleanup — just leave the last values.
    // Removing and re-creating causes a flash of unstyled content on config changes.
  }, [enabled, blurAmount, saturation, overLight, cornerRadius]);

  if (!enabled) return null;
  return null;
});
