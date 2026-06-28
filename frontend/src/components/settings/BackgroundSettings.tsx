import { useRef, useState, useCallback } from 'react';
import type { MediaItemMeta } from '../../hooks/useMediaLibrary';
import { MediaLibraryPanel } from '../MediaLibraryPanel';
import { getMediaBlob } from '../../utils/mediaLibraryStorage';

interface MediaLibraryData {
  images: MediaItemMeta[];
  videos: MediaItemMeta[];
  importFilesByType: (type: 'image' | 'video', files: FileList | File[], folder?: string) => Promise<number>;
  importFolder: (files: FileList, folder?: string) => Promise<number>;
  remove: (id: string) => Promise<void>;
  removeFolder?: (folder: string, type: 'image' | 'video') => Promise<void>;
  removeAll?: (type: 'image' | 'video') => Promise<void>;
  getUrl: (id: string) => Promise<string | null>;
}

interface Props {
  bgImage: string | null;
  bgVideo: string | null;
  bgOpacity: number;
  bgBlur: number;
  onImageChange: (image: string | null) => void;
  onVideoChange: (video: string | null, file?: File) => void;
  onOpacityChange: (opacity: number) => void;
  onBlurChange: (blur: number) => void;
  activeBgType?: 'image' | 'video' | null;
  mediaLibrary?: MediaLibraryData;
}

