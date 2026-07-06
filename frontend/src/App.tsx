import { useRef, useEffect, useCallback, useState, useMemo, lazy, Suspense } from 'react';
import { useChat } from './hooks/useChat';
import { useSession } from './hooks/useSession';
import { usePreset } from './hooks/usePreset';
import { useBackground } from './hooks/useBackground';
import { useAudioAnalyser } from './hooks/useAudioAnalyser';
import { useVideoDominantColor } from './hooks/useVideoDominantColor';
import { useMusic } from './hooks/useMusic';
import { useMediaLibrary } from './hooks/useMediaLibrary';
import { useSettings } from './hooks/useSettings';
import { usePlaylist } from './hooks/usePlaylist';
import { useVideoBackground } from './hooks/useVideoBackground';
import { MessageBubble } from './components/MessageBubble';
import { InputBar } from './components/InputBar';
import { StatusBar } from './components/StatusBar';
import { LgGlassMask } from './components/LgGlassMask';
import { LgGlassCard } from './components/LgGlassCard';
import { LgGlassButton } from './components/LgGlassButton';
import { SessionList } from './components/SessionList';
import { WelcomeScreen } from './components/WelcomeScreen';
import { BgTransition } from './components/BgTransition';
import { MusicPlayer } from './components/MusicPlayer';
import { CompanionPanel } from './components/CompanionPanel';
import { ErrorBoundary } from './components/ErrorBoundary';
import type { ChatMessage, ToolStatusEvent } from './api/types';
import type { ConflictChoice, SubfolderChoice, ConflictInfo, SubfolderInfo } from './components/ImportDialog';
import { BASE_URL } from './config';
import './App.css';

// Lazy-load heavy conditionally-rendered components
const SettingsPanel = lazy(() => import('./components/SettingsPanel'));
const ChangelogPanel = lazy(() => import('./components/ChangelogPanel'));
const ImportDialog = lazy(() => import('./components/ImportDialog'));
const SplashScreen = lazy(() => import('./components/SplashScreen'));

// ── Liquid Glass types ──
export type LgCategory = {
  enabled: boolean;
  mode: 'standard' | 'polar' | 'prominent' | 'shader';
  overLight: boolean;
  displacementScale: number;
  blurAmount: number;
  saturation: number;
  aberrationIntensity: number;
  elasticity: number;
  cornerRadius: number;
};
export type LgConfig = {
  enabled: boolean;
  mask: LgCategory;
  card: LgCategory;
  button: LgCategory;
  companionPanel: boolean;
};

