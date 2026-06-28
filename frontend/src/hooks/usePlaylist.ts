import { useState, useCallback } from 'react';
import type { MusicTrack } from '../components/MusicPlayer';

export function usePlaylist(setMusicFile: (file: string | null) => void) {
  const [playlist, setPlaylist] = useState<MusicTrack[]>([]);
  const [playlistIndex, setPlaylistIndex] = useState(0);

  const handleAddTrack = useCallback((track: MusicTrack) => {
    setPlaylist(prev => {
      const existingIndex = prev.findIndex(t => t.name === track.name);
      if (existingIndex >= 0) {
        const updated = [...prev];
        updated[existingIndex] = { ...updated[existingIndex], url: track.url };
        setPlaylistIndex(existingIndex);
        setMusicFile(track.url);
        return updated;
      }
      const next = [...prev, track];
      setPlaylistIndex(next.length - 1);
      setMusicFile(track.url);
      return next;
    });
  }, [setMusicFile]);

  const handleRemoveTrack = useCallback((index: number) => {
    setPlaylist(prev => {
      const next = prev.filter((_, i) => i !== index);
      if (next.length === 0) {
        setMusicFile(null);
        setPlaylistIndex(0);
      } else if (index === playlistIndex) {
        const newIdx = Math.min(index, next.length - 1);
        setPlaylistIndex(newIdx);
        setMusicFile(next[newIdx].url);
      } else if (index < playlistIndex) {
        setPlaylistIndex(i => i - 1);
      }
      return next;
    });
  }, [playlistIndex, setMusicFile]);

  const handleSelectTrack = useCallback((index: number) => {
    if (index >= 0 && index < playlist.length) {
      setPlaylistIndex(index);
      setMusicFile(playlist[index].url);
    }
  }, [playlist, setMusicFile]);

  const handleMusicNext = useCallback(() => {
    if (playlist.length <= 1) return;
    const newIndex = (playlistIndex + 1) % playlist.length;
    setPlaylistIndex(newIndex);
    setMusicFile(playlist[newIndex].url);
  }, [playlist, playlistIndex, setMusicFile]);

  const handleMusicPrev = useCallback(() => {
    if (playlist.length <= 1) return;
    const newIndex = (playlistIndex - 1 + playlist.length) % playlist.length;
    setPlaylistIndex(newIndex);
    setMusicFile(playlist[newIndex].url);
  }, [playlist, playlistIndex, setMusicFile]);

  return {
    playlist,
    playlistIndex,
    handleAddTrack,
    handleRemoveTrack,
    handleSelectTrack,
    handleMusicNext,
    handleMusicPrev,
  };
}
