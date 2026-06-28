import { useState, useEffect, useRef, memo } from 'react';
import { AgentTeamsClient } from '../api/client';
import { PresetSelector } from './PresetSelector';
import { LgGlassInteractive } from './LgGlassInteractive';
import type { HealthResponse, PresetDef, CustomPreset } from '../api/types';

interface Props {
  baseUrl: string;
  onClear: () => void;
  onToggleSidebar?: () => void;
  onOpenSettings?: () => void;
  onOpenChangelog?: () => void;
  focused?: boolean;
  hideGlass?: boolean;
  videoPlaying?: boolean;
  onTogglePlay?: () => void;
  videoMuted?: boolean;
  musicMuted?: boolean;
  onToggleMute?: () => void;
  onToggleMusicMute?: () => void;
  presetLocked?: boolean;
  activePresetId?: string | null;
  onSelectPreset?: (presetId: string | null) => void;
  builtinPresets?: PresetDef[];
  customPresets?: CustomPreset[];
  onAddCustomPreset?: (preset: CustomPreset) => void;
  onUpdateCustomPreset?: (id: string, updates: Partial<CustomPreset>) => void;
  onDeleteCustomPreset?: (id: string) => void;
  showMusic?: boolean;
  musicPlaying?: boolean;
  onToggleMusicPanel?: () => void;
}

