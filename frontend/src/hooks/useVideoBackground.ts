import { useState, useRef, useEffect, useCallback } from 'react';

export function useVideoBackground() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [videoMuted, setVideoMuted] = useState(() => {
    try { return localStorage.getItem('agent-teams-video-muted') !== 'false'; } catch { return true; }
  });
  const [videoPlaying, setVideoPlaying] = useState(false);

  // Persist muted state
  useEffect(() => {
    localStorage.setItem('agent-teams-video-muted', String(videoMuted));
  }, [videoMuted]);

  // Unmute on first user interaction (browser autoplay policy)
  const autoplayUnlockedRef = useRef(false);
  const setupAutoplayUnlock = useCallback((bgVideo: string | null) => {
    if (!bgVideo || !videoMuted || autoplayUnlockedRef.current) return;
    const unlock = (e: Event) => {
      const target = e.target as HTMLElement;
      if (target.closest('.btn-icon')) return;
      autoplayUnlockedRef.current = true;
      setVideoMuted(false);
      window.removeEventListener('click', unlock);
      window.removeEventListener('keydown', unlock);
    };
    window.addEventListener('click', unlock);
    window.addEventListener('keydown', unlock);
    return () => {
      window.removeEventListener('click', unlock);
      window.removeEventListener('keydown', unlock);
    };
  }, [videoMuted]);

  return {
    videoRef,
    videoMuted,
    setVideoMuted,
    videoPlaying,
    setVideoPlaying,
    autoplayUnlockedRef,
    setupAutoplayUnlock,
  };
}
