import { useState, useRef, useEffect, useCallback } from 'react';
import type { PresetDef, CustomPreset } from '../api/types';
import { PresetForm } from './presets/PresetForm';
import { PresetList } from './presets/PresetList';

interface Props {
  baseUrl: string;
  activePresetId: string | null;
  onSelect: (presetId: string | null) => void;
  builtinPresets: PresetDef[];
  customPresets: CustomPreset[];
  onAddCustom: (preset: CustomPreset) => void;
  onUpdateCustom: (id: string, updates: Partial<CustomPreset>) => void;
  onDeleteCustom: (id: string) => void;
  externalOpen?: boolean;
  onExternalOpenHandled?: () => void;
  locked?: boolean;
}

export function PresetSelector({
  activePresetId, onSelect, builtinPresets, customPresets,
  onAddCustom, onUpdateCustom, onDeleteCustom,
  externalOpen, onExternalOpenHandled, locked,
}: Props) {
  const [open, setOpen] = useState(false);
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [focusIndex, setFocusIndex] = useState(-1);
  const [switchAnim, setSwitchAnim] = useState(false);
  const [search, setSearch] = useState('');
  const prevPresetIdRef = useRef(activePresetId);
  const ref = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const keyboardNavRef = useRef(false);

  const allPresets = [...builtinPresets, ...customPresets];
  const activePreset = allPresets.find(p => p.id === activePresetId);

  const q = search.trim().toLowerCase();
  const filteredBuiltin = q ? builtinPresets.filter(p => p.name.toLowerCase().includes(q) || p.description.toLowerCase().includes(q)) : builtinPresets;
  const filteredCustom = q ? customPresets.filter(p => p.name.toLowerCase().includes(q) || (p.description || '').toLowerCase().includes(q)) : customPresets;
  const hasResults = filteredBuiltin.length > 0 || filteredCustom.length > 0;
  const totalItems = 1 + filteredBuiltin.length + filteredCustom.length;
  const showSearch = allPresets.length > 6;

  useEffect(() => {
    if (externalOpen && !locked) { setOpen(true); setShowForm(false); setSearch(''); onExternalOpenHandled?.(); }
    else if (externalOpen) onExternalOpenHandled?.();
  }, [externalOpen, onExternalOpenHandled, locked]);

  useEffect(() => {
    if (prevPresetIdRef.current !== activePresetId) {
      prevPresetIdRef.current = activePresetId;
      setSwitchAnim(true);
      const timer = setTimeout(() => setSwitchAnim(false), 500);
      return () => clearTimeout(timer);
    }
  }, [activePresetId]);

  useEffect(() => {
    if (open && showSearch && searchInputRef.current && !showForm) setTimeout(() => searchInputRef.current?.focus(), 80);
  }, [open, showSearch, showForm]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) { setOpen(false); setShowForm(false); setEditingId(null); setSearch(''); }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (showForm) { setShowForm(false); setEditingId(null); }
        else { setOpen(false); setSearch(''); }
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [open, showForm]);

  useEffect(() => { if (!open) setFocusIndex(-1); }, [open]);

  const handleSelect = useCallback((id: string | null) => {
    onSelect(id); setOpen(false); setShowForm(false); setEditingId(null); setSearch('');
  }, [onSelect]);

  const resetForm = useCallback(() => { setShowForm(false); setEditingId(null); }, []);

  const handleOpenCreate = useCallback(() => { resetForm(); setShowForm(true); }, [resetForm]);

  const handleEdit = useCallback((preset: CustomPreset, e: React.MouseEvent) => {
    e.stopPropagation();
    setEditingId(preset.id);
    setShowForm(true);
  }, []);

  const handleDelete = useCallback((id: string, e: React.MouseEvent) => { e.stopPropagation(); onDeleteCustom(id); }, [onDeleteCustom]);

  const handleDuplicate = useCallback((preset: CustomPreset, e: React.MouseEvent) => {
    e.stopPropagation();
    onAddCustom({ ...preset, id: `custom_${Date.now()}`, name: `${preset.name} (副本)`, isCustom: true });
  }, [onAddCustom]);

  const handleSaveForm = useCallback((name: string, icon: string, description: string, instructions: string) => {
    const instructionList = instructions.split('\n').map(s => s.trim()).filter(Boolean);
    if (editingId) {
      onUpdateCustom(editingId, { name, icon, description, system_instructions: instructionList });
    } else {
      onAddCustom({ id: `custom_${Date.now()}`, name, icon, description, system_instructions: instructionList, isCustom: true });
    }
    setShowForm(false); setEditingId(null);
  }, [editingId, onUpdateCustom, onAddCustom]);

  const handleListKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (showForm) return;
    if (e.key === 'ArrowDown') { e.preventDefault(); keyboardNavRef.current = true; setFocusIndex(prev => Math.min(prev + 1, totalItems - 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); keyboardNavRef.current = true; setFocusIndex(prev => Math.max(prev - 1, 0)); }
    else if (e.key === 'Enter' && focusIndex >= 0) {
      e.preventDefault();
      if (focusIndex === 0) handleSelect(null);
      else { const preset = [...filteredBuiltin, ...filteredCustom][focusIndex - 1]; if (preset) handleSelect(preset.id); }
    }
  }, [showForm, focusIndex, totalItems, filteredBuiltin, filteredCustom, handleSelect]);

  useEffect(() => {
    if (focusIndex < 0 || !listRef.current || !keyboardNavRef.current) return;
    keyboardNavRef.current = false;
    const items = listRef.current.querySelectorAll('.preset-panel-item');
    items[focusIndex]?.scrollIntoView({ block: 'nearest' });
  }, [focusIndex]);

  const editingPreset = editingId ? customPresets.find(p => p.id === editingId) : undefined;

  return (
    <div className="preset-selector" ref={ref}>
      <button type="button"
        className={`preset-selector-trigger${activePresetId ? ' active-preset' : ''}${open ? ' active' : ''}${switchAnim ? ' preset-switch' : ''}${locked ? ' locked' : ''}`}
        onClick={() => { if (!locked) setOpen(!open); }}
        title={locked ? (activePreset ? `人格已锁定: ${activePreset.name}` : '人格已锁定') : (activePreset ? `当前人格: ${activePreset.name}` : '选择人格')}
        disabled={locked}>
        {activePreset ? (
          <span className="preset-trigger-content">
            <span className="preset-trigger-icon">{activePreset.icon}</span>
            <span className="preset-trigger-name">{activePreset.name}</span>
            {locked && (
              <svg className="preset-trigger-lock" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <rect x="3" y="11" width="18" height="11" rx="2" ry="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" />
              </svg>
            )}
          </span>
        ) : (
          <span className="preset-trigger-content">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" /><circle cx="12" cy="7" r="4" />
            </svg>
            <span className="preset-trigger-label">人格</span>
          </span>
        )}
      </button>

      {open && (
        <div className={`preset-selector-panel${showForm ? ' show-form' : ''}`} onKeyDown={handleListKeyDown}>
          {showForm ? (
            <PresetForm editingId={editingId} preset={editingPreset} onSave={handleSaveForm}
              onClose={() => { setShowForm(false); setEditingId(null); }} />
          ) : (
            <>
              <div className="preset-panel-header">
                <span className="preset-panel-title">选择人格</span>
                <span className="preset-panel-count">{allPresets.length}</span>
              </div>

              {showSearch && (
                <div className="preset-panel-search">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" />
                  </svg>
                  <input ref={searchInputRef} type="text" placeholder="搜索人格..." value={search}
                    onChange={e => { setSearch(e.target.value); setFocusIndex(-1); }}
                    onKeyDown={e => { if (e.key === 'Escape') { setSearch(''); e.stopPropagation(); } }} />
                  {search && (
                    <button type="button" className="preset-search-clear" onClick={() => { setSearch(''); searchInputRef.current?.focus(); }}>
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                        <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                      </svg>
                    </button>
                  )}
                </div>
              )}

              <div className="preset-panel-list" ref={listRef}>
                <PresetList
                  activePresetId={activePresetId} focusIndex={focusIndex}
                  filteredBuiltin={filteredBuiltin} filteredCustom={filteredCustom}
                  onSelect={handleSelect} onFocus={setFocusIndex}
                  onEdit={handleEdit} onDelete={handleDelete} onDuplicate={handleDuplicate}
                />

                {allPresets.length === 0 && (
                  <div className="preset-panel-empty">
                    <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" /><circle cx="12" cy="7" r="4" />
                    </svg>
                    <span className="preset-panel-empty-title">暂无人格预设</span>
                    <span className="preset-panel-empty-hint">点击下方按钮创建</span>
                  </div>
                )}

                {allPresets.length > 0 && !hasResults && (
                  <div className="preset-panel-empty">
                    <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                      <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" />
                    </svg>
                    <span className="preset-panel-empty-title">未找到匹配人格</span>
                    <span className="preset-panel-empty-hint">尝试其他关键词</span>
                  </div>
                )}
              </div>

              <div className="preset-panel-footer">
                <button type="button" className="preset-create-btn" onClick={handleOpenCreate}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
                  </svg>
                  创建自定义人格
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