export const StatusBar = memo(function StatusBar({ baseUrl, onClear, onToggleSidebar, onOpenSettings, onOpenChangelog, focused, hideGlass, videoPlaying, onTogglePlay, videoMuted, musicMuted, onToggleMute, onToggleMusicMute, presetLocked, activePresetId, onSelectPreset, builtinPresets, customPresets, onAddCustomPreset, onUpdateCustomPreset, onDeleteCustomPreset, showMusic, musicPlaying, onToggleMusicPanel }: Props) {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [presetPanelOpen, setPresetPanelOpen] = useState(false);
  const presetAnchorRef = useRef<HTMLDivElement>(null);
  const clientRef = useRef(new AgentTeamsClient(baseUrl));

  // Close preset panel on outside click
  useEffect(() => {
    if (!presetPanelOpen) return;
    const handler = (e: MouseEvent) => {
      if (presetAnchorRef.current && !presetAnchorRef.current.contains(e.target as Node)) {
        setPresetPanelOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [presetPanelOpen]);

  // Update client when baseUrl changes
  useEffect(() => {
    clientRef.current = new AgentTeamsClient(baseUrl);
  }, [baseUrl]);

  useEffect(() => {
    const check = async () => {
      try {
        const h = await clientRef.current.health();
        setHealth(h);
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Connection failed');
        setHealth(null);
      }
    };
    check();
    const interval = setInterval(check, 30_000);
    return () => clearInterval(interval);
  }, [baseUrl]);

  const allMuted = (videoPlaying && videoMuted) || (!!musicPlaying && musicMuted === true);

  return (
    <div className={`status-bar${focused ? ' focused' : ''}${hideGlass ? ' hide-glass' : ''}`}>
      <div className="status-left">
        {onToggleSidebar && (
          <LgGlassInteractive>
            <button className="sidebar-toggle" onClick={onToggleSidebar} title="聊天记录" aria-label="聊天记录">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <line x1="3" y1="6" x2="21" y2="6" />
                <line x1="3" y1="12" x2="21" y2="12" />
                <line x1="3" y1="18" x2="21" y2="18" />
              </svg>
            </button>
          </LgGlassInteractive>
        )}
        <div className="status-preset-anchor" ref={presetAnchorRef}>
          <button
            type="button"
            className="status-center-btn"
            onClick={() => { if (!presetLocked) setPresetPanelOpen(v => !v); }}
            title={presetLocked ? '人格已锁定' : '点击选择人格'}
            disabled={presetLocked}
          >
            <span className={`status-center-info${error ? ' disconnected' : ''}${presetPanelOpen ? ' preset-open' : ''}`}>
              <span className="status-title">Main x Sub～</span>
              <span className={`model-badge-wrap${!health?.model && !error ? ' hidden' : ''}`}>
                <span className={`model-particles${presetPanelOpen ? ' burst' : ''}`}>
                  <i /><i /><i /><i /><i /><i />
                  <i /><i /><i /><i /><i /><i />
                </span>
                <span className="model-shimmer" />
                <span className={`model-badge${error ? ' disconnected' : ''}`}>
                  {error ? '连接失败' : (health?.model ?? '')}
                </span>
              </span>
            </span>
          </button>
          <div className="status-preset-panel-wrap" style={{ display: presetPanelOpen && !presetLocked ? 'block' : 'none' }}>
            <PresetSelector
              baseUrl={baseUrl}
              activePresetId={activePresetId ?? null}
              onSelect={(id) => { onSelectPreset?.(id); setPresetPanelOpen(false); }}
              builtinPresets={builtinPresets ?? []}
              customPresets={customPresets ?? []}
              onAddCustom={onAddCustomPreset ?? (() => {})}
              onUpdateCustom={onUpdateCustomPreset ?? (() => {})}
              onDeleteCustom={onDeleteCustomPreset ?? (() => {})}
              externalOpen={presetPanelOpen && !presetLocked}
              onExternalOpenHandled={() => {}}
              locked={false}
            />
          </div>
        </div>
      </div>
      <div className="status-right">
        {videoPlaying !== undefined && onTogglePlay && (
          <LgGlassInteractive>
            <button className="btn-icon" onClick={onTogglePlay} title={videoPlaying ? '暂停背景视频' : '播放背景视频'} aria-label={videoPlaying ? '暂停背景视频' : '播放背景视频'}>
              {videoPlaying ? (
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="6" y="4" width="4" height="16" />
                  <rect x="14" y="4" width="4" height="16" />
                </svg>
              ) : (
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polygon points="5 3 19 12 5 21 5 3" />
                </svg>
              )}
            </button>
          </LgGlassInteractive>
        )}
        {(videoMuted !== undefined || musicMuted !== undefined) && onToggleMute && (
          <LgGlassInteractive>
            <button className={`btn-icon${allMuted ? ' muted' : ''}`} onClick={() => { onToggleMute(); onToggleMusicMute?.(); }} title={allMuted ? '取消静音' : '静音'} aria-label={allMuted ? '取消静音' : '静音'}>
              {allMuted ? (
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                  <line x1="23" y1="9" x2="17" y2="15" />
                  <line x1="17" y1="9" x2="23" y2="15" />
                </svg>
              ) : (
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                  <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
                  <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
                </svg>
              )}
            </button>
          </LgGlassInteractive>
        )}
        {showMusic && onToggleMusicPanel && (
          <LgGlassInteractive>
            <button className={`btn-icon music-entry-btn${musicPlaying ? ' playing' : ''}`} onClick={onToggleMusicPanel} title="音乐" aria-label="音乐">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M9 18V5l12-2v13" />
                <circle cx="6" cy="18" r="3" />
                <circle cx="18" cy="16" r="3" />
              </svg>
              {musicPlaying && <span className="music-entry-pulse" />}
            </button>
          </LgGlassInteractive>
        )}
        {onOpenChangelog && (
          <LgGlassInteractive>
            <button className="btn-icon" onClick={onOpenChangelog} title="更新日志" aria-label="更新日志">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                <polyline points="14 2 14 8 20 8" />
                <line x1="16" y1="13" x2="8" y2="13" />
                <line x1="16" y1="17" x2="8" y2="17" />
                <polyline points="10 9 9 9 8 9" />
              </svg>
            </button>
          </LgGlassInteractive>
        )}
        {onOpenSettings && (
          <LgGlassInteractive>
            <button className="btn-icon" onClick={onOpenSettings} title="设置" aria-label="设置">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
              </svg>
            </button>
          </LgGlassInteractive>
        )}
        <LgGlassInteractive>
          <button className="btn-clear" onClick={onClear} title="新建会话">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            新建会话
          </button>
        </LgGlassInteractive>
      </div>
    </div>
  );
});
