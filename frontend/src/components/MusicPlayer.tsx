import { useState, useRef, useEffect, useCallback, memo } from 'react';
import type { MediaItemMeta } from '../hooks/useMediaLibrary';
import { MediaLibraryPanel } from './MediaLibraryPanel';
import { getMediaLyrics, updateMediaLyrics } from '../utils/mediaLibraryStorage';
import { useLyrics } from '../hooks/useLyrics';
import { MusicPlaylist } from './music/MusicPlaylist';
import { MusicLyrics } from './music/MusicLyrics';
import { drawIdle, drawLive } from './music/waveform';

export interface MusicTrack {
  url: string;
  name: string;
}

interface MusicLibraryData {
  music: MediaItemMeta[];
  importFiles: (files: FileList | File[], folder?: string) => Promise<number>;
  importFolder: (files: FileList, folder?: string) => Promise<number>;
  remove: (id: string) => Promise<void>;
  removeFolder?: (folder: string, type: 'music') => Promise<void>;
  removeAll?: (type: 'music') => Promise<void>;
  getUrl: (id: string) => Promise<string | null>;
}

interface Props {
  musicPlaying: boolean;
  musicMuted: boolean;
  musicVolume: number;
  musicFile: string | null;
  playlist: MusicTrack[];
  playlistIndex: number;
  onTogglePlay: () => void;
  onToggleMute: () => void;
  onVolumeChange: (volume: number) => void;
  onFileChange: (file: string | null) => void;
  onNext: () => void;
  onPrev: () => void;
  onAddTrack: (track: MusicTrack) => void;
  onRemoveTrack?: (index: number) => void;
  onSelectTrack?: (index: number) => void;
  expanded: boolean;
  onExpandedChange: (expanded: boolean) => void;
  onAnalyserReady?: (analyser: AnalyserNode | null) => void;
  hideGlass?: boolean;
  visible?: boolean;
  musicLibrary?: MusicLibraryData;
}

/* ── Helpers ── */
function fmt(s: number): string {
  if (!isFinite(s) || s < 0) return '0:00';
  return `${Math.floor(s / 60)}:${Math.floor(s % 60).toString().padStart(2, '0')}`;
}

function trackLabel(url: string | null): string {
  if (!url) return '';
  if (url.startsWith('blob:')) return '正在播放';
  const last = url.split('/').pop() || '';
  return last.replace(/\.[^.]+$/, '') || '正在播放';
}

