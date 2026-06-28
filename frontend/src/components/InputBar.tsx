import { useState, useRef, useEffect, useCallback, memo } from 'react';
import { ToolSelector } from './ToolSelector';
import { LgGlassInteractive } from './LgGlassInteractive';
import { BASE_URL } from '../config';
import type { ToolStatusEvent } from '../api/types';

interface Props {
  onSend: (message: string) => void;
  onStop?: () => void;
  disabled: boolean;
  focused: boolean;
  onFocusChange: (focused: boolean) => void;
  frequencyData?: Uint8Array | null;
  toolEvents?: ToolStatusEvent[];
}

const CANVAS_W = 320;
const BAR_COUNT = 64;
const BAR_GAP = CANVAS_W / BAR_COUNT;
const MIN_H = 2;
const MAX_H = 20;
// Prismatic palette — matches status-bar rainbow line
const PRISM = [
  [242, 128, 160], // pink
  [184, 169, 232], // lavender
  [141, 216, 176], // mint
  [245, 200, 160], // peach
];

function lerpColor(a: number[], b: number[], t: number): string {
  const r = Math.round(a[0] + (b[0] - a[0]) * t);
  const g = Math.round(a[1] + (b[1] - a[1]) * t);
  const b2 = Math.round(a[2] + (b[2] - a[2]) * t);
  return `rgb(${r},${g},${b2})`;
}

function prismAt(t: number): string {
  const scaled = t * (PRISM.length - 1);
  const i = Math.min(Math.floor(scaled), PRISM.length - 2);
  return lerpColor(PRISM[i], PRISM[i + 1], scaled - i);
}

