import { useState, useCallback, useEffect } from 'react';
import { loadBool, loadNumber } from '../utils/storage';

const MUSIC_VOLUME_KEY = 'agent-teams-music-volume';
const MUSIC_MUTED_KEY = 'agent-teams-music-muted';
const MUSIC_FILE_KEY = 'agent-teams-music-file';

export interface UseMusicReturn {
  musicPlaying: boolean;
  musicMuted: boolean;
  musicVolume: number;
  musicFile: string | null;
  setMusicPlaying: (playing: boolean) => void;
  setMusicMuted: (muted: boolean) => void;
  setMusicVolume: (volume: number) => void;
  setMusicFile: (file: string | null) => void;
  toggleMusic: () => void;
  toggleMute: () => void;
}

export function useMusic(): UseMusicReturn {
  const [musicPlaying, setMusicPlaying] = useState(false);
  const [musicMuted, setMusicMuted] = useState(() => loadBool(MUSIC_MUTED_KEY, false));
  const [musicVolume, setMusicVolumeState] = useState(() => loadNumber(MUSIC_VOLUME_KEY, 0.5));
  const [musicFile, setMusicFileState] = useState<string | null>(() => {
    try {
      const val = localStorage.getItem(MUSIC_FILE_KEY);
      // Blob URLs are ephemeral and invalid after page reload — discard them
      if (val && val.startsWith('blob:')) {
        localStorage.removeItem(MUSIC_FILE_KEY);
        return null;
      }
      return val;
    } catch {
      return null;
    }
  });

  const toggleMusic = useCallback(() => {
    setMusicPlaying(prev => !prev);
  }, []);

  const toggleMute = useCallback(() => {
    setMusicMuted(prev => !prev);
  }, []);

  const setMusicVolume = useCallback((volume: number) => {
    setMusicVolumeState(volume);
    localStorage.setItem(MUSIC_VOLUME_KEY, volume.toString());
  }, []);

  const setMusicFile = useCallback((file: string | null) => {
    setMusicFileState(file);
    if (file) {
      localStorage.setItem(MUSIC_FILE_KEY, file);
    } else {
      localStorage.removeItem(MUSIC_FILE_KEY);
    }
  }, []);

  // Persist muted state to localStorage
  useEffect(() => {
    localStorage.setItem(MUSIC_MUTED_KEY, String(musicMuted));
  }, [musicMuted]);

  return {
    musicPlaying,
    musicMuted,
    musicVolume,
    musicFile,
    setMusicPlaying,
    setMusicMuted,
    setMusicVolume,
    setMusicFile,
    toggleMusic,
    toggleMute,
  };
}