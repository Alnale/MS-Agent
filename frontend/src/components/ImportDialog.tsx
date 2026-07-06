import { useState, memo } from 'react';

export type ConflictChoice = 'skip' | 'overwrite' | 'cancel';
export type SubfolderChoice = 'include' | 'exclude';

export interface ConflictInfo {
  fileName: string;
  existingFolder?: string;
}

export interface SubfolderInfo {
  folders: string[];
}

interface ConflictProps {
  mode: 'conflict';
  info: ConflictInfo;
  onResolve: (choice: ConflictChoice, remember: boolean) => void;
}

interface SubfolderProps {
  mode: 'subfolder';
  info: SubfolderInfo;
  onResolve: (choice: SubfolderChoice, remember: boolean) => void;
}

type Props = ConflictProps | SubfolderProps;

export const ImportDialog = memo(function ImportDialog({ mode, info, onResolve }: Props) {
  const [remember, setRemember] = useState(false);

  if (mode === 'conflict') {
    const { fileName = '', existingFolder } = info as ConflictInfo;
    return (
      <div className="import-dialog-overlay">
        <div className="import-dialog">
          <div className="import-dialog-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
              <line x1="12" y1="9" x2="12" y2="13" /><line x1="12" y1="17" x2="12.01" y2="17" />
            </svg>
          </div>
          <div className="import-dialog-body">
            <h4 className="import-dialog-title">文件名冲突</h4>
            <p className="import-dialog-desc">
              「<strong>{fileName}</strong>」已存在{existingFolder ? `于「${existingFolder}」` : ''}，如何处理？
            </p>
          </div>
          <div className="import-dialog-actions">
            <button className="import-dialog-btn skip" onClick={() => onResolve('skip', remember)}>跳过</button>
            <button className="import-dialog-btn overwrite" onClick={() => onResolve('overwrite', remember)}>覆盖</button>
            <button className="import-dialog-btn cancel" onClick={() => onResolve('cancel', remember)}>取消导入</button>
          </div>
          <label className="import-dialog-remember">
            <input type="checkbox" checked={remember} onChange={e => setRemember(e.target.checked)} />
            <span>后续冲突都应用此选择</span>
          </label>
        </div>
      </div>
    );
  }

  // Subfolder mode
  const folders = (info as SubfolderInfo).folders ?? [];
  return (
    <div className="import-dialog-overlay">
      <div className="import-dialog">
        <div className="import-dialog-icon">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </svg>
        </div>
        <div className="import-dialog-body">
          <h4 className="import-dialog-title">发现子文件夹</h4>
          <p className="import-dialog-desc">
            导入内容中包含 {folders.length} 个子文件夹：
          </p>
          <div className="import-dialog-folders">
            {folders.slice(0, 5).map((f, i) => (
              <span key={i} className="import-dialog-folder-tag">{f}</span>
            ))}
            {folders.length > 5 && <span className="import-dialog-folder-tag more">+{folders.length - 5}</span>}
          </div>
        </div>
        <div className="import-dialog-actions">
          <button className="import-dialog-btn skip" onClick={() => onResolve('exclude', remember)}>仅导入当前层</button>
          <button className="import-dialog-btn overwrite" onClick={() => onResolve('include', remember)}>遍历子文件夹</button>
        </div>
        <label className="import-dialog-remember">
          <input type="checkbox" checked={remember} onChange={e => setRemember(e.target.checked)} />
          <span>后续导入都应用此选择</span>
        </label>
      </div>
    </div>
  );
});

export default ImportDialog;
