const DB_NAME = 'agent-teams-bg';
const STORE_NAME = 'background';
const KEY_IMAGE = 'bg-image';
const KEY_VIDEO = 'bg-video';

// Reuse a single DB connection instead of opening a new one per operation
let dbPromise: Promise<IDBDatabase> | null = null;

function openDB(): Promise<IDBDatabase> {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      req.result.createObjectStore(STORE_NAME);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => {
      dbPromise = null; // Reset on failure so next call retries
      reject(req.error);
    };
  });
  return dbPromise;
}

async function get(key: string): Promise<string | null> {
  try {
    const db = await openDB();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readonly');
      const req = tx.objectStore(STORE_NAME).get(key);
      req.onsuccess = () => {
        const result = req.result;
        // Handle both Blob (video) and string (image data URL) stored values
        if (result instanceof Blob) {
          resolve(URL.createObjectURL(result));
        } else {
          resolve(result ?? null);
        }
      };
      req.onerror = () => reject(req.error);
    });
  } catch {
    return null;
  }
}

async function put(key: string, value: string | Blob): Promise<void> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    tx.objectStore(STORE_NAME).put(value, key);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function del(key: string): Promise<void> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    tx.objectStore(STORE_NAME).delete(key);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

export const loadBgImage = () => get(KEY_IMAGE);
export const saveBgImage = (d: string | Blob) => put(KEY_IMAGE, d);
export const removeBgImage = () => del(KEY_IMAGE);

export const loadBgVideo = () => get(KEY_VIDEO);
export const saveBgVideo = (d: Blob | string) => put(KEY_VIDEO, d);
export const removeBgVideo = () => del(KEY_VIDEO);
