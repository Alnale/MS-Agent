import { useState } from 'react';
import { ColorPicker } from '../ColorPicker';

function hexToRgb(hex: string): string {
  const m = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
  return m ? `${parseInt(m[1],16)},${parseInt(m[2],16)},${parseInt(m[3],16)}` : '242,128,160';
}

interface Props {
  useSolidBubble: boolean;
  bubbleTextColor: string;
  userBubbleColor: string;
  userBubbleAlpha: number;
  assistantBubbleColor: string;
  assistantBubbleAlpha: number;
  solidUserBubbleColor: string;
  solidAssistantBubbleColor: string;
  autoTextEnabled: boolean;
  bgVideo: string | null;
  onBubbleTextColorChange: (color: string) => void;
  onUserBubbleColorChange: (color: string) => void;
  onUserBubbleAlphaChange: (alpha: number) => void;
  onAssistantBubbleColorChange: (color: string) => void;
  onAssistantBubbleAlphaChange: (alpha: number) => void;
  onSolidUserBubbleColorChange: (color: string) => void;
  onSolidAssistantBubbleColorChange: (color: string) => void;
}

export function BubbleColorSettings({
  useSolidBubble, bubbleTextColor, userBubbleColor, userBubbleAlpha,
  assistantBubbleColor, assistantBubbleAlpha, solidUserBubbleColor, solidAssistantBubbleColor,
  autoTextEnabled, bgVideo,
  onBubbleTextColorChange, onUserBubbleColorChange, onUserBubbleAlphaChange,
  onAssistantBubbleColorChange, onAssistantBubbleAlphaChange,
  onSolidUserBubbleColorChange, onSolidAssistantBubbleColorChange,
}: Props) {
  const [bubbleColorOpen, setBubbleColorOpen] = useState(false);

  return (
    <>
      <button className="settings-collapse-header" onClick={() => setBubbleColorOpen(!bubbleColorOpen)} aria-expanded={bubbleColorOpen}>
        <span className="settings-label">气泡颜色</span>
        <svg className={`settings-collapse-chevron${bubbleColorOpen ? ' open' : ''}`} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      <div className={`settings-collapse-body${bubbleColorOpen ? ' open' : ''}`}>
        <div className="settings-collapse-inner">
          {useSolidBubble ? (
            <>
              <div className="cp-theme-section">
                <span className="cp-theme-label">快速配色</span>
                <div className="cp-themes">
                  {[
                    { name: '白瓷', u: '#ffffff', a: '#ffffff', t: '#1a1a2e' },
                    { name: '浅灰', u: '#f3f4f6', a: '#f3f4f6', t: '#1a1a2e' },
                    { name: '暖白', u: '#fefce8', a: '#fefce8', t: '#1a1a2e' },
                    { name: '淡粉', u: '#fdf2f8', a: '#fdf2f8', t: '#1a1a2e' },
                    { name: '淡蓝', u: '#eff6ff', a: '#eff6ff', t: '#1a1a2e' },
                    { name: '淡绿', u: '#f0fdf4', a: '#f0fdf4', t: '#1a1a2e' },
                    { name: '墨染', u: '#374151', a: '#4b5563', t: '#f3f4f6' },
                    { name: '深色', u: '#1f2937', a: '#111827', t: '#f3f4f6' },
                  ].map(theme => (
                    <button key={theme.name} className="cp-theme-btn" title={theme.name}
                      onClick={() => { onSolidUserBubbleColorChange(theme.u); onSolidAssistantBubbleColorChange(theme.a); onBubbleTextColorChange(theme.t); }}>
                      <div className="cp-theme-preview">
                        <div className="cp-theme-swatch" style={{ background: theme.u }} />
                        <div className="cp-theme-swatch" style={{ background: theme.a }} />
                      </div>
                      <span className="cp-theme-name">{theme.name}</span>
                    </button>
                  ))}
                </div>
              </div>
              <ColorPicker label="用户气泡" color={solidUserBubbleColor} alpha={1}
                presets={['#ffffff', '#f3f4f6', '#fefce8', '#fdf2f8', '#eff6ff', '#f0fdf4', '#374151', '#1f2937']}
                onColorChange={onSolidUserBubbleColorChange} onAlphaChange={() => {}} />
              <ColorPicker label="助手气泡" color={solidAssistantBubbleColor} alpha={1}
                presets={['#ffffff', '#f3f4f6', '#fefce8', '#fdf2f8', '#eff6ff', '#f0fdf4', '#4b5563', '#111827']}
                onColorChange={onSolidAssistantBubbleColorChange} onAlphaChange={() => {}} />
            </>
          ) : (
            <>
              <div className="cp-theme-section">
                <span className="cp-theme-label">快速配色</span>
                <div className="cp-themes">
                  {[
                    { name: '樱花', u: '#f280a0', ua: 0.36, a: '#b8a9e8', aa: 0.35, t: '#1a1a2e' },
                    { name: '薄荷', u: '#6ee7b7', ua: 0.30, a: '#a5b4fc', aa: 0.28, t: '#1a2e2a' },
                    { name: '日落', u: '#fb923c', ua: 0.32, a: '#fbbf24', aa: 0.28, t: '#2e1a0a' },
                    { name: '深海', u: '#60a5fa', ua: 0.30, a: '#818cf8', aa: 0.28, t: '#0a1a2e' },
                    { name: '薰衣草', u: '#c084fc', ua: 0.32, a: '#e879f9', aa: 0.28, t: '#1e0a2e' },
                    { name: '极光', u: '#34d399', ua: 0.28, a: '#8b5cf6', aa: 0.25, t: '#0a2e1a' },
                    { name: '白瓷', u: '#ffffff', ua: 1, a: '#ffffff', aa: 1, t: '#1a1a2e' },
                    { name: '墨染', u: '#374151', ua: 0.65, a: '#4b5563', aa: 0.55, t: '#f3f4f6' },
                  ].map(theme => (
                    <button key={theme.name} className="cp-theme-btn" title={theme.name}
                      onClick={() => { onUserBubbleColorChange(theme.u); onUserBubbleAlphaChange(theme.ua); onAssistantBubbleColorChange(theme.a); onAssistantBubbleAlphaChange(theme.aa); onBubbleTextColorChange(theme.t); }}>
                      <div className="cp-theme-preview">
                        <div className="cp-theme-swatch" style={{ background: `rgba(${hexToRgb(theme.u)},${theme.ua})` }} />
                        <div className="cp-theme-swatch" style={{ background: `rgba(${hexToRgb(theme.a)},${theme.aa})` }} />
                      </div>
                      <span className="cp-theme-name">{theme.name}</span>
                    </button>
                  ))}
                </div>
              </div>
              <ColorPicker label="用户气泡" color={userBubbleColor} alpha={userBubbleAlpha}
                onColorChange={onUserBubbleColorChange} onAlphaChange={onUserBubbleAlphaChange} />
              <ColorPicker label="助手气泡" color={assistantBubbleColor} alpha={assistantBubbleAlpha}
                onColorChange={onAssistantBubbleColorChange} onAlphaChange={onAssistantBubbleAlphaChange} />
            </>
          )}
          <div className={`settings-collapse-body${!(autoTextEnabled && !useSolidBubble && bgVideo) ? ' open' : ''}`}>
            <div className="settings-collapse-inner">
              <ColorPicker label="文字颜色" color={bubbleTextColor} alpha={1}
                presets={['#1a1a2e', '#1e1b2e', '#2d1b3d', '#0a1a2e', '#1a2e2a', '#2e1a0a', '#f3f4f6', '#e5e7eb', '#d1d5db', '#ffffff']}
                onColorChange={onBubbleTextColorChange} onAlphaChange={() => {}} />
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
