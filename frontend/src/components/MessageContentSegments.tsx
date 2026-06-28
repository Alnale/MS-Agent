import { memo, useRef, useEffect, lazy, Suspense, useState, useCallback } from 'react';
import { copyToClipboard } from '../utils/clipboard';

const MermaidDiagram = lazy(() => import('./MermaidDiagram').then(m => ({ default: m.MermaidDiagram })));
const HtmlPreview = lazy(() => import('./HtmlPreview'));

interface Props {
  html: string;
  isStreaming?: boolean;
}

/**
 * Splits rendered HTML by mermaid block markers and renders
 * HTML segments with dangerouslySetInnerHTML and mermaid blocks
 * as interactive MermaidDiagram components.
 */
export const MessageContentSegments = memo(function MessageContentSegments({ html, isStreaming }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [previewCode, setPreviewCode] = useState<string | null>(null);

  // Event delegation for copy buttons and preview buttons
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleClick = async (e: Event) => {
      const target = e.target as HTMLElement;

      // Preview button
      const previewBtn = target.closest('[data-preview-btn]');
      if (previewBtn) {
        e.preventDefault();
        e.stopPropagation();
        const wrapper = previewBtn.closest('[data-copy-content]') as HTMLElement;
        if (wrapper) {
          const code = wrapper.getAttribute('data-copy-content');
          if (code) {
            const decoded = code
              .replace(/&amp;/g, '&')
              .replace(/&lt;/g, '<')
              .replace(/&gt;/g, '>')
              .replace(/&quot;/g, '"')
              .replace(/&#39;/g, "'");
            setPreviewCode(decoded);
          }
        }
        return;
      }

      // Copy button
      const copyBtn = target.closest('[data-copy-btn]');
      if (copyBtn) {
        e.preventDefault();
        e.stopPropagation();
        const wrapper = copyBtn.closest('[data-copy-content]') as HTMLElement;
        if (!wrapper) return;
        const content = wrapper.getAttribute('data-copy-content');
        if (!content) return;
        const ok = await copyToClipboard(content);
        if (ok) {
          copyBtn.textContent = '✓ 已复制';
          copyBtn.classList.add('copied');
          setTimeout(() => {
            copyBtn.textContent = '📋 复制';
            copyBtn.classList.remove('copied');
          }, 1500);
        }
      }
    };

    container.addEventListener('click', handleClick);
    return () => container.removeEventListener('click', handleClick);
  }, [html]);

  const closePreview = useCallback(() => setPreviewCode(null), []);

  if (!html) {
    return <div className={`message-content${isStreaming ? ' streaming' : ''}`}>...</div>;
  }

  // Split by mermaid block markers
  const mermaidRegex = /<div class="mermaid-block" data-mermaid-id="([^"]+)">([\s\S]*?)<\/div>/g;
  const segments: Array<{ type: 'html' | 'mermaid'; content: string; id?: string }> = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = mermaidRegex.exec(html)) !== null) {
    if (match.index > lastIndex) {
      segments.push({ type: 'html', content: html.slice(lastIndex, match.index) });
    }
    const mermaidCode = match[2]
      .replace(/&amp;/g, '&')
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&quot;/g, '"');
    segments.push({ type: 'mermaid', content: mermaidCode, id: match[1] });
    lastIndex = match.index + match[0].length;
  }
  if (lastIndex < html.length) {
    segments.push({ type: 'html', content: html.slice(lastIndex) });
  }

  // If no mermaid blocks, render as plain HTML
  if (segments.length === 0 || (segments.length === 1 && segments[0].type === 'html')) {
    return (
      <>
        <div
          ref={containerRef}
          className={`message-content${isStreaming ? ' streaming' : ''}`}
          dangerouslySetInnerHTML={{ __html: html }}
        />
        {previewCode && (
          <Suspense fallback={null}>
            <HtmlPreview code={previewCode} onClose={closePreview} />
          </Suspense>
        )}
      </>
    );
  }

  return (
    <>
      <div ref={containerRef} className={`message-content${isStreaming ? ' streaming' : ''}`}>
        {segments.map((seg, i) => {
          if (seg.type === 'mermaid') {
            return (
              <Suspense key={seg.id || i} fallback={<div className="mermaid-loading">Loading diagram...</div>}>
                <MermaidDiagram code={seg.content} id={seg.id || `mermaid-${i}`} />
              </Suspense>
            );
          }
          return (
            <span key={i} dangerouslySetInnerHTML={{ __html: seg.content }} />
          );
        })}
      </div>
      {previewCode && (
        <Suspense fallback={null}>
          <HtmlPreview code={previewCode} onClose={closePreview} />
        </Suspense>
      )}
    </>
  );
});
