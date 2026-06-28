import { useState, useRef, useCallback, useEffect, memo } from 'react';
import type { MediaItemMeta } from '../hooks/useMediaLibrary';
import { getMediaObjectURL, getMediaBlob } from '../utils/mediaLibraryStorage';

interface Props {
  type: 'image' | 'video' | 'music';
  items: MediaItemMeta[];
  selectedId?: string | null;
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
  onRemoveFolder?: (folder: string) => void;
  onRemoveAll?: () => void;
  onImportFiles: (files: FileList | File[], folder?: string) => Promise<number>;
  onImportFolder: (files: FileList, folder?: string) => Promise<number>;
  getUrl: (id: string) => Promise<string | null>;
}

// Collapsed folders storage key
const COLLAPSED_FOLDERS_KEY = 'agent-teams-collapsed-folders';

function getCollapsedFolders(type: string): Set<string> {
  try {
    const stored = localStorage.getItem(`${COLLAPSED_FOLDERS_KEY}-${type}`);
    return stored ? new Set(JSON.parse(stored)) : new Set();
  } catch {
    return new Set();
  }
}

function saveCollapsedFolders(type: string, collapsed: Set<string>) {
  try {
    localStorage.setItem(`${COLLAPSED_FOLDERS_KEY}-${type}`, JSON.stringify([...collapsed]));
  } catch { /* noop */ }
}


const TYPE_LABELS = {
  image: { label: '图片', accept: 'image/*', icon: 'image' as const },
  video: { label: '视频', accept: 'video/mp4,video/webm,video/ogg', icon: 'video' as const },
  music: { label: '音乐', accept: 'audio/*', icon: 'music' as const },
};

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

