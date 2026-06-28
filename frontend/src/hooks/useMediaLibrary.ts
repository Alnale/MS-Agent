import { useState, useCallback, useEffect, useRef } from 'react';
import {
  type MediaItemMeta,
  addMediaItem,
  getMediaItems,
  getMediaObjectURL,
  removeMediaItem,
  clearMediaLibrary,
  removeMediaByFolder,
  classifyFile,
  findDuplicate,
} from '../utils/mediaLibraryStorage';
import type { ConflictChoice, SubfolderChoice, ConflictInfo, SubfolderInfo } from '../components/ImportDialog';

export type { MediaItemMeta } from '../utils/mediaLibraryStorage';

export interface ConflictResolver {
  (info: ConflictInfo): Promise<{ choice: ConflictChoice; remember: boolean }>;
}

export interface SubfolderResolver {
  (info: SubfolderInfo): Promise<{ choice: SubfolderChoice; remember: boolean }>;
}

export interface UseMediaLibraryReturn {
  images: MediaItemMeta[];
  videos: MediaItemMeta[];
  music: MediaItemMeta[];
  loading: boolean;
  importFiles: (files: FileList | File[], folder?: string) => Promise<number>;
  importFilesByType: (type: 'image' | 'video' | 'music', files: FileList | File[], folder?: string) => Promise<number>;
  importFolder: (files: FileList, folder?: string) => Promise<number>;
  remove: (id: string) => Promise<void>;
  removeFolder: (folder: string, type: 'image' | 'video' | 'music') => Promise<void>;
  removeAll: (type: 'image' | 'video' | 'music') => Promise<void>;
  getUrl: (id: string) => Promise<string | null>;
  refresh: () => Promise<void>;
}

