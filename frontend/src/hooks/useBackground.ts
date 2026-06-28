import { useState, useCallback, useEffect, useRef } from 'react';
import {
  loadBgImage, saveBgImage, removeBgImage,
  loadBgVideo, saveBgVideo, removeBgVideo,
} from '../utils/bgStorage';

const BG_OPACITY_KEY = 'agent-teams-bg-opacity';
const BG_BLUR_KEY = 'agent-teams-bg-blur';

function loadNumber(key: string, fallback: number): number {
  try {
    const val = localStorage.getItem(key);
    return val ? parseFloat(val) : fallback;
  } catch {
    return fallback;
  }
}

export interface UseBackgroundReturn {
  bgImage: string | null;
  bgVideo: string | null;
  bgOpacity: number;
  bgBlur: number;
  bgLoading: boolean;
  setBgImage: (image: string | null) => void;
  setBgVideo: (video: string | null, file?: File) => void;
  activateBgImage: () => Promise<void>;
  activateBgVideo: () => Promise<boolean>;
  setBgOpacity: (opacity: number) => void;
  setBgBlur: (blur: number) => void;
}

/**
 * Create a minimal static gradient image blob as a fallback "video".
 * Used when no video is stored in IndexedDB — gives the user visual feedback
 * that the background feature works, prompting them to import a real video.
 */
async function createDefaultVideoBlob(): Promise<Blob | null> {
  try {
    const canvas = document.createElement('canvas');
    canvas.width = 640;
    canvas.height = 360;
    const ctx = canvas.getContext('2d');
    if (!ctx) return null;

    const grad = ctx.createLinearGradient(0, 0, canvas.width, canvas.height);
    grad.addColorStop(0, '#1a1a2e');
    grad.addColorStop(0.5, '#16213e');
    grad.addColorStop(1, '#0f3460');
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    // Add subtle text
    ctx.fillStyle = 'rgba(255,255,255,0.15)';
    ctx.font = '20px sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('自定义视频背景', canvas.width / 2, canvas.height / 2);

    return new Promise((resolve) => {
      canvas.toBlob(b => resolve(b), 'image/png');
    });
  } catch {
    return null;
  }
}

export function useBackground(): UseBackgroundReturn {
  const [bgImage, setBgImageState] = useState<string | null>(null);
  const [bgVideo, setBgVideoState] = useState<string | null>(null);
  const [bgLoading, setBgLoading] = useState(true);
  const [bgOpacity, setBgOpacityState] = useState(() => loadNumber(BG_OPACITY_KEY, 0.3));
  const [bgBlur, setBgBlurState] = useState(() => loadNumber(BG_BLUR_KEY, 0));

  useEffect(() => {
    let revoked = false;
    Promise.all([loadBgImage(), loadBgVideo()]).then(([img, vid]) => {
      if (revoked) return;
      if (img) setBgImageState(img);
      if (vid) setBgVideoState(vid);
      setBgLoading(false);
    });
    return () => { revoked = true; };
  }, []);

  // Cleanup object URLs on change or unmount to prevent memory leaks
  const prevBlobUrlRef = useRef<string | null>(null);
  useEffect(() => {
    const prev = prevBlobUrlRef.current;
    prevBlobUrlRef.current = bgVideo;
    return () => {
      if (prev && prev.startsWith('blob:')) {
        URL.revokeObjectURL(prev);
      }
    };
  }, [bgVideo]);

  const setBgImage = useCallback((image: string | null) => {
    setBgImageState(image);
    if (image) {
      // Blob URLs are ephemeral — fetch the actual blob and store it in IndexedDB
      // so it survives page refreshes
      if (image.startsWith('blob:')) {
        fetch(image)
          .then(r => r.blob())
          .then(blob => saveBgImage(blob))
          .catch(console.error);
      } else {
        // data: URLs or http: URLs — store directly
        saveBgImage(image).catch(console.error);
      }
    } else {
      removeBgImage().catch(console.error);
    }
  }, []);

  const setBgVideo = useCallback((video: string | null, file?: File) => {
    setBgVideoState(video);
    if (video && file) {
      // File (Blob) provided → save to IndexedDB for persistence
      saveBgVideo(file).catch(console.error);
    } else if (!video) {
      // Null → remove from IndexedDB
      removeBgVideo().catch(console.error);
    }
    // URL-only (no file) → just set state, don't save temporary URL to IndexedDB
  }, []);

  const activateBgImage = useCallback(async () => {
    const url = await loadBgImage();
    if (url) setBgImageState(url);
  }, []);

  const activateBgVideo = useCallback(async (): Promise<boolean> => {
    const url = await loadBgVideo();
    if (url) {
      setBgVideoState(url);
      return true;
    }
    // No stored video — show a fallback gradient image so the user sees something
    try {
      const blob = await createDefaultVideoBlob();
      if (blob) {
        const imgUrl = URL.createObjectURL(blob);
        setBgImageState(imgUrl);
        // Save the actual blob for persistence across refreshes
        await saveBgImage(blob);
        return true;
      }
    } catch (e) {
      console.warn('Failed to create fallback background:', e);
    }
    return false;
  }, []);

  const setBgOpacity = useCallback((opacity: number) => {
    setBgOpacityState(opacity);
    localStorage.setItem(BG_OPACITY_KEY, opacity.toString());
  }, []);

  const setBgBlur = useCallback((blur: number) => {
    setBgBlurState(blur);
    localStorage.setItem(BG_BLUR_KEY, blur.toString());
  }, []);

  return { bgImage, bgVideo, bgOpacity, bgBlur, bgLoading, setBgImage, setBgVideo, activateBgImage, activateBgVideo, setBgOpacity, setBgBlur };
}