export const InputBar = memo(function InputBar({ onSend, onStop, disabled, focused, onFocusChange, frequencyData, toolEvents }: Props) {
  const [value, setValue] = useState('');
  const [clipboardText, setClipboardText] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef<number>(0);
  const lastEnterRef = useRef(0);
  const lastClickRef = useRef(0);
  const [sendPulse, setSendPulse] = useState(false);

  useEffect(() => {
    if (!disabled && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [disabled]);

  // Track textarea focus/blur via DOM events (catches programmatic focus too)
  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    const onFocus = () => {
      onFocusChange(true);
      if (navigator.clipboard?.readText) {
        navigator.clipboard.readText().then((text) => {
          if (text) setClipboardText(text);
        }).catch(() => {});
      }
    };
    const onBlur = () => onFocusChange(false);
    el.addEventListener('focusin', onFocus);
    el.addEventListener('focusout', onBlur);
    return () => {
      el.removeEventListener('focusin', onFocus);
      el.removeEventListener('focusout', onBlur);
    };
  }, [onFocusChange]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = '48px';
    el.style.height = Math.min(el.scrollHeight, 160) + 'px';
  }, [value]);

  // Detect clipboard content via paste events
  useEffect(() => {
    const handlePaste = (e: ClipboardEvent) => {
      const text = e.clipboardData?.getData('text/plain');
      if (text) setClipboardText(text);
    };
    document.addEventListener('paste', handlePaste);
    return () => document.removeEventListener('paste', handlePaste);
  }, []);

  const idlePhaseRef = useRef(0);
  const barsRef = useRef<Float32Array>(new Float32Array(BAR_COUNT));
  const frequencyDataRef = useRef(frequencyData);
  frequencyDataRef.current = frequencyData;

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const w = canvas.width;
    const h = canvas.height;
    ctx.clearRect(0, 0, w, h);

    const freqData = frequencyDataRef.current;
    const hasAudio = !!freqData;
    const bars = barsRef.current;
    const target = new Float32Array(BAR_COUNT);

    idlePhaseRef.current += 0.02;

    for (let i = 0; i < BAR_COUNT; i++) {
      if (hasAudio) {
        const bin = Math.floor((i / BAR_COUNT) * (freqData!.length * 0.7));
        target[i] = MIN_H + (freqData![bin] / 255) * (MAX_H - MIN_H);
      } else {
        const p = idlePhaseRef.current;
        // Slow breathing pulse
        const breathe = 0.6 + 0.4 * Math.sin(p * 0.4);
        // Three layered waves at different speeds and frequencies
        const w1 = Math.sin(p * 1.0 + i * 0.12) * 0.30;
        const w2 = Math.sin(p * 0.6 + i * 0.25) * 0.18;
        const w3 = Math.sin(p * 1.5 + i * 0.06) * 0.12;
        // Center emphasis — bars near the middle are taller
        const center = 1 - Math.abs(i / BAR_COUNT - 0.5) * 1.2;
        target[i] = MIN_H + (w1 + w2 + w3 + 0.6) * (MAX_H - MIN_H) * 0.35 * breathe * (0.6 + 0.4 * center);
      }
      // Smooth interpolation
      bars[i] += (target[i] - bars[i]) * 0.18;
    }

    // Prismatic ribbon — continuous filled shape with smooth top edge
    ctx.beginPath();
    ctx.moveTo(0, h);

    for (let i = 0; i < BAR_COUNT; i++) {
      const x = i * BAR_GAP + BAR_GAP / 2;
      const barH = bars[i];
      const y = h - barH;
      if (i === 0) {
        ctx.lineTo(0, y);
      } else {
        const prevX = (i - 1) * BAR_GAP + BAR_GAP / 2;
        const cpX = (prevX + x) / 2;
        ctx.quadraticCurveTo(prevX, h - bars[i - 1], cpX, (h - bars[i - 1] + y) / 2);
        ctx.quadraticCurveTo(x, y, x, y);
      }
    }

    ctx.lineTo(CANVAS_W, h);
    ctx.closePath();

    // Fill with prismatic gradient
    const grad = ctx.createLinearGradient(0, 0, w, 0);
    grad.addColorStop(0, prismAt(0));
    grad.addColorStop(0.33, prismAt(0.33));
    grad.addColorStop(0.66, prismAt(0.66));
    grad.addColorStop(1, prismAt(1));

    ctx.globalAlpha = hasAudio ? 0.65 : 0.28;
    ctx.fillStyle = grad;
    ctx.fill();

    // Glow pass
    ctx.globalAlpha = hasAudio ? 0.35 : 0.15;
    ctx.filter = 'blur(4px)';
    ctx.fill();
    ctx.filter = 'none';

    // Highlight edge along the top
    ctx.beginPath();
    for (let i = 0; i < BAR_COUNT; i++) {
      const x = i * BAR_GAP + BAR_GAP / 2;
      const y = h - bars[i];
      if (i === 0) {
        ctx.moveTo(0, y);
      } else {
        const prevX = (i - 1) * BAR_GAP + BAR_GAP / 2;
        const cpX = (prevX + x) / 2;
        ctx.quadraticCurveTo(prevX, h - bars[i - 1], cpX, (h - bars[i - 1] + y) / 2);
        ctx.quadraticCurveTo(x, y, x, y);
      }
    }
    ctx.strokeStyle = grad;
    ctx.lineWidth = 1.5;
    ctx.globalAlpha = hasAudio ? 0.85 : 0.4;
    ctx.stroke();

    ctx.globalAlpha = 1;
    rafRef.current = requestAnimationFrame(draw);
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas) {
      canvas.width = CANVAS_W;
      canvas.height = MAX_H;
    }
    rafRef.current = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(rafRef.current);
  }, [draw]);

  const handleSubmit = () => {
    if (disabled) return;
    if (!value.trim()) return;
    onSend(value);
    setValue('');
    requestAnimationFrame(() => {
      if (textareaRef.current) {
        textareaRef.current.style.height = '48px';
      }
    });
  };

  const handleButtonClick = () => {
    if (disabled) {
      // Double-click detection when streaming: stop generation
      const now = Date.now();
      if (now - lastClickRef.current < 400) {
        onStop?.();
        lastClickRef.current = 0;
        return;
      }
      lastClickRef.current = now;
      return;
    }
    handleSubmit();
  };

  const handlePasteFill = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    if (clipboardText) {
      setValue(clipboardText);
      setClipboardText('');
    }
  }, [clipboardText]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      // Double-Enter on empty textarea: paste clipboard → send
      if (!value.trim() && clipboardText) {
        const now = Date.now();
        if (now - lastEnterRef.current < 400) {
          onSend(clipboardText);
          setClipboardText('');
          setValue('');
          lastEnterRef.current = 0;
          setSendPulse(true);
          setTimeout(() => setSendPulse(false), 600);
          return;
        }
        lastEnterRef.current = now;
        setValue(clipboardText);
        setClipboardText('');
        return;
      }
      handleSubmit();
    }
  };

  const handleToolSelect = (syntax: string) => {
    const textarea = textareaRef.current;
    if (!textarea) {
      setValue((prev) => prev + syntax);
      return;
    }
    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    const newValue = value.slice(0, start) + syntax + value.slice(end);
    setValue(newValue);
    requestAnimationFrame(() => {
      textarea.focus();
      const pos = start + syntax.length;
      textarea.setSelectionRange(pos, pos);
    });
  };

  const handleDirectExecute = (syntax: string) => {
    onSend(syntax);
  };

  const showClipHint = !value && clipboardText;

  return (
    <>
      <canvas ref={canvasRef} className="audio-spectrum" />
      <div className={`input-bar${focused ? ' focused' : ''}`}>
        <ToolSelector baseUrl={BASE_URL} onSelect={handleToolSelect} onDirectExecute={handleDirectExecute} toolEvents={toolEvents} />
        <div className="textarea-wrap">
          <textarea
            ref={textareaRef}
            value={value}
            onChange={e => setValue(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="说点什么吧~ (Enter 发送，Shift+Enter 换行)"
            disabled={disabled}
            rows={1}
          />
          {showClipHint && (
            <span className="textarea-clip-hint" onMouseDown={handlePasteFill}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="8" y="2" width="8" height="4" rx="1" ry="1" />
                <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
              </svg>
              {(() => { const t = clipboardText.replace(/[\r\n]+/g, ' ').trim(); return t.length > 30 ? `${t.slice(0, 30)}...` : t; })()}
            </span>
          )}
        </div>
        <LgGlassInteractive>
        <button className={`send-btn${disabled ? ' streaming' : ''}${sendPulse ? ' send-pulse' : ''}`} onClick={handleButtonClick} disabled={!disabled && !value.trim()} aria-label={disabled ? '双击停止生成' : '发送消息'} title={disabled ? '双击停止生成' : undefined}>
          {disabled ? (
            <svg className="send-btn-spinner" width="16" height="16" viewBox="0 0 16 16" fill="none">
              <circle cx="8" cy="8" r="6" stroke="currentColor" strokeWidth="2" strokeDasharray="28" strokeDashoffset="8" strokeLinecap="round" />
            </svg>
          ) : (
            <>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <line x1="22" y1="2" x2="11" y2="13" />
                <polygon points="22 2 15 22 11 13 2 9 22 2" />
              </svg>
              发送
            </>
          )}
        </button>
        </LgGlassInteractive>
      </div>
    </>
  );
});