export const MediaLibraryPanel = memo(function MediaLibraryPanel({
  type, items, selectedId, onSelect, onRemove, onRemoveFolder, onRemoveAll, onImportFiles, onImportFolder, getUrl,
}: Props) {
  const [importing, setImporting] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [collapsedFolders, setCollapsedFolders] = useState<Set<string>>(() => getCollapsedFolders(type));
  const fileRef = useRef<HTMLInputElement>(null);
  const folderRef = useRef<HTMLInputElement>(null);

  const { label, accept, icon } = TYPE_LABELS[type];

  const toggleFolder = useCallback((folderName: string) => {
    setCollapsedFolders(prev => {
      const next = new Set(prev);
      if (next.has(folderName)) {
        next.delete(folderName);
      } else {
        next.add(folderName);
      }
      saveCollapsedFolders(type, next);
      return next;
    });
  }, [type]);

  const handleFileSelect = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files?.length) return;
    setImporting(true);
    try {
      await onImportFiles(files);
    } finally {
      setImporting(false);
      if (e.target) e.target.value = '';
    }
  }, [onImportFiles]);

  const handleFolderSelect = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files?.length) return;
    setImporting(true);
    try {
      await onImportFolder(files);
    } finally {
      setImporting(false);
      if (e.target) e.target.value = '';
    }
  }, [onImportFolder]);

  const handleDrop = useCallback(async (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    const files = e.dataTransfer.files;
    if (!files.length) return;
    setImporting(true);
    try {
      await onImportFiles(files);
    } finally {
      setImporting(false);
    }
  }, [onImportFiles]);

  const handleItemClick = useCallback(async (item: MediaItemMeta) => {
    onSelect(item.id);
  }, [onSelect]);

  const handleRemove = useCallback((e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    onRemove(id);
  }, [onRemove]);

  // Group by folder
  const grouped = new Map<string, MediaItemMeta[]>();
  for (const item of items) {
    const folder = item.folder || '未分类';
    if (!grouped.has(folder)) grouped.set(folder, []);
    grouped.get(folder)!.push(item);
  }
  const sortedFolders = Array.from(grouped.entries()).sort((a, b) => a[0].localeCompare(b[0]));

  return (
    <div
      className={`media-lib-panel${dragOver ? ' drag-over' : ''}`}
      onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
      onDragLeave={() => setDragOver(false)}
      onDrop={handleDrop}
    >
      {/* Action bar */}
      <div className="media-lib-actions">
        <button className="media-lib-btn" onClick={() => fileRef.current?.click()} disabled={importing}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="17 8 12 3 7 8" />
            <line x1="12" y1="3" x2="12" y2="15" />
          </svg>
          导入{label}
        </button>
        <button className="media-lib-btn" onClick={() => folderRef.current?.click()} disabled={importing}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
            <line x1="12" y1="11" x2="12" y2="17" />
            <line x1="9" y1="14" x2="15" y2="14" />
          </svg>
          导入文件夹
        </button>
        {items.length > 0 && (
          <>
            <span className="media-lib-count">{items.length} 项</span>
            {onRemoveAll && (
              <button className="media-lib-btn media-lib-btn-danger" onClick={onRemoveAll} title="清空所有" aria-label="清空所有">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                </svg>
                清空
              </button>
            )}
          </>
        )}
      </div>

      <input ref={fileRef} type="file" accept={accept} multiple onChange={handleFileSelect} style={{ display: 'none' }} />
      <input ref={folderRef} type="file" multiple {...{ webkitdirectory: '' } as any} onChange={handleFolderSelect} style={{ display: 'none' }} />

      {/* Content */}
      {items.length === 0 ? (
        <div className="media-lib-empty">
          <div className="media-lib-empty-icon">
            {icon === 'image' && (
              <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                <circle cx="8.5" cy="8.5" r="1.5" />
                <polyline points="21 15 16 10 5 21" />
              </svg>
            )}
            {icon === 'video' && (
              <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="23 7 16 12 23 17 23 7" />
                <rect x="1" y="5" width="15" height="14" rx="2" ry="2" />
              </svg>
            )}
            {icon === 'music' && (
              <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M9 18V5l12-2v13" />
                <circle cx="6" cy="18" r="3" />
                <circle cx="18" cy="16" r="3" />
              </svg>
            )}
          </div>
          <span className="media-lib-empty-text">还没有{label}文件</span>
          <span className="media-lib-empty-hint">点击上方按钮或拖放文件到此处</span>
        </div>
      ) : (
        <div className="media-lib-content">
          {sortedFolders.map(([folderName, folderItems]) => {
            const isCollapsed = collapsedFolders.has(folderName);
            return (
              <div key={folderName} className={`media-lib-folder${isCollapsed ? ' collapsed' : ''}`}>
                {sortedFolders.length > 1 && (
                  <div className="media-lib-folder-header" onClick={() => toggleFolder(folderName)}>
                    <svg className={`media-lib-folder-arrow${isCollapsed ? '' : ' expanded'}`} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="9 18 15 12 9 6" />
                    </svg>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                    </svg>
                    <span>{folderName}</span>
                    <span className="media-lib-folder-count">{folderItems.length}</span>
                    {onRemoveFolder && (
                      <button className="media-lib-folder-del" onClick={(e) => { e.stopPropagation(); onRemoveFolder(folderName); }} title={`删除「${folderName}」`} aria-label={`删除「${folderName}」`}>
                        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                          <polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                        </svg>
                      </button>
                    )}
                  </div>
                )}
                <div className={`media-lib-folder-body${isCollapsed ? ' collapsed' : ''}`}>
                  <div className="media-lib-folder-inner">
                    <div className={type === 'music' ? 'media-lib-list' : 'media-lib-grid'}>
                      {folderItems.map((item) => (
                        <MediaItemCard
                          key={item.id}
                          item={item}
                          selected={item.id === selectedId}
                          type={type}
                          onClick={() => handleItemClick(item)}
                          onRemove={(e) => handleRemove(e, item.id)}
                          getUrl={getUrl}
                        />
                      ))}
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Drag overlay */}
      {dragOver && (
        <div className="media-lib-drag-overlay">
          <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="17 8 12 3 7 8" />
            <line x1="12" y1="3" x2="12" y2="15" />
          </svg>
          <span>拖放{label}文件到此处</span>
        </div>
      )}
    </div>
  );
});

interface CardProps {
  item: MediaItemMeta;
  selected: boolean;
  type: 'image' | 'video' | 'music';
  onClick: () => void;
  onRemove: (e: React.MouseEvent) => void;
  getUrl: (id: string) => Promise<string | null>;
}

// Music item row component (compact list style)
function MusicItemRow({ item, selected, onClick, onRemove }: Omit<CardProps, 'type' | 'getUrl'>) {
  return (
    <div
      className={`media-lib-row${selected ? ' selected' : ''}`}
      onClick={onClick}
      title={`${item.name}\n${fmtSize(item.size)}`}
    >
      <div className="media-lib-row-info">
        <span className="media-lib-row-name">{item.name}</span>
        <span className="media-lib-row-size">{fmtSize(item.size)}</span>
      </div>
      <button className="media-lib-row-remove" onClick={onRemove} title="移除" aria-label="移除">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
      {selected && (
        <div className="media-lib-row-check">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12" />
          </svg>
        </div>
      )}
    </div>
  );
}

function MediaItemCard({ item, selected, type, onClick, onRemove }: CardProps) {
  // For music type, use the compact row layout
  if (type === 'music') {
    return <MusicItemRow item={item} selected={selected} onClick={onClick} onRemove={onRemove} />;
  }

  const [thumbUrl, setThumbUrl] = useState<string | null>(null);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [hovering, setHovering] = useState(false);
  const videoRef = useRef<HTMLVideoElement>(null);
  const videoUrlRef = useRef<string | null>(null);

  // Revoke video object URL on unmount
  useEffect(() => {
    return () => {
      if (videoUrlRef.current) URL.revokeObjectURL(videoUrlRef.current);
    };
  }, []);

  const loadThumb = useCallback(async () => {
    if (type === 'video') {
      setThumbUrl(item.thumbnail ?? null);
      setLoaded(true);
      return;
    }
    // Image
    if (item.thumbnail) {
      setThumbUrl(item.thumbnail);
      setLoaded(true);
      return;
    }
    try {
      const url = await getMediaObjectURL(item.id);
      setThumbUrl(url);
    } catch { /* noop */ }
    setLoaded(true);
  }, [item.id, item.thumbnail, type]);

  // Load video blob on hover only (deferred to avoid blocking initial render)
  useEffect(() => {
    if (!hovering || type !== 'video' || videoUrl) return;
    let cancelled = false;
    (async () => {
      try {
        const blob = await getMediaBlob(item.id);
        if (cancelled || !blob) return;
        if (videoUrlRef.current) URL.revokeObjectURL(videoUrlRef.current);
        const url = URL.createObjectURL(blob);
        videoUrlRef.current = url;
        setVideoUrl(url);
      } catch { /* noop */ }
    })();
    return () => { cancelled = true; };
  }, [hovering, type, item.id, videoUrl]);

  // Load thumbnail immediately on mount (preloads in background even when hidden)
  const cardRef = useRef<HTMLDivElement>(null);
  useEffect(() => { loadThumb(); }, [loadThumb]);

  // Play on hover: set src → wait canplay → play. Remove src on leave so poster shows.
  useEffect(() => {
    const v = videoRef.current;
    if (!v || !videoUrl) return;
    if (hovering) {
      if (!v.src || v.src !== videoUrl) v.src = videoUrl;
      const doPlay = () => v.play().catch(() => {});
      if (v.readyState >= 2) {
        v.currentTime = 0;
        doPlay();
      } else {
        v.currentTime = 0;
        v.addEventListener('canplay', doPlay, { once: true });
      }
    } else {
      v.pause();
      v.removeAttribute('src');
      v.load();
    }
  }, [hovering, videoUrl]);

  const handleMouseEnter = useCallback(() => setHovering(true), []);
  const handleMouseLeave = useCallback(() => setHovering(false), []);

  return (
    <div
      ref={cardRef}
      className={`media-lib-card${selected ? ' selected' : ''}${loaded ? ' loaded' : ''}${hovering ? ' hovering' : ''}`}
      onClick={onClick}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      title={`${item.name}\n${fmtSize(item.size)}`}
    >
      <div className="media-lib-card-thumb">
        {type === 'video' ? (
          <>
            {thumbUrl && <img className="media-lib-video-poster" src={thumbUrl} alt="" />}
            <video
              ref={videoRef}
              muted
              playsInline
              preload="none"
            />
          </>
        ) : thumbUrl ? (
          <img src={thumbUrl} alt="" />
        ) : (
          <div className="media-lib-card-placeholder" />
        )}
      </div>
      <div className="media-lib-card-info">
        <span className="media-lib-card-name">{item.name}</span>
        <div className="media-lib-card-meta">
          <span className="media-lib-card-size">{fmtSize(item.size)}</span>
          <button className="media-lib-card-remove" onClick={onRemove} title="移除" aria-label="移除">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
      </div>
      {selected && (
        <div className="media-lib-card-check">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12" />
          </svg>
        </div>
      )}
    </div>
  );
}
