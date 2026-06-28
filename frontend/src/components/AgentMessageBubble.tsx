import { useState, useEffect, useCallback, memo } from 'react';
import type { ChatMessage, AgentProgress } from '../api/types';
import { ThinkingIndicator } from './ThinkingIndicator';
import { MessageContentSegments } from './MessageContentSegments';
import { copyToClipboard, escapeHtml } from '../utils/clipboard';

interface Props {
  message: ChatMessage;
  questionId?: string;
  agentProgress?: AgentProgress[];
}

/** Extract a readable domain + path label from a URL */
function getSourceLabel(url: string): string {
  try {
    const u = new URL(url);
    const path = u.pathname.length > 1 ? u.pathname : '';
    return u.hostname + path;
  } catch {
    return url;
  }
}

function formatStreamElapsed(ms: number): string {
  const totalSec = ms / 1000;
  if (totalSec < 60) return `${totalSec.toFixed(1)}s`;
  const minutes = Math.floor(totalSec / 60);
  const secs = Math.floor(totalSec % 60);
  return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

export const AgentMessageBubble = memo(function AgentMessageBubble({ message, questionId, agentProgress }: Props) {
  const isThinking = message.isStreaming && !message.content;
  const isStreaming = message.isStreaming && !!message.content;
  const [copied, setCopied] = useState(false);
  const [thinkingCopied, setThinkingCopied] = useState(false);
  const [thinkingOpen, setThinkingOpen] = useState(false);
  const [subAgentLogOpen, setSubAgentLogOpen] = useState(false);
  const [openThinking, setOpenThinking] = useState<Set<number>>(new Set());
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const [streamElapsed, setStreamElapsed] = useState(0);

  useEffect(() => {
    if (!isStreaming) { setStreamElapsed(0); return; }
    const timer = setInterval(() => {
      setStreamElapsed(Date.now() - message.timestamp);
    }, 100);
    return () => clearInterval(timer);
  }, [isStreaming, message.timestamp]);

  const html = message.renderedHtml || (message.content ? escapeHtml(message.content).replace(/\n/g, '<br>') : '');

  const handleCopy = useCallback(async () => {
    if (!message.content) return;
    await copyToClipboard(message.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [message.content]);

  const handleThinkingCopy = useCallback(async () => {
    if (!message.thinking) return;
    await copyToClipboard(message.thinking);
    setThinkingCopied(true);
    setTimeout(() => setThinkingCopied(false), 1500);
  }, [message.thinking]);

  return (
    <div id={`msg-${message.id}`} className="message-row assistant">
      {isThinking ? (
        <ThinkingIndicator startTime={message.timestamp} agentProgress={agentProgress} />
      ) : (
        <div className="message-bubble assistant">
          {message.thinking && (
            <div className="thinking-section">
              <div className="thinking-header">
                <button
                  className="thinking-toggle"
                  onClick={() => setThinkingOpen(!thinkingOpen)}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="12" cy="12" r="10" />
                    <path d="M12 6v6l4 2" />
                  </svg>
                  模型思考过程
                  <svg className={`thinking-toggle-icon${thinkingOpen ? ' open' : ''}`} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <polyline points="6 9 12 15 18 9" />
                  </svg>
                </button>
                <span
                  className={`thinking-copy-btn${thinkingCopied ? ' copied' : ''}`}
                  onClick={handleThinkingCopy}
                  title="复制思考内容"
                >
                  {thinkingCopied ? (
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                  ) : (
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                    </svg>
                  )}
                </span>
              </div>
              <div
                className={`thinking-content-wrapper${thinkingOpen ? ' open' : ''}`}
                style={{ maxHeight: thinkingOpen ? '300px' : '0px' }}
              >
                <div className="thinking-content">
                  <span className="thinking-text">{message.thinking}</span>
                </div>
              </div>
            </div>
          )}
          {message.subAgentResults && message.subAgentResults.length > 0 && (
            <div className="sub-agent-section">
              <button
                className="sub-agent-toggle"
                onClick={() => setSubAgentLogOpen(!subAgentLogOpen)}
              >
                <span className="sub-agent-toggle-icon-wrap">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M12 8V4H8" /><rect x="2" y="2" width="8" height="8" rx="2" />
                    <path d="M16 12h4v4" /><rect x="14" y="14" width="8" height="8" rx="2" />
                    <path d="M10.5 13.5L13.5 10.5" />
                  </svg>
                </span>
                <span className="sub-agent-toggle-text">
                  SubAgent 协作分析
                  <span className="sub-agent-toggle-count">{message.subAgentResults.length}</span>
                </span>
                <svg className={`sub-agent-chevron${subAgentLogOpen ? ' open' : ''}`} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </button>
              <div className={`sub-agent-body${subAgentLogOpen ? ' open' : ''}`}>
                <div className="sub-agent-timeline">
                  {message.subAgentResults.map((result, i) => {
                    const q = result.quality;
                    const level = q >= 0.8 ? 'high' : q >= 0.5 ? 'medium' : 'low';
                    const pct = Math.round(q * 100);
                    const r = 16;
                    const circ = 2 * Math.PI * r;
                    const offset = circ * (1 - q);
                    return (
                      <div
                        key={i}
                        className={`sub-agent-card ${level}`}
                        style={{ animationDelay: `${i * 80}ms` }}
                      >
                        <div className="sub-agent-card-dot" />
                        {i < message.subAgentResults!.length - 1 && (
                          <div className="sub-agent-card-line" />
                        )}
                        <div className="sub-agent-card-inner">
                          <div className="sub-agent-card-head">
                            <span className="sub-agent-card-id">
                              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                                <rect x="3" y="11" width="18" height="10" rx="2" />
                                <circle cx="12" cy="16" r="1" />
                                <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                              </svg>
                              {result.agent_id}
                            </span>
                            <span className="sub-agent-card-ring-wrap" title={`质量 ${pct}%`}>
                              <svg className="sub-agent-card-ring" width="36" height="36" viewBox="0 0 36 36">
                                <circle className="sub-agent-ring-bg" cx="18" cy="18" r={r} />
                                <circle
                                  className="sub-agent-ring-fg"
                                  cx="18" cy="18" r={r}
                                  strokeDasharray={circ}
                                  strokeDashoffset={offset}
                                />
                              </svg>
                              <span className="sub-agent-card-ring-text">{pct}</span>
                            </span>
                          </div>
                          {result.thinking && (
                            <div className="sub-agent-card-details">
                              <button
                                className="sub-agent-card-summary"
                                onClick={() => setOpenThinking(prev => {
                                  const next = new Set(prev);
                                  next.has(i) ? next.delete(i) : next.add(i);
                                  return next;
                                })}
                              >
                                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                                  <circle cx="12" cy="12" r="10" /><path d="M12 6v6l4 2" />
                                </svg>
                                思考过程
                                <svg className={`sub-agent-thinking-chevron${openThinking.has(i) ? ' open' : ''}`} width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                  <polyline points="6 9 12 15 18 9" />
                                </svg>
                              </button>
                              <div className={`sub-agent-thinking-body${openThinking.has(i) ? ' open' : ''}`}>
                                <div className="sub-agent-card-thinking">{result.thinking}</div>
                              </div>
                            </div>
                          )}
                          <div className="sub-agent-card-content">
                            <span className="sub-agent-card-content-label">返回结果</span>
                            <span className="sub-agent-card-content-text">{result.content_summary}</span>
                          </div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          )}
          <MessageContentSegments html={html} isStreaming={message.isStreaming} />
          {message.stickerUrl && !isThinking && (
            <div className="sticker-container">
              <img src={message.stickerUrl} alt="表情包" className="sticker-img" loading="lazy" />
            </div>
          )}
          {message.httpSources && message.httpSources.length > 0 && (
            <div className="http-sources-section">
              <button
                className="http-sources-toggle"
                onClick={() => setSourcesOpen(!sourcesOpen)}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <circle cx="12" cy="12" r="10" />
                  <line x1="2" y1="12" x2="22" y2="12" />
                  <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
                </svg>
                <span className="http-sources-toggle-text">
                  参考来源
                  <span className="http-sources-toggle-count">{message.httpSources.length}</span>
                </span>
                <svg className={`http-sources-chevron${sourcesOpen ? ' open' : ''}`} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </button>
              <div className={`http-sources-body${sourcesOpen ? ' open' : ''}`}>
                <div className="http-sources-list">
                  {message.httpSources.map((src, i) => (
                    <a
                      key={i}
                      className="http-source-item"
                      href={src.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      title={src.url}
                    >
                      <span className="http-source-index">{i + 1}</span>
                      <span className="http-source-icon">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
                          <polyline points="15 3 21 3 21 9" />
                          <line x1="10" y1="14" x2="21" y2="3" />
                        </svg>
                      </span>
                      <span className="http-source-label">{src.title || getSourceLabel(src.url)}</span>
                    </a>
                  ))}
                </div>
              </div>
            </div>
          )}
          {isStreaming && (
            <span className="stream-timer">
              <span className="stream-timer-dot" />
              {formatStreamElapsed(streamElapsed)}
            </span>
          )}
        </div>
      )}
      <div className="message-footer">
        <span className="footer-time">
          {new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
        </span>
        {message.responseTimeMs != null && (
          <span className="footer-response-time">
            {message.responseTimeMs < 1000
              ? `${message.responseTimeMs}ms`
              : message.responseTimeMs < 60000
                ? `${(message.responseTimeMs / 1000).toFixed(1)}s`
                : `${Math.floor(message.responseTimeMs / 60000)}m${Math.round((message.responseTimeMs % 60000) / 1000)}s`}
          </span>
        )}
        <span className="footer-spacer" />
        {!isThinking && message.content && (
          <button
            className={`footer-btn${copied ? ' copied' : ''}`}
            onClick={handleCopy}
            title="复制"
          >
            {copied ? (
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <polyline points="20 6 9 17 4 12" />
              </svg>
            ) : (
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
              </svg>
            )}
            {copied ? '已复制' : '复制'}
          </button>
        )}
        {!isThinking && questionId && (
          <button
            className="footer-btn"
            onClick={() => {
              if (!questionId) return;
              const el = document.getElementById(`msg-${questionId}`);
              if (el) {
                el.scrollIntoView({ behavior: 'smooth', block: 'center' });
              }
            }}
            title="定位到提问"
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="12" y1="19" x2="12" y2="5" />
              <polyline points="5 12 12 5 19 12" />
            </svg>
            提问
          </button>
        )}
      </div>
    </div>
  );
});
