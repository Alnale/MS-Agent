import { useEffect, useRef, useState, useCallback, memo } from 'react';
import mermaid from 'mermaid';
import { sanitizeSvg } from '../utils/sanitizer';
import { copyToClipboard } from '../utils/clipboard';

// Initialize mermaid once with a consistent config
let mermaidInitialized = false;
function initMermaid() {
  if (mermaidInitialized) return;
  mermaid.initialize({
    startOnLoad: false,
    theme: 'base',
    themeVariables: {
      primaryColor: '#f2a0b0',
      primaryTextColor: '#1a1a2e',
      primaryBorderColor: '#e8899e',
      lineColor: '#b8a9e8',
      secondaryColor: '#d8cef4',
      tertiaryColor: '#f5f0fa',
      noteBkgColor: '#fdf8f5',
      noteTextColor: '#1a1a2e',
      fontFamily: "'Inter', -apple-system, sans-serif",
      fontSize: '13px',
    },
    flowchart: {
      htmlLabels: true,
      curve: 'basis',
      padding: 16,
    },
    sequence: { mirrorActors: false, messageAlign: 'center' },
    gantt: { fontSize: 12 },
  });
  mermaidInitialized = true;
}

interface Props {
  code: string;
  id: string;
}

const CopyIcon = () => (
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
    <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  </svg>
);

const CheckIcon = () => (
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
    <polyline points="20 6 9 17 4 12" />
  </svg>
);

export const MermaidDiagram = memo(function MermaidDiagram({ code, id }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [svg, setSvg] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    initMermaid();
    let cancelled = false;

    async function render() {
      try {
        const { svg: renderedSvg } = await mermaid.render(id, code);
        if (!cancelled) {
          setSvg(sanitizeSvg(renderedSvg));
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          const msg = err instanceof Error ? err.message : 'Failed to render diagram';
          setError(msg);
          setSvg(null);
        }
      }
    }

    render();
    return () => { cancelled = true; };
  }, [code, id]);

  const handleCopy = useCallback(async () => {
    await copyToClipboard(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [code]);

  if (error) {
    return (
      <div className="mermaid-error">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="12" cy="12" r="10" />
          <line x1="15" y1="9" x2="9" y2="15" />
          <line x1="9" y1="9" x2="15" y2="15" />
        </svg>
        <span>Diagram error: {error}</span>
        <pre className="mermaid-error-code">{code}</pre>
      </div>
    );
  }

  if (!svg) {
    return (
      <div className="mermaid-loading">
        <div className="mermaid-spinner" />
        <span>Rendering diagram...</span>
      </div>
    );
  }

  return (
    <div className="mermaid-container">
      <div className="mermaid-toolbar">
        <span className="mermaid-label">Mermaid</span>
        <button
          className={`block-copy-btn${copied ? ' copied' : ''}`}
          onClick={handleCopy}
          title="复制代码"
        >
          {copied ? <CheckIcon /> : <CopyIcon />}
          {copied ? '已复制' : '复制'}
        </button>
      </div>
      <div
        ref={containerRef}
        className="mermaid-diagram"
        dangerouslySetInnerHTML={{ __html: svg }}
      />
    </div>
  );
});
