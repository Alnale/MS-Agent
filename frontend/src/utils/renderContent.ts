import katex from 'katex';
import { escapeHtml } from './clipboard';

function renderLatex(latex: string, displayMode: boolean): string {
  try {
    return katex.renderToString(latex, {
      displayMode,
      throwOnError: false,
      trust: false,
      strict: false,
    });
  } catch {
    return escapeHtml(displayMode ? `$$${latex}$$` : `$${latex}$`);
  }
}

function sanitizeUrl(url: string): string {
  const trimmed = url.trim().toLowerCase().replace(/[\s\x00-\x1f]+/g, '');
  if (/^(javascript|data|vbscript|jar):/.test(trimmed)) {
    return '#';
  }
  return escapeHtml(url);
}

function renderInlineMarkdown(text: string): string {
  let html = escapeHtml(text);

  // Inline code
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>');

  // Bold
  html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');

  // Italic
  html = html.replace(/(?<!\*)\*(?!\*)(.+?)(?<!\*)\*(?!\*)/g, '<em>$1</em>');

  // Links (with URL sanitization to prevent javascript: XSS)
  html = html.replace(
    /\[([^\]]+)\]\(([^)]+)\)/g,
    (_match, text, url) => {
      const safeUrl = sanitizeUrl(url);
      return `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer">${text}</a>`;
    }
  );

  return html;
}

/** Split a markdown table row into cells, preserving empty cells */
function splitTableRow(line: string): string[] {
  const trimmed = line.trim();
  const stripped = trimmed.startsWith('|') ? trimmed.slice(1) : trimmed;
  const inner = stripped.endsWith('|') ? stripped.slice(0, -1) : stripped;
  return inner.split('|').map(c => c.trim());
}

/** Parse a markdown table block into HTML table */
function renderTable(tableBlock: string): string {
  const lines = tableBlock.trim().split('\n').filter(l => l.trim());
  if (lines.length < 2) return renderInlineMarkdown(tableBlock);

  const headerCells = splitTableRow(lines[0]);
  const separatorLine = lines[1].trim();
  if (!/^\|?[\s\-:|]+\|?$/.test(separatorLine)) {
    return renderInlineMarkdown(tableBlock);
  }

  const alignments = splitTableRow(separatorLine).map(c => {
    if (c.startsWith(':') && c.endsWith(':')) return 'center';
    if (c.endsWith(':')) return 'right';
    return 'left';
  });

  const bodyRows = lines.slice(2).map(line => splitTableRow(line));
  const rawMarkdown = tableBlock.trim();

  let html = `<div class="table-wrapper" data-copy-content="${escapeHtml(rawMarkdown)}">`;
  html += '<div class="table-toolbar"><span class="table-label">Table</span>';
  html += '<button class="block-copy-btn" data-copy-btn title="复制表格">📋 复制</button></div>';
  html += '<table class="md-table"><thead><tr>';
  headerCells.forEach((cell, i) => {
    const align = alignments[i] ? ` style="text-align:${alignments[i]}"` : '';
    html += `<th${align}>${renderInlineMarkdown(cell)}</th>`;
  });
  html += '</tr></thead><tbody>';
  bodyRows.forEach(row => {
    html += '<tr>';
    row.forEach((cell, i) => {
      const align = alignments[i] ? ` style="text-align:${alignments[i]}"` : '';
      html += `<td${align}>${renderInlineMarkdown(cell)}</td>`;
    });
    for (let i = row.length; i < headerCells.length; i++) {
      html += '<td></td>';
    }
    html += '</tr>';
  });
  html += '</tbody></table></div>';
  return html;
}

/**
 * Extract markdown tables from text line-by-line.
 * Returns segments where table segments contain complete table markdown
 * and text segments contain everything else.
 */
function extractTableBlocks(text: string): { type: 'table' | 'text'; content: string }[] {
  const lines = text.split('\n');
  const segments: { type: 'table' | 'text'; content: string }[] = [];
  let currentText: string[] = [];

  const flushText = () => {
    if (currentText.length > 0) {
      segments.push({ type: 'text', content: currentText.join('\n') });
      currentText = [];
    }
  };

  let i = 0;
  while (i < lines.length) {
    // Look for table: header(i) + separator(i+1) + body rows(i+2..)
    if (
      i + 1 < lines.length &&
      lines[i].includes('|') &&
      /^\|?[\s\-:]+\|[\s\-:|]*$/.test(lines[i + 1].trim())
    ) {
      // Count separator columns to bound the table
      const sepColCount = splitTableRow(lines[i + 1]).length;
      flushText();

      // Collect table lines: header + separator + body rows
      const tableLines = [lines[i], lines[i + 1]];
      let j = i + 2;
      while (
        j < lines.length &&
        lines[j].includes('|') &&
        splitTableRow(lines[j]).length <= sepColCount + 2
      ) {
        tableLines.push(lines[j]);
        j++;
      }

      segments.push({ type: 'table', content: tableLines.join('\n') });
      i = j;
    } else {
      currentText.push(lines[i]);
      i++;
    }
  }

  flushText();
  return segments;
}

/** Counter for unique mermaid block IDs */
let mermaidCounter = 0;

