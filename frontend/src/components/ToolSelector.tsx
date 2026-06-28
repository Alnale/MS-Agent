import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import type { ToolDefinition, ToolStatusEvent } from '../api/types';
import { LgGlassInteractive } from './LgGlassInteractive';
import { ParamInput } from './tools/ParamInput';
import { getAllParams, getVisibleParams, isContextRequired, abbreviateToolName, TOOL_PARAM_DEPS } from './tools/toolParamUtils';

interface StatusAnim {
  id: number;
  toolName: string;
  success: boolean;
}

interface Props {
  baseUrl: string;
  onSelect: (syntax: string) => void;
  onDirectExecute?: (syntax: string) => void;
  toolEvents?: ToolStatusEvent[];
}

export function ToolSelector({ baseUrl, onSelect, onDirectExecute, toolEvents }: Props) {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<ToolDefinition[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState('');
  const [selectedTool, setSelectedTool] = useState<ToolDefinition | null>(null);
  const [paramValues, setParamValues] = useState<Record<string, string>>({});
  const ref = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const [animQueue, setAnimQueue] = useState<StatusAnim[]>([]);
  const [currentAnim, setCurrentAnim] = useState<StatusAnim | null>(null);
  const animTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const prevEventCountRef = useRef(0);
  const animIdRef = useRef(0);

  useEffect(() => {
    if (!toolEvents || toolEvents.length <= prevEventCountRef.current) {
      prevEventCountRef.current = toolEvents?.length ?? 0;
      return;
    }
    const newEvents = toolEvents.slice(prevEventCountRef.current);
    prevEventCountRef.current = toolEvents.length;
    const newAnims: StatusAnim[] = [];
    for (const ev of newEvents) {
      if (ev.status === 'completed' || ev.status === 'error' || ev.status === 'rejected') {
        newAnims.push({ id: ++animIdRef.current, toolName: ev.tool_name, success: ev.status === 'completed' && ev.success !== false });
      }
    }
    if (newAnims.length > 0) setAnimQueue(prev => [...prev, ...newAnims]);
  }, [toolEvents]);

  useEffect(() => {
    if (currentAnim || animQueue.length === 0) return;
    setCurrentAnim(animQueue[0]);
    setAnimQueue(prev => prev.slice(1));
  }, [currentAnim, animQueue]);

  useEffect(() => {
    if (!currentAnim) return;
    animTimerRef.current = setTimeout(() => setCurrentAnim(null), 6000);
    return () => { if (animTimerRef.current) clearTimeout(animTimerRef.current); };
  }, [currentAnim]);

  useEffect(() => {
    if (!toolEvents || toolEvents.length === 0) {
      prevEventCountRef.current = 0;
      setCurrentAnim(null);
      setAnimQueue([]);
      if (animTimerRef.current) clearTimeout(animTimerRef.current);
    }
  }, [toolEvents]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const cachedRef = useRef<ToolDefinition[] | null>(null);
  const cacheTimeRef = useRef<number>(0);

  const fetchTools = useCallback((force = false) => {
    if (!force && cachedRef.current && Date.now() - cacheTimeRef.current < 60_000) {
      setItems(cachedRef.current);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setSearch('');
    fetch(`${baseUrl}/tools`).then(r => r.json()).then(data => {
      if (cancelled) return;
      const tools: ToolDefinition[] = data.tools ?? [];
      setItems(tools);
      if (tools.length > 0) { cachedRef.current = tools; cacheTimeRef.current = Date.now(); }
    }).catch(() => { if (!cancelled) setItems([]); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [baseUrl]);

  useEffect(() => { if (open) fetchTools(); }, [open, fetchTools]);
  useEffect(() => { if (open && searchRef.current) searchRef.current.focus(); }, [open]);

  const filtered = useMemo(() => {
    if (!search.trim()) return items;
    const q = search.toLowerCase();
    return items.filter(item => item.name.toLowerCase().includes(q) || item.description.toLowerCase().includes(q));
  }, [items, search]);

  const handleSelect = (item: ToolDefinition) => {
    const params = getAllParams(item);
    if (params.length === 0) { onSelect(`[[tool:${item.name}]]`); setOpen(false); }
    else { setSelectedTool(item); setParamValues({}); }
  };

  const handleParamSubmit = () => {
    if (!selectedTool) return;
    const args: Record<string, string> = {};
    for (const [key, val] of Object.entries(paramValues)) { if (val.trim()) args[key] = val.trim(); }
    onSelect(Object.keys(args).length === 0 ? `[[tool:${selectedTool.name}]]` : `[[tool:${selectedTool.name}|${JSON.stringify(args)}]]`);
    setSelectedTool(null); setParamValues({}); setOpen(false);
  };

  const handleDirectExecute = () => {
    if (!selectedTool) return;
    const args: Record<string, string> = {};
    for (const [key, val] of Object.entries(paramValues)) { if (val.trim()) args[key] = val.trim(); }
    const syntax = Object.keys(args).length > 0 ? `[[tool:${selectedTool.name}|${JSON.stringify(args)}]]` : `[[tool:${selectedTool.name}]]`;
    setSelectedTool(null); setParamValues({}); setOpen(false);
    if (onDirectExecute) onDirectExecute(syntax); else onSelect(syntax);
  };

  const handleParamBack = () => { setSelectedTool(null); setParamValues({}); };

  const setParamValue = useCallback((name: string, val: string) => {
    setParamValues(prev => {
      const next = { ...prev, [name]: val };
      if (selectedTool) {
        const deps = TOOL_PARAM_DEPS[selectedTool.name];
        if (deps?.[name]) {
          const clearDescendants = (parent: string) => {
            const rules = deps[parent];
            if (!rules) return;
            for (const [, children] of Object.entries(rules)) {
              for (const child of children) { delete next[child]; clearDescendants(child); }
            }
          };
          clearDescendants(name);
        }
      }
      return next;
    });
  }, [selectedTool]);

  const hasAnim = !!currentAnim;
  const allParams = selectedTool ? getAllParams(selectedTool) : [];
  const visibleParams = selectedTool ? getVisibleParams(allParams, selectedTool.name, paramValues) : [];

  return (
    <div className="tool-selector" ref={ref}>
      <LgGlassInteractive>
        <button type="button"
          className={`tool-selector-trigger${open ? ' active' : ''}${hasAnim ? (currentAnim!.success ? ' tool-trigger-success' : ' tool-trigger-error') : ''}`}
          onClick={() => setOpen(v => !v)} title="工具调用">
          <span className={`tool-trigger-default${hasAnim ? ' hiding' : ''}`}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="4 17 10 11 4 5" /><line x1="12" y1="19" x2="20" y2="19" />
            </svg>
            <span className="tool-selector-label">工具</span>
          </span>
          <span className={`tool-trigger-status-wrap${hasAnim ? ' showing' : ''}`}>
            <span className="tool-trigger-icon">
              {currentAnim?.success ? (
                <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M4 7.2L6 9.2L10 5.2" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" /></svg>
              ) : (
                <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M5 5L9 9M9 5L5 9" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /></svg>
              )}
            </span>
            <span className="tool-trigger-name">{currentAnim ? abbreviateToolName(currentAnim.toolName) : ''}</span>
            <span className="tool-trigger-label">{currentAnim?.success ? '成功' : '失败'}</span>
          </span>
        </button>
      </LgGlassInteractive>

      {open && (
        <div className="tool-selector-panel">
          <div className="tool-panel-header">
            <span className="tool-panel-title">
              {selectedTool ? (
                <span>
                  <button type="button" className="tool-panel-back" onClick={handleParamBack}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="15 18 9 12 15 6" /></svg>
                  </button>
                  {selectedTool.name}
                </span>
              ) : '工具调用'}
            </span>
            <div className="tool-panel-header-right">
              {!selectedTool && (
                <>
                  <span className="tool-panel-count">{items.length > 0 ? `${filtered.length} / ${items.length}` : ''}</span>
                  <button type="button" className="tool-panel-refresh" onClick={() => fetchTools(true)} title="刷新工具列表" disabled={loading}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="23 4 23 10 17 10" /><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
                    </svg>
                  </button>
                </>
              )}
            </div>
          </div>

          {selectedTool ? (
            <div className="tool-param-form">
              {selectedTool.description && <div className="tool-param-desc">{selectedTool.description}</div>}
              {visibleParams.map(param => (
                <ParamInput key={param.name} param={param} value={paramValues[param.name] ?? ''}
                  onChange={(val) => setParamValue(param.name, val)}
                  contextReq={isContextRequired(selectedTool.name, param.name, paramValues)} />
              ))}
              {visibleParams.length === 0 && <div className="tool-param-empty">请先选择上方选项以显示参数</div>}
              <div className="tool-param-actions">
                <button type="button" className="tool-param-submit" onClick={handleParamSubmit}>插入调用</button>
                <button type="button" className="tool-param-execute" onClick={handleDirectExecute}>直接执行</button>
              </div>
            </div>
          ) : (
            <>
              {items.length > 0 && (
                <div className="tool-panel-search">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" />
                  </svg>
                  <input ref={searchRef} type="text" placeholder="搜索工具..." value={search} onChange={(e) => setSearch(e.target.value)} />
                </div>
              )}
              <div className="tool-panel-list">
                {loading ? (
                  <div className="tool-panel-empty"><div className="tool-panel-spinner" /><span>加载中...</span></div>
                ) : items.length === 0 ? (
                  <div className="tool-panel-empty">
                    <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
                    </svg>
                    <span className="tool-panel-empty-title">暂无可用工具</span>
                    <span className="tool-panel-empty-hint">在 tools/ 目录下添加 JSON 文件</span>
                  </div>
                ) : filtered.length === 0 ? (
                  <div className="tool-panel-empty">
                    <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                      <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" /><line x1="8" y1="11" x2="14" y2="11" />
                    </svg>
                    <span className="tool-panel-empty-title">未找到匹配工具</span>
                  </div>
                ) : (
                  filtered.map(item => (
                    <button key={item.name} className="tool-panel-item" onClick={() => handleSelect(item)}>
                      <div className="tool-item-icon">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <polyline points="4 17 10 11 4 5" /><line x1="12" y1="19" x2="20" y2="19" />
                        </svg>
                      </div>
                      <div className="tool-item-info">
                        <span className="tool-item-name">{item.name}</span>
                        {item.description && <span className="tool-item-desc">{item.description}</span>}
                      </div>
                      <svg className="tool-item-arrow" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                        <polyline points="9 18 15 12 9 6" />
                      </svg>
                    </button>
                  ))
                )}
              </div>
              <div className="tool-panel-footer"><span>点击插入，AI 会帮你填参数</span></div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
