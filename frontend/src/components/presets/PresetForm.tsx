import { useState, useEffect, useRef, type RefObject } from 'react';
import type { CustomPreset } from '../../api/types';

interface Props {
  editingId: string | null;
  preset?: CustomPreset;
  onSave: (name: string, icon: string, description: string, instructions: string) => void;
  onClose: () => void;
}

export function PresetForm({ editingId, preset, onSave, onClose }: Props) {
  const [formName, setFormName] = useState(preset?.name || '');
  const [formIcon, setFormIcon] = useState(preset?.icon || '');
  const [formDesc, setFormDesc] = useState(preset?.description || '');
  const [formInstructions, setFormInstructions] = useState(preset?.system_instructions.join('\n') || '');
  const nameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (nameInputRef.current) setTimeout(() => nameInputRef.current?.focus(), 50);
  }, []);

  const handleSave = () => {
    if (!formName.trim()) return;
    onSave(formName.trim(), formIcon.trim() || '✨', formDesc.trim(),
      formInstructions.split('\n').map(s => s.trim()).filter(Boolean).join('\n'));
  };

  return (
    <div className="preset-custom-form">
      <div className="preset-form-header">
        <span className="preset-form-title">{editingId ? '编辑人格' : '创建人格'}</span>
        <button type="button" className="preset-form-close" onClick={onClose}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>
      <div className="preset-form-fields" onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && e.target instanceof HTMLInputElement) { e.preventDefault(); handleSave(); } }}>
        <div className="preset-form-row">
          <input type="text" className="preset-form-icon-input" placeholder="✨" value={formIcon} onChange={e => setFormIcon(e.target.value)} maxLength={4} />
          <input ref={nameInputRef as RefObject<HTMLInputElement>} type="text" className="preset-form-name-input" placeholder="人格名称" value={formName} onChange={e => setFormName(e.target.value)} />
        </div>
        <input type="text" className="preset-form-desc-input" placeholder="简短描述" value={formDesc} onChange={e => setFormDesc(e.target.value)} />
        <textarea className="preset-form-instructions-input"
          placeholder="系统指令（每行一条）&#10;例如：你是一只可爱的小猫&#10;你喜欢在句尾加上喵~"
          value={formInstructions} onChange={e => setFormInstructions(e.target.value)} rows={5} />
        <div className="preset-form-footer-info">
          {formInstructions.trim() && (
            <span className="preset-form-hint">{formInstructions.split('\n').filter(s => s.trim()).length} 条指令</span>
          )}
          {(formName.trim() || formIcon.trim()) && (
            <div className="preset-form-preview">
              <span className="preset-form-preview-label">预览：</span>
              <span className="preset-preview-item">
                <span className="preset-preview-icon">{formIcon.trim() || '✨'}</span>
                <span className="preset-preview-name">{formName.trim() || '未命名'}</span>
              </span>
            </div>
          )}
        </div>
      </div>
      <div className="preset-form-actions">
        <button type="button" className="preset-form-cancel" onClick={onClose}>取消</button>
        <button type="button" className="preset-form-save" onClick={handleSave} disabled={!formName.trim()}>
          {editingId ? '保存' : '创建'}
        </button>
      </div>
    </div>
  );
}