export function BackgroundSettings({
  bgImage, bgVideo, bgOpacity, bgBlur,
  onImageChange, onVideoChange, onOpacityChange, onBlurChange,
  activeBgType, mediaLibrary,
}: Props) {
  const imageInputRef = useRef<HTMLInputElement>(null);
  const videoInputRef = useRef<HTMLInputElement>(null);
  const [bgTab, setBgTab] = useState<'image' | 'video'>(() => activeBgType === 'video' ? 'video' : 'image');
  const [bgSource, setBgSource] = useState<'upload' | 'library'>('upload');
  const [dragOver, setDragOver] = useState(false);
  const [libTransitioning, setLibTransitioning] = useState(false);

  const handleImageSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    if (file.size > 100 * 1024 * 1024) { alert('图片大小不能超过 100MB'); return; }
    const reader = new FileReader();
    reader.onload = (event) => { onImageChange(event.target?.result as string); };
    reader.readAsDataURL(file);
  };

  const handleVideoSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    if (file.size > 1024 * 1024 * 1024) { alert('视频大小不能超过 1GB'); return; }
    onVideoChange(URL.createObjectURL(file), file);
  };

  const handleRemoveImage = () => {
    onImageChange(null);
    if (imageInputRef.current) imageInputRef.current.value = '';
  };

  const handleRemoveVideo = () => {
    if (bgVideo && bgVideo.startsWith('blob:')) URL.revokeObjectURL(bgVideo);
    onVideoChange(null);
    if (videoInputRef.current) videoInputRef.current.value = '';
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    const file = e.dataTransfer.files[0];
    if (!file) return;
    if (bgTab === 'image' && file.type.startsWith('image/')) {
      if (file.size > 100 * 1024 * 1024) { alert('图片大小不能超过 100MB'); return; }
      const reader = new FileReader();
      reader.onload = (event) => onImageChange(event.target?.result as string);
      reader.readAsDataURL(file);
    } else if (bgTab === 'video' && file.type.startsWith('video/')) {
      if (file.size > 1024 * 1024 * 1024) { alert('视频大小不能超过 1GB'); return; }
      onVideoChange(URL.createObjectURL(file), file);
    }
  };

  const handleLibrarySelect = useCallback(async (id: string) => {
    if (!mediaLibrary) return;
    if (bgTab === 'image') {
      const url = await mediaLibrary.getUrl(id);
      if (!url) return;
      onImageChange(url);
    } else {
      const blob = await getMediaBlob(id);
      if (!blob) return;
      onVideoChange(URL.createObjectURL(blob), blob as File);
    }
    setBgSource('upload');
  }, [mediaLibrary, bgTab, onImageChange, onVideoChange]);

  const handleLibraryRemove = useCallback(async (id: string) => {
    if (!mediaLibrary) return;
    await mediaLibrary.remove(id);
  }, [mediaLibrary]);

  const switchTab = (tab: 'image' | 'video') => {
    if (bgTab === tab) return;
    if (bgSource === 'library') {
      setLibTransitioning(true);
      setTimeout(() => { setBgTab(tab); setLibTransitioning(false); }, 320);
    } else {
      setBgTab(tab);
    }
  };

  return (
    <div className="settings-section">
      <div className="bg-section-header">
        <label className="settings-label">背景</label>
        <div className="bg-tab-bar">
          <div className="bg-tab-indicator" style={{ transform: `translateX(${bgTab === 'video' ? '100%' : '0'})` }} />
          <button className={`bg-tab${bgTab === 'image' ? ' active' : ''}`} onClick={() => switchTab('image')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <rect x="3" y="3" width="18" height="18" rx="2" ry="2" /><circle cx="8.5" cy="8.5" r="1.5" /><polyline points="21 15 16 10 5 21" />
            </svg>
            图片
          </button>
          <button className={`bg-tab${bgTab === 'video' ? ' active' : ''}`} onClick={() => switchTab('video')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polygon points="23 7 16 12 23 17 23 7" /><rect x="1" y="5" width="15" height="14" rx="2" ry="2" />
            </svg>
            视频
          </button>
        </div>
      </div>

      {mediaLibrary && (
        <div className="bg-source-toggle">
          <div className="bg-source-indicator" style={{ transform: `translateX(${bgSource === 'library' ? '100%' : '0'})` }} />
          <button className={`bg-source-btn${bgSource === 'upload' ? ' active' : ''}`} onClick={() => setBgSource('upload')}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" />
            </svg>
            上传
          </button>
          <button className={`bg-source-btn${bgSource === 'library' ? ' active' : ''}`} onClick={() => setBgSource('library')}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <rect x="3" y="3" width="7" height="7" /><rect x="14" y="3" width="7" height="7" /><rect x="14" y="14" width="7" height="7" /><rect x="3" y="14" width="7" height="7" />
            </svg>
            素材库
          </button>
        </div>
      )}

      {mediaLibrary && (
        <div style={{
          position: bgSource === 'library' && !libTransitioning ? 'relative' : 'absolute',
          visibility: bgSource === 'library' && !libTransitioning ? 'visible' : 'hidden',
          height: bgSource === 'library' && !libTransitioning ? 'auto' : 0,
          overflow: bgSource === 'library' && !libTransitioning ? 'visible' : 'hidden',
          pointerEvents: bgSource === 'library' && !libTransitioning ? 'auto' : 'none',
          opacity: bgSource === 'library' && !libTransitioning ? 1 : 0,
          transition: 'opacity 0.25s ease',
        }}>
          <div className="settings-collapse-inner">
            <MediaLibraryPanel
              type={bgTab === 'video' ? 'video' : 'image'}
              items={bgTab === 'video' ? mediaLibrary.videos : mediaLibrary.images}
              selectedId={null}
              onSelect={handleLibrarySelect}
              onRemove={handleLibraryRemove}
              onRemoveFolder={mediaLibrary.removeFolder ? (folder) => mediaLibrary.removeFolder!(folder, bgTab) : undefined}
              onRemoveAll={mediaLibrary.removeAll ? () => mediaLibrary.removeAll!(bgTab) : undefined}
              onImportFiles={(files, folder) => mediaLibrary.importFilesByType(bgTab, files, folder)}
              onImportFolder={mediaLibrary.importFolder}
              getUrl={mediaLibrary.getUrl}
            />
          </div>
        </div>
      )}

      {/* Image upload */}
      <div className={`settings-collapse-body${bgSource !== 'library' && bgTab === 'image' ? ' open' : ''}`}>
        <div className="settings-collapse-inner">
          <input ref={imageInputRef} type="file" accept="image/*" onChange={handleImageSelect} className="settings-file-input" id="settings-image-input" />
          {bgImage ? (
            <div className="bg-preview-card">
              <img src={bgImage} alt="背景预览" className="bg-preview-media" />
              <div className="bg-preview-overlay">
                <label htmlFor="settings-image-input" className="bg-preview-action">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" />
                  </svg>
                  更换
                </label>
                <button className="bg-preview-action bg-preview-remove" onClick={handleRemoveImage}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                  </svg>
                  移除
                </button>
              </div>
            </div>
          ) : (
            <label htmlFor="settings-image-input" className={`bg-dropzone${dragOver ? ' drag-over' : ''}`}
              onDragOver={(e) => { e.preventDefault(); setDragOver(true); }} onDragLeave={() => setDragOver(false)} onDrop={handleDrop}>
              <div className="bg-dropzone-icon">
                <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" /><circle cx="8.5" cy="8.5" r="1.5" /><polyline points="21 15 16 10 5 21" />
                </svg>
              </div>
              <span className="bg-dropzone-text">拖放或点击选择图片</span>
              <span className="bg-dropzone-hint">支持 JPG / PNG / WebP，最大 100MB</span>
            </label>
          )}
        </div>
      </div>

      {/* Video upload */}
      <div className={`settings-collapse-body${bgSource !== 'library' && bgTab === 'video' ? ' open' : ''}`}>
        <div className="settings-collapse-inner">
          <input ref={videoInputRef} type="file" accept="video/mp4,video/webm,video/ogg" onChange={handleVideoSelect} className="settings-file-input" id="settings-video-input" />
          {bgVideo ? (
            <div className="bg-preview-card">
              <video src={bgVideo} muted loop autoPlay playsInline className="bg-preview-media" />
              <div className="bg-preview-overlay">
                <label htmlFor="settings-video-input" className="bg-preview-action">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" />
                  </svg>
                  更换
                </label>
                <button className="bg-preview-action bg-preview-remove" onClick={handleRemoveVideo}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                  </svg>
                  移除
                </button>
              </div>
            </div>
          ) : (
            <label htmlFor="settings-video-input" className={`bg-dropzone${dragOver ? ' drag-over' : ''}`}
              onDragOver={(e) => { e.preventDefault(); setDragOver(true); }} onDragLeave={() => setDragOver(false)} onDrop={handleDrop}>
              <div className="bg-dropzone-icon">
                <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <polygon points="23 7 16 12 23 17 23 7" /><rect x="1" y="5" width="15" height="14" rx="2" ry="2" />
                </svg>
              </div>
              <span className="bg-dropzone-text">拖放或点击选择视频</span>
              <span className="bg-dropzone-hint">支持 MP4 / WebM / OGG，最大 1GB</span>
            </label>
          )}
        </div>
      </div>

      <div className="settings-divider" />

      {/* Opacity */}
      <div className="settings-section">
        <div className="settings-slider-header">
          <label className="settings-label">不透明度</label>
          <span className="settings-slider-value">{Math.round(bgOpacity * 100)}%</span>
        </div>
        <div className="settings-slider-track">
          <input type="range" min="0.05" max="1" step="0.01" value={bgOpacity}
            onChange={(e) => onOpacityChange(parseFloat(e.target.value))} className="settings-slider"
            style={{ '--fill': `${bgOpacity * 100}%` } as React.CSSProperties} />
        </div>
      </div>

      {/* Blur */}
      <div className="settings-section">
        <div className="settings-slider-header">
          <label className="settings-label">模糊</label>
          <span className="settings-slider-value">{bgBlur.toFixed(1)}px</span>
        </div>
        <div className="settings-slider-track">
          <input type="range" min="0" max="20" step="0.1" value={bgBlur}
            onChange={(e) => onBlurChange(parseFloat(e.target.value))} className="settings-slider"
            style={{ '--fill': `${(bgBlur / 20) * 100}%` } as React.CSSProperties} />
        </div>
      </div>
    </div>
  );
}
