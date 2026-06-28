import { useCallback } from 'react';
import type { ToolStatusEvent } from '../api/types';
import type { MusicTrack } from '../components/MusicPlayer';
import type { MediaItemMeta } from './useMediaLibrary';

interface MediaLibrary {
  music: MediaItemMeta[];
  images: MediaItemMeta[];
  videos: MediaItemMeta[];
  importFiles: (files: FileList | File[]) => Promise<number>;
  getUrl: (id: string) => Promise<string | null>;
  findByName: (type: 'music' | 'image' | 'video', name: string) => MediaItemMeta | undefined;
}

interface Deps {
  mediaLibrary: MediaLibrary;
  musicPlaying: boolean;
  setBgImage: (url: string | null) => void;
  setBgVideo: (url: string | null, file?: File) => void;
  activateBgImage: () => void;
  activateBgVideo: () => Promise<boolean>;
  handleAddTrack: (track: MusicTrack) => void;
  setMusicPinned: (v: boolean | ((prev: boolean) => boolean)) => void;
  setMusicPlaying: (v: boolean) => void;
  setVideoPlaying: (v: boolean | ((prev: boolean) => boolean)) => void;
  toggleMusic: () => void;
  toggleMusicMute: () => void;
  handleMusicNext: () => void;
  handleMusicPrev: () => void;
  setMusicVolume: (v: number) => void;
  setMusicFile: (file: string | null) => void;
  playlist: MusicTrack[];
  setPlaylistIndex: (v: number) => void;
}

function b64ToBlob(b64: string, mime: string): Blob {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new Blob([bytes] as BlobPart[], { type: mime });
}

export function useMediaToolHandler(deps: Deps) {
  return useCallback((event: ToolStatusEvent) => {
    if (event.tool_name !== 'media' || !event.success) return;
    const output = event.output as Record<string, unknown>;
    const cmd = (output?.command as string)
      ?? (output?.arguments as Record<string, unknown>)?.action as string;
    if (!cmd) return;

    const fileName = output.file_name as string | undefined
      ?? (output?.arguments as Record<string, unknown>)?.file_name as string | undefined;

    const { mediaLibrary, setBgImage, setBgVideo, activateBgImage, activateBgVideo,
      handleAddTrack, setMusicPinned, setMusicPlaying, setVideoPlaying, toggleMusic, toggleMusicMute,
      handleMusicNext, handleMusicPrev, setMusicVolume, setMusicFile, playlist, setPlaylistIndex } = deps;

    switch (cmd) {
      case 'import_and_set_bg_image': {
        const b64 = output.file_data as string;
        const mime = (output.mime_type as string) || 'image/png';
        if (!b64) break;
        const blob = b64ToBlob(b64, mime);
        const file = new File([blob], `${fileName || 'image'}.${mime.split('/')[1] || 'png'}`, { type: mime });
        mediaLibrary.importFiles([file]).then(() => setBgImage(URL.createObjectURL(blob)));
        break;
      }
      case 'import_and_set_bg_video': {
        const b64 = output.file_data as string;
        const mime = (output.mime_type as string) || 'video/mp4';
        if (!b64) break;
        const blob = b64ToBlob(b64, mime);
        const file = new File([blob], `${fileName || 'video'}.${mime.split('/')[1] || 'mp4'}`, { type: mime });
        mediaLibrary.importFiles([file]).then(() => setBgVideo(URL.createObjectURL(blob), blob as File));
        break;
      }
      case 'import_and_play_music': {
        const b64 = output.file_data as string;
        const mime = (output.mime_type as string) || 'audio/mpeg';
        if (!b64) break;
        const blob = b64ToBlob(b64, mime);
        const file = new File([blob], `${fileName || 'music'}.${mime.split('/')[1] || 'mp3'}`, { type: mime });
        mediaLibrary.importFiles([file]).then(() => {
          const url = URL.createObjectURL(blob);
          const trackName = (fileName || '未知曲目').replace(/\.[^.]+$/, '');
          handleAddTrack({ url, name: trackName });
          setMusicPinned(true);
        });
        break;
      }
      case 'set_bg_image': {
        if (!fileName) break;
        const found = mediaLibrary.findByName('image', fileName);
        if (found) mediaLibrary.getUrl(found.id).then(url => { if (url) setBgImage(url); });
        break;
      }
      case 'set_bg_video': {
        if (!fileName) break;
        const found = mediaLibrary.findByName('video', fileName);
        if (found) mediaLibrary.getUrl(found.id).then(url => { if (url) setBgVideo(url); });
        break;
      }
      case 'play_music': {
        if (fileName) {
          const found = playlist.find(t => t.name === fileName);
          if (found) { setPlaylistIndex(playlist.indexOf(found)); setMusicFile(found.url); }
          else {
            const libItem = mediaLibrary.findByName('music', fileName);
            if (libItem) mediaLibrary.getUrl(libItem.id).then(url => { if (url) handleAddTrack({ url, name: libItem.name }); });
          }
        } else if (playlist.length === 0 && mediaLibrary.music.length > 0) {
          const first = mediaLibrary.music[0];
          mediaLibrary.getUrl(first.id).then(url => { if (url) handleAddTrack({ url, name: first.name }); });
        }
        setMusicPinned(true);
        setMusicPlaying(true);
        break;
      }
      case 'pause_music': if (deps.musicPlaying) toggleMusic(); break;
      case 'resume_music': if (!deps.musicPlaying) toggleMusic(); break;
      case 'next_track': handleMusicNext(); break;
      case 'prev_track': handleMusicPrev(); break;
      case 'toggle_mute': toggleMusicMute(); break;
      case 'set_volume': {
        const args = output?.arguments as Record<string, unknown> | undefined;
        const vol = (output.volume as number) ?? (args?.volume as number) ?? 50;
        setMusicVolume(vol / 100);
        break;
      }
      case 'activate_bg_video': activateBgVideo().then(ok => { if (ok) setVideoPlaying(true); }); break;
      case 'activate_bg_image': activateBgImage(); break;
      case 'clear_bg': setBgImage(null); setBgVideo(null); break;
      case 'get_status': break;
    }
  }, [deps]);
}