export function renderContent(content: string): string {
  if (!content) return '';

  // Defensive: strip any tool tags that may have leaked from backend
  let cleaned = content
    .replace(/\[\[tool:[^\]]*\]\]/g, '')
    .replace(/\[\[tool:[^\]]*$/g, '');

  const parts: string[] = [];

  interface Segment {
    text: string;
    latex?: string;
    displayMode?: boolean;
    codeBlock?: boolean;
    lang?: string;
  }

  // Phase 0: Split by fenced code blocks (highest priority, no inner parsing)
  const codeBlockRegex = /```(\w*)\n?([\s\S]*?)```/g;
  const preSegments: Segment[] = [];
  let preLastIndex = 0;
  let preMatch: RegExpExecArray | null;

  while ((preMatch = codeBlockRegex.exec(cleaned)) !== null) {
    if (preMatch.index > preLastIndex) {
      preSegments.push({ text: cleaned.slice(preLastIndex, preMatch.index) });
    }
    preSegments.push({ text: preMatch[0], codeBlock: true, lang: preMatch[1] || undefined, latex: preMatch[2] });
    preLastIndex = preMatch.index + preMatch[0].length;
  }
  if (preLastIndex < cleaned.length) {
    preSegments.push({ text: cleaned.slice(preLastIndex) });
  }
  if (preSegments.length === 0) {
    preSegments.push({ text: cleaned });
  }

  for (const preSeg of preSegments) {
    if (preSeg.codeBlock) {
      if (preSeg.lang === 'mermaid') {
        const id = `mermaid-${++mermaidCounter}-${Date.now()}`;
        const encoded = preSeg.latex!.trim();
        parts.push(
          `<div class="mermaid-block" data-mermaid-id="${id}">${escapeHtml(encoded)}</div>`
        );
        continue;
      }
      const lang = preSeg.lang ? ` class="language-${preSeg.lang}"` : '';
      const rawCode = preSeg.latex!.replace(/\n$/, '');
      const codeLabel = preSeg.lang || 'Code';
      const isHtml = preSeg.lang === 'html' || preSeg.lang === 'htm';
      const previewBtn = isHtml
        ? `<button class="block-preview-btn" data-preview-btn title="预览HTML">▶ 预览</button>`
        : '';
      parts.push(
        `<div class="code-block-wrapper" data-copy-content="${escapeHtml(rawCode)}">` +
        `<div class="code-toolbar"><span class="code-lang">${escapeHtml(codeLabel)}</span>` +
        `<div class="code-toolbar-actions">${previewBtn}` +
        `<button class="block-copy-btn" data-copy-btn title="复制代码">📋 复制</button></div></div>` +
        `<pre class="code-block"><code${lang}>${escapeHtml(rawCode)}</code></pre></div>`
      );
      continue;
    }

    // Phase 1: Split by block-level LaTeX: $$...$$
    const blockRegex = /\$\$([\s\S]+?)\$\$/g;
    let lastIndex = 0;
    let match: RegExpExecArray | null;
    const segments: Segment[] = [];

    while ((match = blockRegex.exec(preSeg.text)) !== null) {
      if (match.index > lastIndex) {
        segments.push({ text: preSeg.text.slice(lastIndex, match.index) });
      }
      segments.push({ text: match[0], latex: match[1], displayMode: true });
      lastIndex = match.index + match[0].length;
    }
    if (lastIndex < preSeg.text.length) {
      segments.push({ text: preSeg.text.slice(lastIndex) });
    }
    if (segments.length === 0) {
      segments.push({ text: preSeg.text });
    }

    for (const seg of segments) {
      if (seg.latex !== undefined && seg.displayMode) {
        parts.push(`<div class="math-block">${renderLatex(seg.latex, true)}</div>`);
      } else {
        // Phase 2: Extract and render tables (line-by-line scan, works without blank-line separation)
        const tableBlocks = extractTableBlocks(seg.text);
        for (const block of tableBlocks) {
          if (block.type === 'table') {
            parts.push(renderTable(block.content));
          } else {
            // Phase 3: Process inline LaTeX: $...$
            const inlineRegex = /\$([^\$\n]+?)\$/g;
            let inlineLastIndex = 0;
            let inlineMatch: RegExpExecArray | null;
            const text = block.content;

            while ((inlineMatch = inlineRegex.exec(text)) !== null) {
              if (inlineMatch.index > inlineLastIndex) {
                parts.push(renderInlineMarkdown(text.slice(inlineLastIndex, inlineMatch.index)));
              }
              parts.push(`<span class="math-inline">${renderLatex(inlineMatch[1], false)}</span>`);
              inlineLastIndex = inlineMatch.index + inlineMatch[0].length;
            }
            if (inlineLastIndex < text.length) {
              parts.push(renderInlineMarkdown(text.slice(inlineLastIndex)));
            }
          }
        }
      }
    }
  }

  // Convert newlines to <br> (but not inside math/code/table/mermaid blocks)
  return parts
    .map((p) => {
      if (p.includes('math-block') || p.includes('math-inline') || p.includes('code-block') || p.includes('table-wrapper') || p.includes('mermaid-block')) return p;
      return p.replace(/\n/g, '<br>');
    })
    .join('');
}