export const MusicPlayer = memo(function MusicPlayer({
  musicPlaying, musicMuted, musicVolume, musicFile,
  playlist, playlistIndex,
  onTogglePlay, onToggleMute, onVolumeChange, onFileChange,
  onNext, onPrev, onAddTrack, onRemoveTrack, onSelectTrack,
  expanded, onExpandedChange, onAnalyserReady, hideGlass, visible,
  musicLibrary,
}: Props) {
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [showLibrary, setShowLibrary] = useState(false);
  const [pinned, setPinned] = useState(() => {
    try { return localStorage.getItem('agent-teams-music-pinned') === 'true'; } catch { return false; }
  });
  const [lrcText, setLrcText] = useState<string | null>(null);
  const lrcInputRef = useRef<HTMLInputElement>(null);
  const lyricLineRef = useRef<HTMLSpanElement>(null);
  const prevLyricIdx = useRef<number | undefined>(undefined);
  const libItemIdRef = useRef<string | null>(null);

  const [timingOffset, setTimingOffset] = useState(0);
  const offsetKeyRef = useRef<string | null>(null);

  const loadOffset = useCallback((key: string) => {
    try { const v = localStorage.getItem(`agent-teams-lyric-offset-${key}`); setTimingOffset(v ? parseFloat(v) : 0); }
    catch { setTimingOffset(0); }
  }, []);

  const saveOffset = useCallback((key: string, val: number) => {
    try { localStorage.setItem(`agent-teams-lyric-offset-${key}`, String(val)); } catch {}
  }, []);

  const handleOffsetChange = useCallback((v: number) => {
    setTimingOffset(v);
    if (offsetKeyRef.current) saveOffset(offsetKeyRef.current, v);
  }, [saveOffset]);

  const prevPlRef = useRef<{ len: number; idx: number }>({ len: 0, idx: -1 });
  useEffect(() => {
    const prevPl = prevPlRef.current;
    const trackChanged = playlist.length !== prevPl.len || playlistIndex !== prevPl.idx;
    prevPl.len = playlist.length;
    prevPl.idx = playlistIndex;

    if (trackChanged) {
      setLrcText(null);
      prevLyricIdx.current = undefined;
      libItemIdRef.current = null;
      const trackKey = playlist[playlistIndex]?.name || '';
      offsetKeyRef.current = trackKey;
      if (trackKey) loadOffset(trackKey); else setTimingOffset(0);
    }

    if (!playlist.length || playlistIndex >= playlist.length) return;
    const track = playlist[playlistIndex];
    const trackBase = track.name.replace(/\.[^.]+$/, '');
    const libItem = musicLibrary?.music.find(m => m.name === track.name || m.name === trackBase || m.id === track.url);
    if (!libItem) { libItemIdRef.current = null; return; }
    if (!trackChanged && libItemIdRef.current === libItem.id) return;
    libItemIdRef.current = libItem.id;
    let cancelled = false;
    getMediaLyrics(libItem.id).then(lrc => { if (!cancelled && lrc) setLrcText(lrc); }).catch(() => {});
    return () => { cancelled = true; };
  }, [playlist, playlistIndex, musicLibrary, loadOffset]);

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.id && detail.id === libItemIdRef.current) {
        getMediaLyrics(detail.id).then(lrc => { if (lrc) setLrcText(lrc); }).catch(() => {});
      }
    };
    window.addEventListener('music-lyrics-ready', handler);
    return () => window.removeEventListener('music-lyrics-ready', handler);
  }, []);

  const handleLrcImport = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      const text = ev.target?.result as string;
      if (!text) return;
      setLrcText(text);
      if (playlist.length && playlistIndex < playlist.length) {
        const track = playlist[playlistIndex];
        const libItem = musicLibrary?.music.find(m => m.name === track.name || m.id === track.url);
        if (libItem) updateMediaLyrics(libItem.id, text).catch(() => {});
      }
    };
    reader.readAsText(file);
    e.target.value = '';
  }, [playlist, playlistIndex, musicLibrary]);

  const { currentText, nextText, hasLyrics, currentIndex, lineProgress, currentLineTime } = useLyrics({ lrc: lrcText, currentTime, offset: timingOffset });

  useEffect(() => {
    if (currentIndex === prevLyricIdx.current) return;
    prevLyricIdx.current = currentIndex;
    const el = lyricLineRef.current;
    if (!el) return;
    el.style.transition = 'none';
    el.style.opacity = '0';
    requestAnimationFrame(() => {
      requestAnimationFrame(() => { el.style.transition = 'opacity 0.35s ease-out'; el.style.opacity = '1'; });
    });
  }, [currentIndex]);

  const containerRef = useRef<HTMLDivElement>(null);
  const hoverTimer = useRef(0);
  const audioRef = useRef<HTMLAudioElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [canvasReady, setCanvasReady] = useState(false);
  const canvasCallbackRef = useCallback((el: HTMLCanvasElement | null) => { canvasRef.current = el; setCanvasReady(!!el); }, []);
  const progressRef = useRef<HTMLDivElement>(null);

  const analyserRef = useRef<AnalyserNode | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const liveRaf = useRef(0);
  const idleRaf = useRef(0);
  const playIntentRef = useRef(false); // Track if we intend to be playing

  useEffect(() => { try { localStorage.setItem('agent-teams-music-pinned', String(pinned)); } catch {} }, [pinned]);

  const togglePinned = useCallback(() => { setPinned(p => { if (!p) onExpandedChange(true); return !p; }); }, [onExpandedChange]);

  const capsuleRef = useRef<HTMLDivElement>(null);
  const expandRef = useRef<HTMLDivElement>(null);
  const isInsideRef = useRef(false);

  useEffect(() => {
    const check = (e: MouseEvent) => {
      if (pinned) return;
      const cap = capsuleRef.current, exp = expandRef.current;
      const mx = e.clientX, my = e.clientY;
      const insideCap = cap && (() => { const r = cap.getBoundingClientRect(); return mx >= r.left && mx <= r.right && my >= r.top && my <= r.bottom; })();
      const insideExp = exp && (() => { const r = exp.getBoundingClientRect(); return mx >= r.left && mx <= r.right && my >= r.top && my <= r.bottom; })();
      const inside = !!insideCap || !!insideExp;
      if (inside && !isInsideRef.current) { isInsideRef.current = true; clearTimeout(hoverTimer.current); onExpandedChange(true); }
      else if (!inside && isInsideRef.current) { isInsideRef.current = false; clearTimeout(hoverTimer.current); hoverTimer.current = window.setTimeout(() => { if (!isInsideRef.current) onExpandedChange(false); }, 280); }
    };
    document.addEventListener('mousemove', check);
    return () => { document.removeEventListener('mousemove', check); isInsideRef.current = false; };
  }, [pinned, onExpandedChange]);

  const setupAnalyser = useCallback(() => {
    const audio = audioRef.current;
    if (!audio || audioCtxRef.current) return;
    try {
      const ctx = new AudioContext();
      const src = ctx.createMediaElementSource(audio);
      const an = ctx.createAnalyser();
      an.fftSize = 128; an.smoothingTimeConstant = 0.82;
      src.connect(an); an.connect(ctx.destination);
      audioCtxRef.current = ctx; analyserRef.current = an;
      onAnalyserReady?.(an);
    } catch {}
  }, [onAnalyserReady]);

  useEffect(() => {
    return () => {
      onAnalyserReady?.(null);
      if (audioCtxRef.current) { audioCtxRef.current.close().catch(() => {}); audioCtxRef.current = null; analyserRef.current = null; }
    };
  }, [onAnalyserReady]);

  useEffect(() => {
    if (musicPlaying && musicFile) return;
    const cv = canvasRef.current;
    if (!cv) return;
    let ph = 0;
    const tick = () => { ph += 0.02; drawIdle(cv, ph); idleRaf.current = requestAnimationFrame(tick); };
    tick();
    return () => cancelAnimationFrame(idleRaf.current);
  }, [musicPlaying, musicFile, canvasReady]);

  useEffect(() => {
    if (!musicPlaying || !musicFile) { cancelAnimationFrame(liveRaf.current); return; }
    cancelAnimationFrame(idleRaf.current);
    setupAnalyser();
    const cv = canvasRef.current, an = analyserRef.current;
    if (!cv || !an) return;
    const ctx = cv.getContext('2d');
    if (!ctx) return;
    const buf = new Uint8Array(an.frequencyBinCount);
    const draw = () => { liveRaf.current = requestAnimationFrame(draw); an.getByteFrequencyData(buf); drawLive(cv, buf); };
    const t = setTimeout(draw, 60);
    return () => { clearTimeout(t); cancelAnimationFrame(liveRaf.current); };
  }, [musicPlaying, musicFile, setupAnalyser, canvasReady]);

  useEffect(() => {
    const a = audioRef.current;
    if (!a) return;
    if (musicPlaying && musicFile) {
      playIntentRef.current = true;
      a.play().then(() => {
        // Play succeeded - check if we still intend to play
        if (!playIntentRef.current) {
          a.pause();
        }
      }).catch(() => {
        // Play failed (e.g., interrupted by pause) - ensure state matches
        playIntentRef.current = false;
      });
    } else {
      playIntentRef.current = false;
      a.pause();
    }
  }, [musicPlaying, musicFile]);

  useEffect(() => { if (audioRef.current) audioRef.current.volume = musicVolume; }, [musicVolume]);
  useEffect(() => { if (audioRef.current) audioRef.current.muted = musicMuted; }, [musicMuted]);

  const onTime = useCallback(() => { if (audioRef.current) setCurrentTime(audioRef.current.currentTime); }, []);
  const onMeta = useCallback(() => { if (audioRef.current) setDuration(audioRef.current.duration); }, []);
  const handleEnded = useCallback(() => {
    playIntentRef.current = false;
    if (playlist.length > 1) onNext();
    else { setCurrentTime(0); if (audioRef.current) audioRef.current.currentTime = 0; }
  }, [playlist.length, onNext]);

  const handleLibrarySelect = useCallback(async (id: string) => {
    if (!musicLibrary) return;
    const url = await musicLibrary.getUrl(id);
    if (!url) return;
    const item = musicLibrary.music.find(m => m.id === id);
    onAddTrack({ url, name: item?.name || '未知曲目' });
    setShowLibrary(false);
  }, [musicLibrary, onAddTrack]);

  const handleLibraryRemove = useCallback(async (id: string) => { if (!musicLibrary) return; await musicLibrary.remove(id); }, [musicLibrary]);

  const handleSeek = useCallback((e: React.MouseEvent) => {
    const bar = progressRef.current, audio = audioRef.current;
    if (!bar || !audio || !duration) return;
    const r = bar.getBoundingClientRect();
    audio.currentTime = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * duration;
    setCurrentTime(audio.currentTime);
  }, [duration]);

  const pct = duration > 0 ? (currentTime / duration) * 100 : 0;
  const currentTrack = playlist.length > 0 && playlistIndex < playlist.length ? playlist[playlistIndex] : null;
  const label = currentTrack?.name || trackLabel(musicFile);
  const hasFile = !!musicFile;
  const showExpanded = expanded || pinned;
  const showCapsule = !expanded && !pinned;

  return (
    <div ref={containerRef} className={`music-player${visible === false ? ' hidden' : ''}`}>
      <audio ref={audioRef} src={musicFile || undefined} loop preload="auto"
        onTimeUpdate={onTime} onLoadedMetadata={onMeta} onEnded={handleEnded} />

      {/* Capsule (compact view) */}
      <div ref={capsuleRef}
        className={`music-capsule${hideGlass ? ' hide-glass' : ''}${showCapsule ? ' visible' : ''}${musicPlaying ? ' playing' : ''}`}>
        <div className="music-cap-disc">
          <div className="music-cap-disc-inner">
            <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" />
            </svg>
          </div>
        </div>
        <div className="music-cap-info">
          {hasLyrics && currentText ? (
            <span className="music-cap-lyric" ref={lyricLineRef}>{currentText}</span>
          ) : (
            <span className="music-cap-name">{hasFile ? label : '未选择音乐'}</span>
          )}
        </div>
        {duration > 0 && <div className="music-cap-progress"><div className="music-cap-progress-fill" style={{ width: `${pct}%` }} /></div>}
      </div>

      {/* Expanded panel */}
      {showExpanded && (
        <div ref={expandRef}
          className={`music-expanded${hideGlass ? ' hide-glass' : ''}${(expanded || pinned) ? ' visible' : ''}${pinned ? ' pinned' : ''}`}>
          <div className="music-exp-caustic" />

          <div className="music-exp-header">
            <div className="music-exp-track">
              <div className={`music-exp-disc${musicPlaying ? ' spin' : ''}`}>
                <div className="music-exp-disc-in">
                  <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" />
                  </svg>
                </div>
              </div>
              <div className="music-exp-info">
                {hasFile ? <span className="music-exp-sub">{currentTrack?.name || label}</span> : <span className="music-exp-name">未选择音乐</span>}
              </div>
            </div>
            <div className="music-exp-actions">
              <button className={`music-exp-pin${pinned ? ' active' : ''}`} onClick={togglePinned} title={pinned ? '取消钉住' : '钉住'} aria-label={pinned ? '取消钉住' : '钉住'}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill={pinned ? 'currentColor' : 'none'} stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M12 17v5" /><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 1 1 0 0 0 1-1V4a2 2 0 0 0-2-2H9a2 2 0 0 0-2 2v1a1 1 0 0 0 1 1 1 1 0 0 1 1 1z" />
                </svg>
              </button>
              {musicLibrary && (
                <button className="music-exp-add" onClick={() => setShowLibrary(!showLibrary)} title="素材库" aria-label="素材库">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <rect x="3" y="3" width="7" height="7" /><rect x="14" y="3" width="7" height="7" /><rect x="14" y="14" width="7" height="7" /><rect x="3" y="14" width="7" height="7" />
                  </svg>
                </button>
              )}
              <button className={`music-exp-add${hasLyrics ? ' has-lyrics' : ''}`} onClick={() => lrcInputRef.current?.click()} title="导入歌词" aria-label="导入歌词">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" />
                </svg>
              </button>
              <input ref={lrcInputRef} type="file" accept=".lrc,.txt" onChange={handleLrcImport} style={{ display: 'none' }} />
            </div>
          </div>

          <MusicLyrics
            hasLyrics={hasLyrics} currentText={currentText} nextText={nextText}
            lineProgress={lineProgress} timingOffset={timingOffset}
            onOffsetChange={handleOffsetChange}
          />

          <div className="music-exp-wave-wrap">
            <canvas ref={canvasCallbackRef} className="music-exp-wave" width={300} height={36} />
          </div>

          {hasFile && (
            <div className="music-exp-prog-sec">
              <div className="music-exp-prog" ref={progressRef} onClick={handleSeek}>
                <div className="music-exp-prog-bg" />
                <div className="music-exp-prog-fill" style={{ width: `${pct}%` }} />
                <div className="music-exp-prog-dot" style={{ left: `${pct}%` }} />
              </div>
              <div className="music-exp-time">
                <span className="music-exp-time-cur">{fmt(currentTime)}</span>
                <span className="music-exp-time-sep">/</span>
                <span className="music-exp-time-dur">{fmt(duration)}</span>
              </div>
              {hasLyrics && currentLineTime !== null && (
                <div className="music-exp-debug" title="调试信息：播放时间 vs 歌词时间">
                  <span className="music-exp-debug-label">Δ</span>
                  <span className="music-exp-debug-value">{(currentTime - timingOffset - currentLineTime).toFixed(2)}s</span>
                </div>
              )}
            </div>
          )}

          <div className="music-exp-ctrls">
            <div className="music-exp-ctrl-l">
              <button className="music-exp-ctrl-btn mint" onClick={onToggleMute} title={musicMuted ? '取消静音' : '静音'} aria-label={musicMuted ? '取消静音' : '静音'}>
                {musicMuted
                  ? <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" /><line x1="23" y1="9" x2="17" y2="15" /><line x1="17" y1="9" x2="23" y2="15" /></svg>
                  : <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" /><path d="M15.54 8.46a5 5 0 0 1 0 7.07" /></svg>}
              </button>
            </div>
            <button className="music-exp-ctrl-btn" onClick={onPrev} title="上一首" aria-label="上一首">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M19 20L9 12l10-8v16zM7 19V5H5v14h2z" /></svg>
            </button>
            <button className="music-exp-ctrl-btn play" onClick={onTogglePlay} title={musicPlaying ? '暂停' : '播放'} aria-label={musicPlaying ? '暂停' : '播放'} disabled={!hasFile}>
              {musicPlaying
                ? <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1.2" /><rect x="14" y="4" width="4" height="16" rx="1.2" /></svg>
                : <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><polygon points="7 4 21 12 7 20 7 4" /></svg>}
            </button>
            <button className="music-exp-ctrl-btn" onClick={onNext} title="下一首" aria-label="下一首">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M5 4l10 8-10 8V4zm12-1v14h2V5h-2z" /></svg>
            </button>
            <div className="music-exp-ctrl-r">
              {hasFile && (
                <button className="music-exp-ctrl-btn peach" onClick={() => { playIntentRef.current = false; onFileChange(null); setCurrentTime(0); setDuration(0); if (audioRef.current) { audioRef.current.pause(); audioRef.current.src = ''; } }} title="移除" aria-label="移除">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg>
                </button>
              )}
            </div>
          </div>

          <div className="music-exp-vol">
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" /></svg>
            <input type="range" min="0" max="1" step="0.01" value={musicVolume} onChange={(e) => onVolumeChange(parseFloat(e.target.value))} className="music-exp-vol-slider" />
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" /><path d="M19.07 4.93a10 10 0 0 1 0 14.14" /><path d="M15.54 8.46a5 5 0 0 1 0 7.07" /></svg>
          </div>

          <MusicPlaylist
            playlist={playlist} playlistIndex={playlistIndex} musicPlaying={musicPlaying}
            onSelectTrack={(i) => { onSelectTrack?.(i); setCurrentTime(0); setDuration(0); }}
            onRemoveTrack={(i) => onRemoveTrack?.(i)}
          />

          {showLibrary && musicLibrary && (
            <div className="music-lib-overlay">
              <div className="music-lib-header">
                <span>音乐素材库</span>
                <button className="music-lib-close" onClick={() => setShowLibrary(false)}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                  </svg>
                </button>
              </div>
              <MediaLibraryPanel
                type="music" items={musicLibrary.music} selectedId={null}
                onSelect={handleLibrarySelect} onRemove={handleLibraryRemove}
                onRemoveFolder={musicLibrary.removeFolder ? (folder) => musicLibrary.removeFolder!(folder, 'music') : undefined}
                onRemoveAll={musicLibrary.removeAll ? () => musicLibrary.removeAll!('music') : undefined}
                onImportFiles={musicLibrary.importFiles} onImportFolder={musicLibrary.importFolder}
                getUrl={musicLibrary.getUrl}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
});
