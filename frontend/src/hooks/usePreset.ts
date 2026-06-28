import { useState, useEffect, useCallback } from 'react';
import type { PresetDef, CustomPreset } from '../api/types';

type AnyPreset = PresetDef | CustomPreset;

const ACTIVE_KEY = 'agent-teams-active-preset';
const CUSTOM_KEY = 'agent-teams-custom-presets';
const SESSION_PRESETS_KEY = 'agent-teams-session-presets';

export function usePreset(baseUrl: string) {
  const [builtinPresets, setBuiltinPresets] = useState<PresetDef[]>([]);
  const [customPresets, setCustomPresets] = useState<CustomPreset[]>(() => {
    try {
      const raw = localStorage.getItem(CUSTOM_KEY);
      return raw ? JSON.parse(raw) : [];
    } catch { return []; }
  });
  const [activePresetId, setActivePresetId] = useState<string | null>(() => {
    try { return localStorage.getItem(ACTIVE_KEY); } catch { return null; }
  });
  const [sessionPresets, setSessionPresets] = useState<Record<string, string>>(() => {
    try {
      const raw = localStorage.getItem(SESSION_PRESETS_KEY);
      return raw ? JSON.parse(raw) : {};
    } catch { return {}; }
  });

  // Fetch built-in presets from backend
  useEffect(() => {
    const url = baseUrl ? `${baseUrl}/presets` : '/presets';
    fetch(url)
      .then(r => r.json())
      .then(data => setBuiltinPresets(data.presets ?? []))
      .catch(() => {});
  }, [baseUrl]);

  // Persist custom presets
  useEffect(() => {
    localStorage.setItem(CUSTOM_KEY, JSON.stringify(customPresets));
  }, [customPresets]);

  // Persist active selection
  useEffect(() => {
    if (activePresetId) localStorage.setItem(ACTIVE_KEY, activePresetId);
    else localStorage.removeItem(ACTIVE_KEY);
  }, [activePresetId]);

  // Persist session presets
  useEffect(() => {
    localStorage.setItem(SESSION_PRESETS_KEY, JSON.stringify(sessionPresets));
  }, [sessionPresets]);

  const allPresets: AnyPreset[] = [...builtinPresets, ...customPresets];

  const activePreset = allPresets.find(p => p.id === activePresetId) ?? null;

  const systemInstructions = activePreset?.system_instructions ?? [];

  /** Bind a session to a preset */
  const setSessionPreset = useCallback((sessionId: string, presetId: string | null) => {
    setSessionPresets(prev => {
      if (presetId === null) {
        const { [sessionId]: _, ...rest } = prev;
        return rest;
      }
      return { ...prev, [sessionId]: presetId };
    });
  }, []);

  /** Get system instructions for a specific session (per-session binding, fallback to active) */
  const getSystemInstructions = useCallback((sessionId: string): string[] => {
    const boundId = sessionPresets[sessionId];
    if (boundId) {
      const preset = allPresets.find(p => p.id === boundId);
      if (preset) return preset.system_instructions;
    }
    // Fallback: use the current active preset (for sessions without explicit binding)
    return activePreset?.system_instructions ?? [];
  }, [sessionPresets, allPresets, activePreset]);

  const addCustomPreset = useCallback((preset: CustomPreset) => {
    setCustomPresets(prev => [...prev, preset]);
  }, []);

  const updateCustomPreset = useCallback((id: string, updates: Partial<CustomPreset>) => {
    setCustomPresets(prev => prev.map(p => p.id === id ? { ...p, ...updates } : p));
  }, []);

  const deleteCustomPreset = useCallback((id: string) => {
    setCustomPresets(prev => prev.filter(p => p.id !== id));
    if (activePresetId === id) setActivePresetId(null);
    // Also remove from session bindings
    setSessionPresets(prev => {
      const next: Record<string, string> = {};
      for (const [sid, pid] of Object.entries(prev)) {
        if (pid !== id) next[sid] = pid;
      }
      return next;
    });
  }, [activePresetId]);

  return {
    allPresets,
    builtinPresets,
    customPresets,
    activePreset,
    activePresetId,
    setActivePresetId,
    systemInstructions,
    sessionPresets,
    setSessionPreset,
    getSystemInstructions,
    addCustomPreset,
    updateCustomPreset,
    deleteCustomPreset,
  };
}
