const DB_NAME = 'agent-teams-media-library';
const STORE_NAME = 'media';

export interface MediaItem {
  id: string;
  type: 'image' | 'video' | 'music';
  name: string;
  size: number;
  mimeType: string;
  addedAt: number;
  folder?: string;
  blob: Blob;
  thumbnail?: string;
  /** LRC-format lyrics for music files */
  lyrics?: string;
}

export type MediaItemMeta = Omit<MediaItem, 'blob' | 'thumbnail'> & { thumbnail?: string };

let dbPromise: Promise<IDBDatabase> | null = null;

function openDB(): Promise<IDBDatabase> {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        const store = db.createObjectStore(STORE_NAME, { keyPath: 'id' });
        store.createIndex('type', 'type', { unique: false });
        store.createIndex('folder', 'folder', { unique: false });
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => {
      dbPromise = null;
      reject(req.error);
    };
  });
  return dbPromise;
}

function genId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

const IMAGE_EXTS = ['.jpg', '.jpeg', '.png', '.gif', '.webp', '.bmp', '.svg', '.avif'];
const VIDEO_EXTS = ['.mp4', '.webm', '.ogg', '.mov', '.mkv', '.avi'];
const MUSIC_EXTS = ['.mp3', '.wav', '.ogg', '.flac', '.aac', '.m4a', '.wma'];

export function classifyFile(file: File | { name: string; type: string }): 'image' | 'video' | 'music' | null {
  const ext = '.' + (file.name.split('.').pop() || '').toLowerCase();
  if (file.type.startsWith('image/') || IMAGE_EXTS.includes(ext)) return 'image';
  if (file.type.startsWith('video/') || VIDEO_EXTS.includes(ext)) return 'video';
  if (file.type.startsWith('audio/') || MUSIC_EXTS.includes(ext)) return 'music';
  return null;
}

function makeThumbnail(file: File): Promise<string | undefined> {
  return new Promise((resolve) => {
    if (file.type.startsWith('image/')) {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result as string);
      reader.onerror = () => resolve(undefined);
      reader.readAsDataURL(file);
    } else if (file.type.startsWith('video/')) {
      const url = URL.createObjectURL(file);
      const video = document.createElement('video');
      video.preload = 'metadata';
      video.muted = true;
      video.onloadeddata = () => {
        video.currentTime = Math.min(1, video.duration / 4);
      };
      video.onseeked = () => {
        const canvas = document.createElement('canvas');
        canvas.width = 160;
        canvas.height = Math.round(160 * (video.videoHeight / video.videoWidth)) || 90;
        const ctx = canvas.getContext('2d');
        if (ctx) {
          ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
          resolve(canvas.toDataURL('image/jpeg', 0.6));
        } else {
          resolve(undefined);
        }
        URL.revokeObjectURL(url);
      };
      video.onerror = () => { URL.revokeObjectURL(url); resolve(undefined); };
      video.src = url;
    } else {
      resolve(undefined);
    }
  });
}

export async function addMediaItem(
  file: File,
  type: 'image' | 'video' | 'music',
  folder?: string,
): Promise<MediaItem> {
  const db = await openDB();
  const thumbnail = await makeThumbnail(file);
  const item: MediaItem = {
    id: genId(),
    type,
    name: file.name.replace(/\.[^.]+$/, ''),
    size: file.size,
    mimeType: file.type,
    addedAt: Date.now(),
    folder,
    blob: file,
    thumbnail,
  };
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    tx.objectStore(STORE_NAME).put(item);
    tx.oncomplete = () => resolve(item);
    tx.onerror = () => reject(tx.error);
  });
}

export async function getMediaItems(type?: 'image' | 'video' | 'music'): Promise<MediaItemMeta[]> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readonly');
    const store = tx.objectStore(STORE_NAME);
    const req = type ? store.index('type').getAll(type) : store.getAll();
    req.onsuccess = () => {
      const items: MediaItemMeta[] = (req.result as MediaItem[]).map(({ blob, ...meta }) => meta);
      items.sort((a, b) => b.addedAt - a.addedAt);
      resolve(items);
    };
    req.onerror = () => reject(req.error);
  });
}

export async function findDuplicate(name: string, type: string): Promise<MediaItemMeta | null> {
  const items = await getMediaItems(type as 'image' | 'video' | 'music');
  return items.find(i => i.name === name) ?? null;
}

export async function getMediaBlob(id: string): Promise<Blob | null> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readonly');
    const req = tx.objectStore(STORE_NAME).get(id);
    req.onsuccess = () => resolve(req.result?.blob ?? null);
    req.onerror = () => reject(req.error);
  });
}

export async function getMediaObjectURL(id: string): Promise<string | null> {
  const blob = await getMediaBlob(id);
  return blob ? URL.createObjectURL(blob) : null;
}

export async function removeMediaItem(id: string): Promise<void> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    tx.objectStore(STORE_NAME).delete(id);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

export async function clearMediaLibrary(type?: 'image' | 'video' | 'music'): Promise<void> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    const store = tx.objectStore(STORE_NAME);
    if (type) {
      const idx = store.index('type');
      const req = idx.openCursor(type);
      req.onsuccess = () => {
        const cursor = req.result;
        if (cursor) {
          cursor.delete();
          cursor.continue();
        }
      };
    } else {
      store.clear();
    }
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

export async function removeMediaByFolder(folder: string, type: 'image' | 'video' | 'music'): Promise<void> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    const store = tx.objectStore(STORE_NAME);
    const req = store.index('type').openCursor(type);
    req.onsuccess = () => {
      const cursor = req.result;
      if (cursor) {
        if ((cursor.value.folder || '未分类') === folder) {
          cursor.delete();
        }
        cursor.continue();
      }
    };
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

export async function updateMediaLyrics(id: string, lyrics: string): Promise<void> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    const store = tx.objectStore(STORE_NAME);
    const getReq = store.get(id);
    getReq.onsuccess = () => {
      const item = getReq.result;
      if (item) {
        item.lyrics = lyrics;
        store.put(item);
      }
    };
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

export async function getMediaLyrics(id: string): Promise<string | null> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readonly');
    const store = tx.objectStore(STORE_NAME);
    const req = store.get(id);
    req.onsuccess = () => resolve(req.result?.lyrics || null);
    req.onerror = () => reject(req.error);
  });
}