export function useMediaLibrary(
  conflictResolver?: ConflictResolver,
  subfolderResolver?: SubfolderResolver,
): UseMediaLibraryReturn {
  const [images, setImages] = useState<MediaItemMeta[]>([]);
  const [videos, setVideos] = useState<MediaItemMeta[]>([]);
  const [music, setMusic] = useState<MediaItemMeta[]>([]);
  const [loading, setLoading] = useState(true);

  // Per-type conflict memory — each type resolves conflicts independently
  const rememberedConflict = useRef<Record<string, ConflictChoice | null>>({});
  const rememberedSubfolder = useRef<Record<string, SubfolderChoice | null>>({});

  const conflictRef = useRef(conflictResolver);
  conflictRef.current = conflictResolver;
  const subfolderRef = useRef(subfolderResolver);
  subfolderRef.current = subfolderResolver;

  const setter = useCallback((type: 'image' | 'video' | 'music') => {
    return type === 'image' ? setImages : type === 'video' ? setVideos : setMusic;
  }, []);

  const refreshType = useCallback(async (type: 'image' | 'video' | 'music') => {
    try {
      const items = await getMediaItems(type);
      setter(type)(items);
    } catch (e) {
      console.error('Failed to load media library:', type, e);
    }
  }, [setter]);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [i, v, m] = await Promise.all([
        getMediaItems('image'),
        getMediaItems('video'),
        getMediaItems('music'),
      ]);
      setImages(i);
      setVideos(v);
      setMusic(m);
    } catch (e) {
      console.error('Failed to load media library:', e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const resolveConflict = useCallback(async (type: string, info: ConflictInfo): Promise<ConflictChoice> => {
    if (rememberedConflict.current[type]) return rememberedConflict.current[type]!;
    const resolver = conflictRef.current;
    if (!resolver) return 'skip';
    const result = await resolver(info);
    if (result.remember) rememberedConflict.current[type] = result.choice;
    return result.choice;
  }, []);

  const resolveSubfolder = useCallback(async (type: string, info: SubfolderInfo): Promise<SubfolderChoice> => {
    if (rememberedSubfolder.current[type]) return rememberedSubfolder.current[type]!;
    const resolver = subfolderRef.current;
    if (!resolver) return 'include';
    const result = await resolver(info);
    if (result.remember) rememberedSubfolder.current[type] = result.choice;
    return result.choice;
  }, []);

  // Import files for a specific type only
  const importFilesByType = useCallback(async (type: 'image' | 'video' | 'music', files: FileList | File[], folder?: string) => {
    let count = 0;
    for (const file of Array.from(files)) {
      const fileType = classifyFile(file);
      if (fileType !== type) continue;

      const baseName = file.name.replace(/\.[^.]+$/, '');
      const existing = await findDuplicate(baseName, type);

      if (existing) {
        const choice = await resolveConflict(type, { fileName: baseName, existingFolder: existing.folder });
        if (choice === 'cancel') return count;
        if (choice === 'skip') continue;
        if (choice === 'overwrite') await removeMediaItem(existing.id);
      }

      try {
        await addMediaItem(file, type, folder);
        count++;
      } catch (e) {
        console.error('Failed to import file:', file.name, e);
      }
    }
    if (count > 0) await refreshType(type);
    return count;
  }, [refreshType, resolveConflict]);

  // Generic import: auto-classifies files, but each type resolves conflicts independently
  const importFiles = useCallback(async (files: FileList | File[], folder?: string) => {
    let count = 0;
    for (const file of Array.from(files)) {
      const type = classifyFile(file);
      if (!type) continue;

      const baseName = file.name.replace(/\.[^.]+$/, '');
      const existing = await findDuplicate(baseName, type);

      if (existing) {
        const choice = await resolveConflict(type, { fileName: baseName, existingFolder: existing.folder });
        if (choice === 'cancel') return count;
        if (choice === 'skip') continue;
        if (choice === 'overwrite') await removeMediaItem(existing.id);
      }

      try {
        await addMediaItem(file, type, folder);
        count++;
      } catch (e) {
        console.error('Failed to import file:', file.name, e);
      }
    }
    if (count > 0) await refresh();
    return count;
  }, [refresh, resolveConflict]);

  const importFolder = useCallback(async (files: FileList, folder?: string) => {
    const fileList = Array.from(files);
    const rootFolder = folder || fileList[0]?.webkitRelativePath?.split('/')[0] || '未分类';

    const subfolders = new Set<string>();
    for (const file of fileList) {
      const parts = file.webkitRelativePath?.split('/') || [];
      if (parts.length > 2) subfolders.add(parts[1]);
    }

    let includeSubfolders = true;
    if (subfolders.size > 0) {
      includeSubfolders = (await resolveSubfolder('folder', { folders: Array.from(subfolders) })) === 'include';
    }

    const touchedTypes = new Set<'image' | 'video' | 'music'>();
    let count = 0;
    for (const file of fileList) {
      const type = classifyFile(file);
      if (!type) continue;

      const parts = file.webkitRelativePath?.split('/') || [];
      let fileFolder = rootFolder;
      if (includeSubfolders && parts.length > 2) {
        fileFolder = parts.slice(0, -1).join('/');
      }

      const baseName = file.name.replace(/\.[^.]+$/, '');
      const existing = await findDuplicate(baseName, type);

      if (existing) {
        const choice = await resolveConflict(type, { fileName: baseName, existingFolder: existing.folder });
        if (choice === 'cancel') break;
        if (choice === 'skip') continue;
        if (choice === 'overwrite') await removeMediaItem(existing.id);
      }

      try {
        await addMediaItem(file, type, fileFolder);
        count++;
        touchedTypes.add(type);
      } catch (e) {
        console.error('Failed to import file:', file.name, e);
      }
    }
    // Only refresh the types that were actually affected
    await Promise.all(Array.from(touchedTypes).map(t => refreshType(t)));
    return count;
  }, [refreshType, resolveConflict, resolveSubfolder]);

  const remove = useCallback(async (id: string) => {
    // Find which type this item belongs to before deleting
    const type = images.find(i => i.id === id) ? 'image'
      : videos.find(v => v.id === id) ? 'video'
      : music.find(m => m.id === id) ? 'music' : null;
    await removeMediaItem(id);
    if (type) await refreshType(type);
  }, [refreshType, images, videos, music]);

  const removeFolder = useCallback(async (folder: string, type: 'image' | 'video' | 'music') => {
    await removeMediaByFolder(folder, type);
    await refreshType(type);
  }, [refreshType]);

  const removeAll = useCallback(async (type: 'image' | 'video' | 'music') => {
    await clearMediaLibrary(type);
    await refreshType(type);
  }, [refreshType]);

  const getUrl = useCallback(async (id: string) => {
    return getMediaObjectURL(id);
  }, []);

  return {
    images, videos, music, loading,
    importFiles, importFilesByType, importFolder,
    remove, removeFolder, removeAll,
    getUrl, refresh,
  };
}
