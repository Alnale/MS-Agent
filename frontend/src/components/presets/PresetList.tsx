import type { PresetDef, CustomPreset } from '../../api/types';

interface Props {
  activePresetId: string | null;
  focusIndex: number;
  filteredBuiltin: PresetDef[];
  filteredCustom: CustomPreset[];
  onSelect: (id: string | null) => void;
  onFocus: (index: number) => void;
  onEdit: (preset: CustomPreset, e: React.MouseEvent) => void;
  onDelete: (id: string, e: React.MouseEvent) => void;
  onDuplicate: (preset: CustomPreset, e: React.MouseEvent) => void;
}

export function PresetList({
  activePresetId, focusIndex,
  filteredBuiltin, filteredCustom,
  onSelect, onFocus, onEdit, onDelete, onDuplicate,
}: Props) {
  const items: React.ReactNode[] = [];
  let globalIdx = 0;

  // "None" option
  const noneIdx = globalIdx++;
  items.push(
    <button key="__none" type="button"
      className={`preset-panel-item preset-item-none${!activePresetId ? ' selected' : ''}${focusIndex === noneIdx ? ' focused' : ''}`}
      onClick={() => onSelect(null)} onMouseEnter={() => onFocus(noneIdx)}>
      <span className="preset-item-icon preset-item-none-icon">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="10" /><line x1="4.93" y1="4.93" x2="19.07" y2="19.07" />
        </svg>
      </span>
      <span className="preset-item-info">
        <span className="preset-item-name">默认模式</span>
        <span className="preset-item-desc">不使用任何预设人格</span>
      </span>
      {!activePresetId && (
        <svg className="preset-item-check" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="20 6 9 17 4 12" />
        </svg>
      )}
    </button>
  );

  // Built-in presets
  if (filteredBuiltin.length > 0) {
    items.push(
      <div key="__sep-builtin" className="preset-section-sep">
        <span className="preset-section-label">内置人格</span>
        <span className="preset-section-line" />
      </div>
    );
    for (const preset of filteredBuiltin) {
      const idx = globalIdx++;
      items.push(
        <button key={preset.id} type="button"
          className={`preset-panel-item${activePresetId === preset.id ? ' selected' : ''}${focusIndex === idx ? ' focused' : ''}`}
          onClick={() => onSelect(preset.id)} onMouseEnter={() => onFocus(idx)}>
          <span className="preset-item-icon">{preset.icon}</span>
          <span className="preset-item-info">
            <span className="preset-item-name">{preset.name}</span>
            <span className="preset-item-desc">{preset.description}</span>
          </span>
          {preset.system_instructions.length > 0 && <span className="preset-item-count">{preset.system_instructions.length}</span>}
          {activePresetId === preset.id && (
            <svg className="preset-item-check" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="20 6 9 17 4 12" />
            </svg>
          )}
        </button>
      );
    }
  }

  // Custom presets
  if (filteredCustom.length > 0) {
    items.push(
      <div key="__sep-custom" className="preset-section-sep">
        <span className="preset-section-label">自定义人格</span>
        <span className="preset-section-line" />
      </div>
    );
    for (const preset of filteredCustom) {
      const idx = globalIdx++;
      items.push(
        <div key={preset.id} role="button" tabIndex={0}
          className={`preset-panel-item custom${activePresetId === preset.id ? ' selected' : ''}${focusIndex === idx ? ' focused' : ''}`}
          onClick={() => onSelect(preset.id)} onMouseEnter={() => onFocus(idx)}
          onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelect(preset.id); } }}>
          <span className="preset-item-icon">{preset.icon}</span>
          <span className="preset-item-info">
            <span className="preset-item-name">{preset.name}</span>
            <span className="preset-item-desc">{preset.description || '自定义人格'}</span>
          </span>
          {preset.system_instructions.length > 0 && <span className="preset-item-count">{preset.system_instructions.length}</span>}
          <span className="preset-item-actions">
            <button type="button" className="preset-item-duplicate" onClick={e => onDuplicate(preset, e)} title="复制">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
              </svg>
            </button>
            <button type="button" className="preset-item-edit" onClick={e => onEdit(preset, e)} title="编辑">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" /><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
              </svg>
            </button>
            <button type="button" className="preset-item-delete" onClick={e => onDelete(preset.id, e)} title="删除">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
              </svg>
            </button>
          </span>
          {activePresetId === preset.id && (
            <svg className="preset-item-check" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="20 6 9 17 4 12" />
            </svg>
          )}
        </div>
      );
    }
  }

  return <>{items}</>;
}
