import { useState, useMemo, useCallback, memo } from 'react';
import type { Session } from '../hooks/useSession';

interface Props {
  sessions: Session[];
  currentSessionId: string | null;
  onSelect: (session: Session) => void;
  onDelete: (sessionId: string) => void;
  onDeleteBatch: (sessionIds: string[]) => void;
  onNew: () => void;
}

export const SessionList = memo(function SessionList({ sessions, currentSessionId, onSelect, onDelete, onDeleteBatch, onNew }: Props) {
  const [search, setSearch] = useState('');
  const [selectMode, setSelectMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());

  const filtered = useMemo(() => {
    if (!search.trim()) return sessions;
    const q = search.toLowerCase();
    return sessions.filter((s) =>
      s.title.toLowerCase().includes(q) ||
      s.messages.some((m) => m.content.toLowerCase().includes(q))
    );
  }, [sessions, search]);

  const toggleSelect = useCallback((sessionId: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(sessionId)) {
        next.delete(sessionId);
      } else {
        next.add(sessionId);
      }
      return next;
    });
  }, []);

  const toggleSelectAll = useCallback(() => {
    const selectableIds = filtered
      .filter((s) => s.id !== currentSessionId)
      .map((s) => s.id);
    if (selectedIds.size === selectableIds.length && selectableIds.length > 0) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(selectableIds));
    }
  }, [filtered, selectedIds.size, currentSessionId]);

  const enterSelectMode = useCallback(() => {
    setSelectMode(true);
    setSelectedIds(new Set());
  }, []);

  const exitSelectMode = useCallback(() => {
    setSelectMode(false);
    setSelectedIds(new Set());
  }, []);

  const handleBatchDelete = useCallback(() => {
    if (selectedIds.size === 0) return;
    const count = selectedIds.size;
    if (window.confirm(`确定要删除选中的 ${count} 条对话记录吗？此操作不可撤销。`)) {
      onDeleteBatch(Array.from(selectedIds));
      setSelectMode(false);
      setSelectedIds(new Set());
    }
  }, [selectedIds, onDeleteBatch]);

  const handleExport = (e: React.MouseEvent, session: Session) => {
    e.stopPropagation();
    const lines = session.messages.map((m) => {
      const role = m.role === 'user' ? '用户' : '助手';
      const time = new Date(m.timestamp).toLocaleString();
      return `### ${role} (${time})\n\n${m.content}\n`;
    });
    const md = `# ${session.title}\n\n导出时间: ${new Date().toLocaleString()}\n\n---\n\n${lines.join('\n---\n\n')}`;
    const blob = new Blob([md], { type: 'text/markdown;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${session.title.slice(0, 20)}_${new Date().toISOString().slice(0, 10)}.md`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const formatDate = (timestamp: number) => {
    const date = new Date(timestamp);
    const now = new Date();
    const isToday = date.toDateString() === now.toDateString();

    if (isToday) {
      return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    }

    const yesterday = new Date(now);
    yesterday.setDate(yesterday.getDate() - 1);
    const isYesterday = date.toDateString() === yesterday.toDateString();

    if (isYesterday) {
      return '昨天';
    }

    return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
  };

  const selectableCount = filtered.filter((s) => s.id !== currentSessionId).length;
  const allSelected = selectableCount > 0 && selectedIds.size === selectableCount;
  const someSelected = selectedIds.size > 0;

  return (
    <div className="session-list">
      <div className="session-list-header">
        <h3>聊天记录</h3>
        <div className="session-header-actions">
          {selectMode ? (
            <button className="session-mode-btn cancel" onClick={exitSelectMode} title="取消选择" aria-label="取消选择">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
              取消
            </button>
          ) : (
            <button className="session-mode-btn select" onClick={enterSelectMode} title="批量管理" aria-label="批量管理">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="9 11 12 14 22 4" />
                <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
              </svg>
              管理
            </button>
          )}
          <button className="new-chat-btn" onClick={onNew} title="新建聊天" aria-label="新建聊天">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            新建
          </button>
        </div>
      </div>

      {sessions.length > 2 && !selectMode && (
        <div className="session-search">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <input
            type="text"
            placeholder="搜索对话..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      )}

      {selectMode && (
        <div className="session-batch-bar">
          <button className="session-batch-toggle" onClick={toggleSelectAll}>
            <span className={`session-checkbox ${allSelected ? 'checked' : someSelected ? 'partial' : ''}`}>
              {allSelected && (
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              )}
              {someSelected && !allSelected && <span className="checkbox-partial-mark" />}
            </span>
            <span className="session-batch-label">{allSelected ? '取消全选' : '全选'}</span>
          </button>
          <span className="session-batch-count">
            {someSelected ? `已选 ${selectedIds.size} 项` : '选择对话'}
          </span>
        </div>
      )}

      <div className="session-items">
        {filtered.length === 0 ? (
          <div className="session-empty">{search ? '未找到匹配的对话' : '暂无对话记录'}</div>
        ) : (
          filtered.map((session) => (
            <div
              key={session.id}
              className={`session-item ${session.id === currentSessionId && !selectMode ? 'active' : ''} ${selectMode && selectedIds.has(session.id) ? 'selected' : ''}`}
              onClick={() => {
                if (selectMode) {
                  toggleSelect(session.id);
                } else {
                  onSelect(session);
                }
              }}
            >
              {selectMode && (
                <span className={`session-checkbox ${selectedIds.has(session.id) ? 'checked' : ''}`}>
                  {selectedIds.has(session.id) && (
                    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                  )}
                </span>
              )}
              <div className="session-item-content">
                <div className="session-title">{session.title}</div>
                <div className="session-meta">
                  <span className="session-date">{formatDate(session.updatedAt)}</span>
                  <span className="session-count">{session.messages.length} 条消息</span>
                </div>
              </div>
              {!selectMode && (
                <>
                  <button
                    className="session-export"
                    onClick={(e) => handleExport(e, session)}
                    title="导出为 Markdown"
                    aria-label="导出为 Markdown"
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                      <polyline points="7 10 12 15 17 10" />
                      <line x1="12" y1="15" x2="12" y2="3" />
                    </svg>
                  </button>
                  <button
                    className="session-delete"
                    onClick={(e) => {
                      e.stopPropagation();
                      if (window.confirm('确定要删除这个对话吗？')) {
                        onDelete(session.id);
                      }
                    }}
                    title="删除"
                    aria-label="删除"
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <polyline points="3 6 5 6 21 6" />
                      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                    </svg>
                  </button>
                </>
              )}
            </div>
          ))
        )}
      </div>

      {selectMode && someSelected && (
        <div className="session-batch-actions">
          <button className="session-batch-delete" onClick={handleBatchDelete}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="3 6 5 6 21 6" />
              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
              <line x1="10" y1="11" x2="10" y2="17" />
              <line x1="14" y1="11" x2="14" y2="17" />
            </svg>
            删除选中 ({selectedIds.size})
          </button>
        </div>
      )}
    </div>
  );
});
