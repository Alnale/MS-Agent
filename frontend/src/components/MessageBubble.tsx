import { useState, useCallback, useMemo, memo } from 'react';
import type { ChatMessage, AgentProgress } from '../api/types';
import { AgentMessageBubble } from './AgentMessageBubble';
import { copyToClipboard, escapeHtml } from '../utils/clipboard';

interface Props {
  message: ChatMessage;
  onResend?: (content: string, forceResend?: boolean) => void;
  questionId?: string;
  agentProgress?: AgentProgress[];
}

export const MessageBubble = memo(function MessageBubble({ message, onResend, questionId, agentProgress }: Props) {
  const isUser = message.role === 'user';
  const [copied, setCopied] = useState(false);

  // For assistant messages, delegate to AgentMessageBubble
  if (!isUser) {
    return <AgentMessageBubble message={message} questionId={questionId} agentProgress={agentProgress} />;
  }

  // User messages: always escape from raw content (never use renderedHtml which is for assistant messages)
  const html = useMemo(() => message.content ? escapeHtml(message.content).replace(/\n/g, '<br>') : '', [message.content]);

  const handleCopy = useCallback(async () => {
    if (!message.content) return;
    await copyToClipboard(message.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [message.content]);

  return (
    <div id={`msg-${message.id}`} className="message-row user">
      <div className="message-bubble user">
        <div
          className={`message-content${message.isStreaming ? ' streaming' : ''}`}
          dangerouslySetInnerHTML={{ __html: html || '...' }}
        />
      </div>
      <div className="message-footer user-footer">
        {message.content && onResend && (
          <button
            className="copy-btn"
            onClick={() => onResend(message.content, true)}
            title="重新发送"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="23 4 23 10 17 10" />
              <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
            </svg>
            重发
          </button>
        )}
        {message.content && (
          <button
            className={`copy-btn${copied ? ' copied' : ''}`}
            onClick={handleCopy}
            title="复制"
          >
            {copied ? (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <polyline points="20 6 9 17 4 12" />
              </svg>
            ) : (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
              </svg>
            )}
            {copied ? '已复制' : '复制'}
          </button>
        )}
        <div className="message-time user-time">
          {new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
        </div>
      </div>
    </div>
  );
});