function App() {
  const {
    sessions, currentSessionId, createSession, saveSession,
    deleteSession, deleteSessions, setCurrentSessionId,
  } = useSession();

  const settings = useSettings();
  const [showSplash, setShowSplash] = useState(true);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [changelogOpen, setChangelogOpen] = useState(false);
  const [musicPinned, setMusicPinned] = useState(false);
  const [musicExpanded, setMusicExpanded] = useState(false);
  const [musicAnalyser, setMusicAnalyser] = useState<AnalyserNode | null>(null);
  const [inputFocused, setInputFocused] = useState(false);

  const { bgImage, bgVideo, bgOpacity, bgBlur, bgLoading, setBgImage, setBgVideo, activateBgImage, activateBgVideo, setBgOpacity, setBgBlur } = useBackground();

  /* ── Import dialog state ── */
  type ImportDialogState = {
    mode: 'conflict';
    info: ConflictInfo;
    resolve: (value: { choice: ConflictChoice; remember: boolean }) => void;
  } | {
    mode: 'subfolder';
    info: SubfolderInfo;
    resolve: (value: { choice: SubfolderChoice; remember: boolean }) => void;
  };

  const [importDialog, setImportDialog] = useState<ImportDialogState | null>(null);

  const conflictResolver = useCallback((info: ConflictInfo): Promise<{ choice: ConflictChoice; remember: boolean }> => {
    return new Promise(resolve => setImportDialog({ mode: 'conflict', info, resolve }));
  }, []);

  const subfolderResolver = useCallback((info: SubfolderInfo): Promise<{ choice: SubfolderChoice; remember: boolean }> => {
    return new Promise(resolve => setImportDialog({ mode: 'subfolder', info, resolve }));
  }, []);

  const mediaLibrary = useMediaLibrary(conflictResolver, subfolderResolver);

  const video = useVideoBackground();
  const { musicPlaying, musicMuted, musicVolume, musicFile, setMusicVolume, setMusicFile, setMusicPlaying, toggleMusic, toggleMute: toggleMusicMute } = useMusic();
  const { playlist, playlistIndex, handleAddTrack, handleRemoveTrack, handleSelectTrack, handleMusicNext, handleMusicPrev } = usePlaylist(setMusicFile);

  const handleAnalyserReady = useCallback((an: AnalyserNode | null) => { setMusicAnalyser(an); }, []);

  const musicImportFiles = useCallback((files: FileList | File[], folder?: string) => mediaLibrary.importFilesByType('music', files, folder), [mediaLibrary.importFilesByType]);

  const musicLibraryData = useMemo(() => ({
    music: mediaLibrary.music,
    importFiles: musicImportFiles,
    importFolder: musicImportFiles,
    remove: mediaLibrary.remove,
    removeFolder: mediaLibrary.removeFolder,
    removeAll: mediaLibrary.removeAll,
    getUrl: mediaLibrary.getUrl,
  }), [mediaLibrary.music, musicImportFiles, mediaLibrary.remove, mediaLibrary.removeFolder, mediaLibrary.removeAll, mediaLibrary.getUrl]);

  const handleToggleMusicPinned = useCallback(() => { setMusicPinned(prev => !prev); }, []);

  const videoFreq = useAudioAnalyser(video.videoRef, bgVideo, video.videoPlaying, video.videoMuted);
  const [musicFreq, setMusicFreq] = useState<Uint8Array | null>(null);
  const musicFreqRaf = useRef(0);

  useEffect(() => {
    if (!musicAnalyser || !musicPlaying) { setMusicFreq(null); return; }
    const an = musicAnalyser;
    const buf = new Uint8Array(an.frequencyBinCount);
    let last = 0;
    let active = true;
    const tick = () => {
      if (!active) return;
      musicFreqRaf.current = requestAnimationFrame(tick);
      an.getByteFrequencyData(buf);
      const now = performance.now();
      if (now - last > 33) { last = now; setMusicFreq(new Uint8Array(buf)); }
    };
    tick();
    return () => { active = false; cancelAnimationFrame(musicFreqRaf.current); setMusicFreq(null); };
  }, [musicPlaying, musicAnalyser]);

  const mergedFreq = musicFreq || videoFreq;

  const {
    builtinPresets, customPresets, activePresetId, setActivePresetId,
    sessionPresets, setSessionPreset, getSystemInstructions,
    addCustomPreset, updateCustomPreset, deleteCustomPreset,
  } = usePreset(BASE_URL);

  const handlePresetChange = useCallback((presetId: string | null) => {
    setActivePresetId(presetId);
    if (currentSessionId) setSessionPreset(currentSessionId, presetId);
  }, [setActivePresetId, setSessionPreset, currentSessionId]);

  const sessionIdRef = useRef<string | null>(null);

  const [autoTextColor, setAutoTextColor] = useState('#1a1a2e');
  useVideoDominantColor(video.videoRef, bgVideo, video.videoPlaying && settings.autoTextEnabled, useCallback((color: string) => {
    setAutoTextColor(color);
  }, []));

  const effectiveTextColor = settings.useSolidBubble ? settings.bubbleTextColor : (settings.autoTextEnabled && bgVideo && video.videoPlaying ? autoTextColor : settings.bubbleTextColor);

  const hexToRgba = useCallback((hex: string, alpha: number) => {
    const m = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
    if (!m) return `rgba(242,128,160,${alpha})`;
    return `rgba(${parseInt(m[1],16)},${parseInt(m[2],16)},${parseInt(m[3],16)},${alpha})`;
  }, []);

  const appStyle = useMemo(() => ({
    '--bubble-text-color': effectiveTextColor,
    '--user-bubble-bg': settings.useSolidBubble
      ? settings.solidUserBubbleColor
      : `linear-gradient(135deg, ${hexToRgba(settings.userBubbleColor, settings.userBubbleAlpha)} 0%, ${hexToRgba(settings.assistantBubbleColor, settings.userBubbleAlpha)} 100%)`,
    '--assistant-bubble-bg': settings.useSolidBubble
      ? settings.solidAssistantBubbleColor
      : `linear-gradient(135deg, ${hexToRgba(settings.userBubbleColor, settings.assistantBubbleAlpha)} 0%, ${hexToRgba(settings.assistantBubbleColor, settings.assistantBubbleAlpha)} 100%)`,
    '--user-bubble-border': settings.useSolidBubble ? settings.solidUserBubbleColor : hexToRgba(settings.userBubbleColor, Math.min(1, settings.userBubbleAlpha + 0.15)),
    '--assistant-bubble-border': settings.useSolidBubble ? settings.solidAssistantBubbleColor : hexToRgba(settings.assistantBubbleColor, Math.min(1, settings.assistantBubbleAlpha + 0.15)),
  } as React.CSSProperties), [effectiveTextColor, settings.useSolidBubble, settings.solidUserBubbleColor, settings.solidAssistantBubbleColor, settings.userBubbleColor, settings.userBubbleAlpha, settings.assistantBubbleColor, settings.assistantBubbleAlpha, hexToRgba]);

  const isNewSessionRef = useRef(false);

  const bgTransitionActionRef = useRef<(() => void) | null>(null);
  const [bgTransitionActive, setBgTransitionActive] = useState(false);
  const startBgTransition = useCallback((action: () => void) => {
    bgTransitionActionRef.current = action;
    setBgTransitionActive(true);
  }, []);
  const handleBgTransitionMidpoint = useCallback(() => { bgTransitionActionRef.current?.(); bgTransitionActionRef.current = null; }, []);
  const handleBgTransitionComplete = useCallback(() => { setBgTransitionActive(false); }, []);

  // Video effects
  useEffect(() => {
    if (showSplash) return;
    const el = video.videoRef.current;
    if (!el || !bgVideo) return;
    if (!video.videoPlaying) { el.pause(); return; }
    const tryPlay = () => { el.play().catch(() => {}); };
    if (el.readyState >= 3) tryPlay();
    else { el.addEventListener('canplay', tryPlay, { once: true }); return () => el.removeEventListener('canplay', tryPlay); }
  }, [bgVideo, showSplash, video.videoPlaying]);

  useEffect(() => { if (!bgVideo) video.setVideoPlaying(false); }, [bgVideo]);

  useEffect(() => { sessionIdRef.current = currentSessionId; }, [currentSessionId]);

  useEffect(() => { video.setupAutoplayUnlock(bgVideo); }, [bgVideo, video.setupAutoplayUnlock]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (musicPinned) setMusicPinned(false);
        else if (changelogOpen) setChangelogOpen(false);
        else if (settingsOpen) setSettingsOpen(false);
        else if (sidebarOpen) setSidebarOpen(false);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [musicPinned, changelogOpen, settingsOpen, sidebarOpen]);

  const currentSession = currentSessionId ? (sessions.find(s => s.id === currentSessionId) ?? null) : null;

  const handleMessagesChange = useCallback((messages: ChatMessage[]) => {
    const sid = sessionIdRef.current;
    if (sid && messages.length > 0) saveSession(sid, messages);
  }, [saveSession]);

  /* ── Media tool handler ── */
  const handleToolResult = useCallback((event: ToolStatusEvent) => {
    if (event.tool_name !== 'media' || !event.success) return;
    const output = event.output as Record<string, unknown>;
    const cmd = (output?.command as string) ?? (output?.arguments as Record<string, unknown>)?.action as string;
    if (!cmd) return;
    const fileName = output.file_name as string | undefined ?? (output?.arguments as Record<string, unknown>)?.file_name as string | undefined;

    const importB64 = (b64: string, mime: string, ext: string, defaultName: string) => {
      const binary = atob(b64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      const blob = new Blob([bytes] as BlobPart[], { type: mime });
      return { file: new File([blob], `${fileName || defaultName}.${ext}`, { type: mime }), blob };
    };

    switch (cmd) {
      case 'import_and_set_bg_image': {
        const b64 = output.file_data as string;
        const mime = (output.mime_type as string) || 'image/png';
        if (!b64) break;
        const { file, blob } = importB64(b64, mime, 'png', 'image');
        mediaLibrary.importFiles([file]).then(() => {
          setBgImage(URL.createObjectURL(blob));
        });
        break;
      }
      case 'import_and_set_bg_video': {
        const b64 = output.file_data as string;
        const mime = (output.mime_type as string) || 'video/mp4';
        if (!b64) break;
        const { file, blob } = importB64(b64, mime, 'mp4', 'video');
        mediaLibrary.importFiles([file]).then(() => setBgVideo(URL.createObjectURL(blob), blob as File));
        break;
      }
      case 'import_and_play_music': {
        const b64 = output.file_data as string;
        const mime = (output.mime_type as string) || 'audio/mpeg';
        if (!b64) break;
        const { file, blob } = importB64(b64, mime, 'mp3', 'music');
        mediaLibrary.importFiles([file]).then(() => {
          const url = URL.createObjectURL(blob);
          const trackName = (fileName || '未知曲目').replace(/\.[^.]+$/, '');
          handleAddTrack({ url, name: trackName });
          if (!musicPinned) setMusicPinned(true);
        });
        break;
      }
      case 'set_bg_image': {
        if (!fileName) break;
        const found = mediaLibrary.images.find(i => i.name === fileName);
        if (found) mediaLibrary.getUrl(found.id).then(url => { if (url) setBgImage(url); });
        break;
      }
      case 'set_bg_video': {
        if (!fileName) break;
        const found = mediaLibrary.videos.find(v => v.name === fileName);
        if (found) mediaLibrary.getUrl(found.id).then(url => { if (url) setBgVideo(url); });
        break;
      }
      case 'play_music': {
        if (fileName) {
          const found = playlist.find(t => t.name === fileName);
          if (found) { handleSelectTrack(playlist.indexOf(found)); }
          else {
            const libItem = mediaLibrary.music.find(m => m.name === fileName);
            if (libItem) mediaLibrary.getUrl(libItem.id).then(url => { if (url) handleAddTrack({ url, name: libItem.name }); });
          }
        } else if (playlist.length === 0 && mediaLibrary.music.length > 0) {
          const first = mediaLibrary.music[0];
          mediaLibrary.getUrl(first.id).then(url => { if (url) handleAddTrack({ url, name: first.name }); });
        }
        if (!musicPinned) setMusicPinned(true);
        setMusicPlaying(true);
        break;
      }
      case 'pause_music': if (musicPlaying) toggleMusic(); break;
      case 'resume_music': if (!musicPlaying) toggleMusic(); break;
      case 'next_track': handleMusicNext(); break;
      case 'prev_track': handleMusicPrev(); break;
      case 'toggle_mute': toggleMusicMute(); break;
      case 'set_volume': {
        const args = output?.arguments as Record<string, unknown> | undefined;
        const vol = (output.volume as number) ?? (args?.volume as number) ?? 50;
        setMusicVolume(vol / 100);
        break;
      }
      case 'activate_bg_video': activateBgVideo().then(ok => { if (ok) video.setVideoPlaying(true); }); break;
      case 'activate_bg_image': activateBgImage(); break;
      case 'clear_bg': setBgImage(null); setBgVideo(null); break;
      case 'get_status': break;
    }
  }, [mediaLibrary, setBgImage, setBgVideo, activateBgImage, activateBgVideo, handleAddTrack, musicPinned, musicPlaying, toggleMusic, setMusicPlaying, toggleMusicMute, handleMusicNext, handleMusicPrev, setMusicVolume, setMusicPinned, playlist, handleSelectTrack]);

  const { messages, isStreaming, error, toolEvents, agentProgress, companionState, sendMessage: rawSendMessage, stopGeneration, clearMessages } = useChat({
    baseUrl: BASE_URL, session: currentSession, onMessagesChange: handleMessagesChange,
    isNewSessionRef, getSystemInstructions, onToolResult: handleToolResult, companionMode: settings.companionMode,
  });

  const sendMessage = useCallback((content: string, forceResend?: boolean) => {
    if (!sessionIdRef.current) { isNewSessionRef.current = true; const newId = createSession(); sessionIdRef.current = newId; }
    userScrolledRef.current = false;
    rawSendMessage(content, forceResend);
  }, [createSession, rawSendMessage]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const sidebarTouchRef = useRef<{ startX: number; startY: number } | null>(null);
  const userScrolledRef = useRef(false);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onScroll = () => { const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 150; if (atBottom) userScrolledRef.current = false; };
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => { el.removeEventListener('scroll', onScroll); };
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el || userScrolledRef.current) return;
    el.scrollTop = el.scrollHeight;
  }, [messages]);

  const handleNewChat = useCallback(() => {
    isNewSessionRef.current = false;
    const newId = createSession();
    if (activePresetId) setSessionPreset(newId, activePresetId);
    clearMessages();
    setSidebarOpen(false);
  }, [createSession, clearMessages, activePresetId, setSessionPreset]);

  const handleSelectSession = useCallback((session: { id: string }) => {
    setCurrentSessionId(session.id);
    if (session.id in sessionPresets) setActivePresetId(sessionPresets[session.id] || null);
    setSidebarOpen(false);
  }, [setCurrentSessionId, setActivePresetId, sessionPresets]);

  const handleDeleteSession = useCallback((sessionId: string) => { deleteSession(sessionId); }, [deleteSession]);
  const handleDeleteBatch = useCallback((sessionIds: string[]) => { deleteSessions(sessionIds); }, [deleteSessions]);
  const handleSplashComplete = useCallback(() => setShowSplash(false), []);

  const questionIdMap = useMemo(() => {
    const map = new Map<string, string>();
    let lastUserId: string | undefined;
    for (const msg of messages) {
      if (msg.role === 'user') lastUserId = msg.id;
      else if (msg.role === 'assistant' && lastUserId) map.set(msg.id, lastUserId);
    }
    return map;
  }, [messages]);

  return (
    <>
    {showSplash && <ErrorBoundary><Suspense fallback={null}><SplashScreen onComplete={handleSplashComplete} /></Suspense></ErrorBoundary>}
    {bgTransitionActive && <BgTransition onMidpoint={handleBgTransitionMidpoint} onComplete={handleBgTransitionComplete} />}
    <div
      className={`app-root${!bgLoading && (bgImage || bgVideo) ? ' has-custom-bg' : ''}${settings.hideGlass ? ' hide-glass' : ''}${settings.useSolidBubble ? ' solid-bubble' : ''}`}
      data-lg-mask={settings.lgConfig.enabled && settings.lgConfig.mask.enabled ? 'true' : undefined}
      data-lg-card={settings.lgConfig.enabled && settings.lgConfig.card.enabled ? 'true' : undefined}
      data-lg-button={settings.lgConfig.enabled && settings.lgConfig.button.enabled ? 'true' : undefined}
      data-lg-companion={settings.lgConfig.enabled && settings.lgConfig.companionPanel ? 'true' : undefined}
      style={appStyle}
    >
      {!bgLoading && !bgImage && !bgVideo && (
        <div className="default-bg-decor">
          <div className="bg-orb bg-orb-1" /><div className="bg-orb bg-orb-2" /><div className="bg-orb bg-orb-3" /><div className="bg-orb bg-orb-4" /><div className="bg-orb bg-orb-5" />
          <div className="bg-ring bg-ring-1" /><div className="bg-ring bg-ring-2" /><div className="bg-ring bg-ring-3" />
          <div className="bg-sparkle bg-sparkle-1" /><div className="bg-sparkle bg-sparkle-2" /><div className="bg-sparkle bg-sparkle-3" /><div className="bg-sparkle bg-sparkle-4" /><div className="bg-sparkle bg-sparkle-5" /><div className="bg-sparkle bg-sparkle-6" />
          <div className="bg-arc bg-arc-1" /><div className="bg-arc bg-arc-2" />
          <div className="bg-dot-grid" />
        </div>
      )}

      {!bgLoading && bgImage && (
        <div className={`custom-bg-layer${video.videoPlaying ? ' custom-bg-hidden' : ''}`}
          style={{ backgroundImage: `url(${bgImage})`, opacity: bgOpacity, filter: bgBlur > 0 ? `blur(${bgBlur}px)` : undefined }} />
      )}
      {!bgLoading && bgVideo && (
        <video ref={video.videoRef} className={`custom-bg-layer${video.videoPlaying ? '' : ' custom-bg-hidden'}`}
          src={bgVideo} loop muted={video.videoMuted} playsInline
          style={{ opacity: bgOpacity, filter: bgBlur > 0 ? `blur(${bgBlur}px)` : undefined }} />
      )}

      <div style={{ position: 'relative' }}>
        <StatusBar
          baseUrl={BASE_URL} onClear={handleNewChat}
          onToggleSidebar={() => setSidebarOpen(!sidebarOpen)} onOpenSettings={() => setSettingsOpen(true)} onOpenChangelog={() => setChangelogOpen(true)}
          focused={inputFocused} hideGlass={settings.hideGlass} videoPlaying={video.videoPlaying}
          onTogglePlay={() => startBgTransition(async () => {
            if (!bgVideo) { const ok = await activateBgVideo(); if (ok) video.setVideoPlaying(true); }
            else video.setVideoPlaying((v) => !v);
          })}
          videoMuted={bgVideo ? video.videoMuted : undefined} musicMuted={musicMuted}
          onToggleMute={() => { video.autoplayUnlockedRef.current = true; video.setVideoMuted((v) => !v); }}
          onToggleMusicMute={toggleMusicMute}
          presetLocked={messages.length > 0} activePresetId={activePresetId} onSelectPreset={handlePresetChange}
          builtinPresets={builtinPresets} customPresets={customPresets}
          onAddCustomPreset={addCustomPreset} onUpdateCustomPreset={updateCustomPreset} onDeleteCustomPreset={deleteCustomPreset}
          showMusic={!bgLoading && !(bgVideo && video.videoPlaying)} musicPlaying={musicPlaying} onToggleMusicPanel={handleToggleMusicPinned}
        />
        {settings.lgConfig.enabled && settings.lgConfig.mask.enabled && (
          <LgGlassMask config={settings.lgConfig.mask} className="lg-mask-status" active={!inputFocused} />
        )}
      </div>

      <MusicPlayer
        musicPlaying={musicPlaying} musicMuted={musicMuted} musicVolume={musicVolume} musicFile={musicFile}
        playlist={playlist} playlistIndex={playlistIndex}
        onTogglePlay={toggleMusic} onToggleMute={toggleMusicMute} onVolumeChange={setMusicVolume} onFileChange={setMusicFile}
        onNext={handleMusicNext} onPrev={handleMusicPrev} onAddTrack={handleAddTrack} onRemoveTrack={handleRemoveTrack} onSelectTrack={handleSelectTrack}
        expanded={musicExpanded} onExpandedChange={setMusicExpanded} onAnalyserReady={handleAnalyserReady}
        hideGlass={settings.hideGlass} visible={musicPinned && !bgLoading && !(bgVideo && video.videoPlaying)}
        musicLibrary={musicLibraryData}
      />

      <div className={`sidebar ${sidebarOpen ? 'open' : ''}`}
        onTouchStart={(e) => { const t = e.touches[0]; sidebarTouchRef.current = { startX: t.clientX, startY: t.clientY }; }}
        onTouchEnd={(e) => {
          if (!sidebarTouchRef.current) return;
          const t = e.changedTouches[0];
          const dx = t.clientX - sidebarTouchRef.current.startX;
          const dy = Math.abs(t.clientY - sidebarTouchRef.current.startY);
          sidebarTouchRef.current = null;
          if (dx < -80 && dy < 100) setSidebarOpen(false);
        }}>
        <SessionList sessions={sessions} currentSessionId={currentSessionId} onSelect={handleSelectSession}
          onDelete={handleDeleteSession} onDeleteBatch={handleDeleteBatch} onNew={handleNewChat} />
      </div>

      {sidebarOpen && <div className="sidebar-overlay" onClick={() => setSidebarOpen(false)} />}

      {settingsOpen && (
        <ErrorBoundary><Suspense fallback={null}><SettingsPanel
          bgImage={bgImage} bgVideo={bgVideo} bgOpacity={bgOpacity} bgBlur={bgBlur}
          hideGlass={settings.hideGlass} hideWelcomePrompt={settings.hideWelcomePrompt}
          useSolidBubble={settings.useSolidBubble} bubbleTextColor={settings.bubbleTextColor}
          userBubbleColor={settings.userBubbleColor} userBubbleAlpha={settings.userBubbleAlpha}
          assistantBubbleColor={settings.assistantBubbleColor} assistantBubbleAlpha={settings.assistantBubbleAlpha}
          solidUserBubbleColor={settings.solidUserBubbleColor} solidAssistantBubbleColor={settings.solidAssistantBubbleColor}
          autoTextEnabled={settings.autoTextEnabled}
          onImageChange={(img) => startBgTransition(() => setBgImage(img))}
          onVideoChange={(vid, file) => startBgTransition(() => setBgVideo(vid, file))}
          onOpacityChange={setBgOpacity} onBlurChange={setBgBlur}
          onHideGlassChange={settings.setHideGlass} onHideWelcomePromptChange={settings.setHideWelcomePrompt}
          onUseSolidBubbleChange={settings.setUseSolidBubble} onBubbleTextColorChange={settings.setBubbleTextColor}
          onUserBubbleColorChange={settings.setUserBubbleColor} onUserBubbleAlphaChange={settings.setUserBubbleAlpha}
          onAssistantBubbleColorChange={settings.setAssistantBubbleColor} onAssistantBubbleAlphaChange={settings.setAssistantBubbleAlpha}
          onSolidUserBubbleColorChange={settings.setSolidUserBubbleColor} onSolidAssistantBubbleColorChange={settings.setSolidAssistantBubbleColor}
          onAutoTextEnabledChange={settings.setAutoTextEnabled}
          companionMode={settings.companionMode} onCompanionModeChange={settings.handleCompanionModeChange}
          showEmotionPanel={settings.showEmotionPanel} onShowEmotionPanelChange={settings.handleShowEmotionPanelChange}
          onClose={() => setSettingsOpen(false)}
          activeBgType={bgVideo && video.videoPlaying ? 'video' : bgImage ? 'image' : null}
          mediaLibrary={{ images: mediaLibrary.images, videos: mediaLibrary.videos, importFiles: mediaLibrary.importFiles, importFilesByType: mediaLibrary.importFilesByType, importFolder: mediaLibrary.importFolder, remove: mediaLibrary.remove, removeFolder: mediaLibrary.removeFolder, removeAll: mediaLibrary.removeAll, getUrl: mediaLibrary.getUrl }}
          lgConfig={settings.lgConfig} onLgConfigChange={settings.setLgConfig}
        /></Suspense></ErrorBoundary>
      )}

      {changelogOpen && <ErrorBoundary><Suspense fallback={null}><ChangelogPanel onClose={() => setChangelogOpen(false)} /></Suspense></ErrorBoundary>}

      <div className="chat-container">
        <div className="messages-scroll" ref={scrollRef} role="log" aria-live="polite" aria-label="聊天消息">
          <CompanionPanel state={companionState!} visible={settings.companionMode && settings.showEmotionPanel && !!companionState} />
          {messages.length > 0 ? (
            <div className="messages-list">
              {messages.map((msg) => (
                <MessageBubble key={msg.id} message={msg} onResend={sendMessage}
                  questionId={msg.role === 'assistant' ? questionIdMap.get(msg.id) : undefined}
                  agentProgress={msg.isStreaming ? agentProgress : undefined} />
              ))}
            </div>
          ) : (
            <WelcomeScreen onSuggestion={sendMessage} hidePrompt={settings.hideWelcomePrompt} onSelectPreset={handlePresetChange} />
          )}
        </div>

        {error && <div className="error-banner">{error}</div>}

        <div className="input-bar-container" style={{ position: 'relative' }}>
          {settings.lgConfig.enabled && settings.lgConfig.mask.enabled && (
            <LgGlassMask config={settings.lgConfig.mask} className="lg-mask-input" active={!inputFocused} />
          )}
          <InputBar onSend={sendMessage} onStop={stopGeneration} disabled={isStreaming} focused={inputFocused} onFocusChange={setInputFocused} frequencyData={mergedFreq} toolEvents={toolEvents} />
        </div>
      </div>
      {settings.lgConfig.enabled && settings.lgConfig.card.enabled && <LgGlassCard config={settings.lgConfig.card} />}
      {settings.lgConfig.enabled && settings.lgConfig.button.enabled && <LgGlassButton config={settings.lgConfig.button} />}
    </div>

    {importDialog && importDialog.mode === 'conflict' && (
      <ErrorBoundary><Suspense fallback={null}><ImportDialog mode="conflict" info={importDialog.info} onResolve={(choice, remember) => { importDialog.resolve({ choice, remember }); setImportDialog(null); }} /></Suspense></ErrorBoundary>
    )}
    {importDialog && importDialog.mode === 'subfolder' && (
      <ErrorBoundary><Suspense fallback={null}><ImportDialog mode="subfolder" info={importDialog.info} onResolve={(choice, remember) => { importDialog.resolve({ choice, remember }); setImportDialog(null); }} /></Suspense></ErrorBoundary>
    )}
    </>
  );
}

export default App;
