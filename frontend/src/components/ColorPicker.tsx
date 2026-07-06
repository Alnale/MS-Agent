import { useState, useCallback, useRef, useEffect, memo } from 'react';

interface Props {
  label: string;
  color: string;
  alpha: number;
  presets?: string[];
  onColorChange: (color: string) => void;
  onAlphaChange: (alpha: number) => void;
}

function hexToRgb(hex: string): { r: number; g: number; b: number } | null {
  const m = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
  return m ? { r: parseInt(m[1], 16), g: parseInt(m[2], 16), b: parseInt(m[3], 16) } : null;
}

function rgbToHex(r: number, g: number, b: number): string {
  return '#' + [r, g, b].map(v => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0')).join('');
}

const DEFAULT_PRESETS = [
  '#ffffff', '#f8f4f0', '#fdf2f8', '#fce7f3',
  '#f3e8ff', '#ede9fe', '#e0e7ff', '#dbeafe',
  '#e0f2fe', '#ccfbf1', '#d1fae5', '#ecfccb',
  '#fef9c3', '#fef3c7', '#ffedd5', '#fee2e2',
  '#fecaca', '#1a1a2e', '#1e1b2e', '#2d1b3d',
];

export const ColorPicker = memo(function ColorPicker({ label, color, alpha, presets = DEFAULT_PRESETS, onColorChange, onAlphaChange }: Props) {
  const [hexInput, setHexInput] = useState(color);
  const [editing, setEditing] = useState(false);
  const nativeRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!editing) setHexInput(color);
  }, [color, editing]);

  const rgb = hexToRgb(color) || { r: 255, g: 255, b: 255 };

  const handleRgbChange = useCallback((channel: 'r' | 'g' | 'b', value: number) => {
    const newRgb = { ...rgb, [channel]: value };
    onColorChange(rgbToHex(newRgb.r, newRgb.g, newRgb.b));
  }, [rgb, onColorChange]);

  const handleHexCommit = useCallback(() => {
    setEditing(false);
    const cleaned = hexInput.trim();
    if (/^#?[a-f\d]{6}$/i.test(cleaned)) {
      onColorChange(cleaned.startsWith('#') ? cleaned : '#' + cleaned);
    } else {
      setHexInput(color);
    }
  }, [hexInput, color, onColorChange]);

  return (
    <div className="cp">
      <div className="cp-header">
        <span className="cp-label">{label}</span>
        <div className="cp-preview-wrap">
          <div
            className="cp-preview"
            style={{
              background: `rgba(${rgb.r},${rgb.g},${rgb.b},${alpha})`,
            }}
          />
          <div className="cp-checkerboard" />
        </div>
      </div>

      {/* Native color + hex input */}
      <div className="cp-row">
        <button className="cp-native-btn" onClick={() => nativeRef.current?.click()}>
          <div className="cp-native-swatch" style={{ background: color }} />
          <input
            ref={nativeRef}
            type="color"
            value={color}
            onChange={(e) => onColorChange(e.target.value)}
            className="cp-native-input"
          />
        </button>
        <input
          className="cp-hex-input"
          value={editing ? hexInput : color}
          onChange={(e) => { setEditing(true); setHexInput(e.target.value); }}
          onBlur={handleHexCommit}
          onKeyDown={(e) => { if (e.key === 'Enter') handleHexCommit(); }}
          maxLength={7}
          spellCheck={false}
        />
      </div>

      {/* RGB sliders */}
      <div className="cp-rgb">
        {(['r', 'g', 'b'] as const).map(ch => (
          <div className="cp-rgb-row" key={ch}>
            <span className={`cp-rgb-label cp-rgb-${ch}`}>{ch.toUpperCase()}</span>
            <input
              type="range"
              className="cp-rgb-slider"
              min={0} max={255} step={1}
              value={rgb[ch]}
              onChange={(e) => handleRgbChange(ch, +e.target.value)}
            />
            <span className="cp-rgb-value">{rgb[ch]}</span>
          </div>
        ))}
      </div>

      {/* Alpha slider */}
      <div className="cp-alpha-row">
        <span className="cp-alpha-label">透明度</span>
        <input
          type="range"
          className="cp-alpha-slider"
          min={0} max={1} step={0.01}
          value={alpha}
          onChange={(e) => onAlphaChange(+e.target.value)}
        />
        <span className="cp-alpha-value">{Math.round(alpha * 100)}%</span>
      </div>

      {/* Presets */}
      <div className="cp-presets">
        {presets.map(p => (
          <button
            key={p}
            className={`cp-preset${color === p ? ' active' : ''}`}
            style={{ background: p }}
            onClick={() => onColorChange(p)}
            title={p}
          />
        ))}
      </div>
    </div>
  );
});
