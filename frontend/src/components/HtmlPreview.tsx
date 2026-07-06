import { useEffect, useCallback, useState, useRef, useMemo, memo } from 'react';
import { sanitize } from '../utils/sanitizer';

interface Props {
  code: string;
  onClose: () => void;
}

const HtmlPreview = memo(function HtmlPreview({ code, onClose }: Props) {
  const [closing, setClosing] = useState(false);
  const [done, setDone] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [spinning, setSpinning] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    document.body.classList.add('html-preview-open');
    return () => document.body.classList.remove('html-preview-open');
  }, []);

  const handleClose = useCallback(() => {
    setClosing(true);
  }, []);

  const handleRefresh = useCallback(() => {
    setSpinning(true);
    setRefreshKey(k => k + 1);
    setTimeout(() => setSpinning(false), 500);
  }, []);

  const handleOpenExternal = useCallback(() => {
    const blob = new Blob([code], { type: 'text/html' });
    const url = URL.createObjectURL(blob);
    window.open(url, '_blank');
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  }, [code]);

  useEffect(() => {
    const el = panelRef.current;
    if (!el) return;
    if (closing) {
      const onEnd = () => onClose();
      el.addEventListener('animationend', onEnd, { once: true });
      return () => el.removeEventListener('animationend', onEnd);
    }
    const onEnterEnd = () => setDone(true);
    el.addEventListener('animationend', onEnterEnd, { once: true });
    return () => el.removeEventListener('animationend', onEnterEnd);
  }, [closing, onClose]);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') handleClose();
  }, [handleClose]);

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  // Sanitize HTML to prevent XSS in preview iframe
  const sanitizedCode = useMemo(() => sanitize(code), [code]);

  return (
    <>
      <div className={`html-preview-backdrop${closing ? ' closing' : ''}`} onClick={handleClose} />
      <div ref={panelRef} className={`html-preview-panel${closing ? ' closing' : ''}${done ? ' done' : ''}`}>
        <div className="html-preview-header">
          <div className="html-preview-drag-hint">
            <button
              className="html-preview-dot red"
              onClick={handleClose}
              title="关闭 (Esc)"
            />
            <button
              className={`html-preview-dot yellow${spinning ? ' spin' : ''}`}
              onClick={handleRefresh}
              title="刷新"
            />
            <button
              className="html-preview-dot green"
              onClick={handleOpenExternal}
              title="在新窗口打开"
            />
          </div>
          <span className="html-preview-title">HTML Preview</span>
        </div>
        <div className="html-preview-body">
          <iframe
            key={refreshKey}
            className="html-preview-iframe"
            sandbox="allow-same-origin"
            srcDoc={sanitizedCode}
            title="HTML Preview"
          />
        </div>
      </div>
    </>
  );
});

export default HtmlPreview;
