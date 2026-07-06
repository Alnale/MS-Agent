import { useState, useRef, useCallback, useEffect } from 'react';
import { AgentTeamsClient } from '../api/client';
import type { ChatMessage, ChatRequest, ToolStatusEvent, SubAgentResultSummary, AgentProgress, CompanionState } from '../api/types';
import type { Session } from './useSession';
import { renderContent } from '../utils/renderContent';

const nextId = () => crypto.randomUUID();

/** Extract [[tool:name]] from message content, returns tool name or null */
function extractForceTool(content: string): string | null {
  const match = content.match(/\[\[tool:(\w+)\]\]/);
  return match ? match[1] : null;
}

interface UseChatOptions {
  baseUrl?: string;
  session?: Session | null;
  onMessagesChange?: (messages: ChatMessage[]) => void;
  isNewSessionRef?: React.RefObject<boolean>;
  getSystemInstructions?: (sessionId: string) => string[];
  onToolResult?: (event: ToolStatusEvent) => void;
  companionMode?: boolean;
}

export function useChat(options: UseChatOptions = {}) {
  const clientRef = useRef(new AgentTeamsClient(options.baseUrl));
  const sessionIdRef = useRef<string>(crypto.randomUUID());

  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const messagesRef = useRef<ChatMessage[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const isStreamingRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [toolEvents, setToolEvents] = useState<ToolStatusEvent[]>([]);
  const [agentProgress, setAgentProgress] = useState<AgentProgress[]>([]);
  const [companionState, setCompanionState] = useState<CompanionState | null>(null);
  const streamingMsgIdRef = useRef<string | null>(null);

  // Keep ref in sync with state
  messagesRef.current = messages;

  // Keep onToolResult in a ref to avoid stale closures during streaming
  const onToolResultRef = useRef(options.onToolResult);
  onToolResultRef.current = options.onToolResult;

  // Only reset when session ID actually changes, not on every session object reference change.
  // This prevents a cascade: setMessages → onMessagesChange → saveSession → sessions update
  // → new loadSession reference → new currentSession → this effect re-fires → setMessages.
  const prevSessionIdRef = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    const newId = options.session?.id ?? null;
    if (prevSessionIdRef.current === newId) return;
    prevSessionIdRef.current = newId;

    if (options.session) {
      sessionIdRef.current = options.session.id;
      if (options.session.messages.length > 0) {
        setMessages(options.session.messages);
      } else if (options.isNewSessionRef?.current) {
        // Don't clear messages for newly created sessions where messages were
        // already added optimistically by sendMessage
        options.isNewSessionRef.current = false;
      } else {
        // Clear messages when switching to an existing empty session
        setMessages([]);
      }
    } else {
      setMessages([]);
      sessionIdRef.current = crypto.randomUUID();
    }
  }, [options.session?.id]);

  useEffect(() => {
    options.onMessagesChange?.(messages);
  }, [messages, options.onMessagesChange]);

  // Listen for companion mode proactive messages
  useEffect(() => {
    const handler = (e: Event) => {
      const msg = (e as CustomEvent).detail as ChatMessage;
      if (msg && msg.role === 'assistant') {
        setMessages(prev => [...prev, msg]);
      }
    };
    window.addEventListener('companion-message', handler);
    return () => window.removeEventListener('companion-message', handler);
  }, []);

  const sendMessage = useCallback(async (content: string, forceResend = false) => {
    if (!content.trim()) return;
    if (isStreamingRef.current && !forceResend) return;

    setError(null);

    const userMsg: ChatMessage = {
      id: nextId(),
      role: 'user',
      content: content.trim(),
      timestamp: Date.now(),
    };

    // Show thinking skeleton immediately
    const streamStartTime = Date.now();
    const assistantMsg: ChatMessage = {
      id: nextId(),
      role: 'assistant',
      content: '',
      timestamp: streamStartTime,
      isStreaming: true,
    };

    setMessages(prev => [...prev, userMsg, assistantMsg]);
    setIsStreaming(true);
    isStreamingRef.current = true;
    streamingMsgIdRef.current = assistantMsg.id;

    // All messages go through /chat — unified path
    // Extract [[tool:name]] hint and pass as force_tool to backend
    const forceTool = extractForceTool(content.trim());

    const history = messagesRef.current
      .filter(m => m.content && m.content.trim().length > 0 && !m.isStreaming)
      .map(m => ({
        sender_type: m.role === 'assistant' ? 'assistant' : 'user',
        content: m.content,
      }));
    const input: ChatRequest = {
      session_id: sessionIdRef.current,
      message: content.trim(),
      recent_history: history,
      stream_mode: 'full',
      force_tool: forceTool ?? undefined,
      system_instructions: options.getSystemInstructions
        ? options.getSystemInstructions(sessionIdRef.current)
        : undefined,
      companion_mode: options.companionMode || undefined,
    };

    try {
      let accumulated = '';
      let thinkingContent = '';
      let subAgentResults: SubAgentResultSummary[] | undefined;
      let stickerUrl: string | undefined;

      // Reset tool events and progress for this turn
      setToolEvents([]);
      setAgentProgress([]);

      // Throttled UI update: batch SSE chunks and flush every 33ms (~30fps)
      let flushTimer: ReturnType<typeof setTimeout> | null = null;
      let dirty = false;
      const flushUI = () => {
        if (!dirty) return;
        dirty = false;
        setMessages(prev =>
          prev.map(m =>
            m.id === assistantMsg.id
              ? { ...m, content: accumulated, thinking: thinkingContent || undefined }
              : m
          )
        );
      };

      for await (const chunk of clientRef.current.chat(input)) {
        if (chunk.type === 'error') {
          if (flushTimer) clearTimeout(flushTimer);
          throw new Error(chunk.delta || 'Stream error');
        }

        // Handle tool status events
        if (chunk.type === 'tool_status' && chunk.tool_status) {
          setToolEvents(prev => [...prev, chunk.tool_status!]);
          if (chunk.tool_status.status === 'completed' && onToolResultRef.current) {
            onToolResultRef.current(chunk.tool_status);
          }
          continue;
        }

        // Handle real-time agent progress events
        if (chunk.type === 'agent_progress' && chunk.agent_progress) {
          setAgentProgress(prev => [...prev, chunk.agent_progress!]);
          continue;
        }

        // Handle companion state updates
        if (chunk.type === 'companion_state' && chunk.companion_state) {
          setCompanionState(chunk.companion_state);
          // Attach companion state to the current streaming message
          setMessages(prev =>
            prev.map(m =>
              m.id === assistantMsg.id
                ? { ...m, companionState: chunk.companion_state }
                : m
            )
          );
          continue;
        }

        // Handle SubAgent result summaries
        if (chunk.type === 'sub_agent_results' && chunk.sub_agent_results) {
          subAgentResults = chunk.sub_agent_results;
          // Extract sticker from sentiment agent result
          const sentimentResult = subAgentResults.find(r => r.agent_id === 'sentiment' && r.sticker);
          if (sentimentResult?.sticker) {
            stickerUrl = `/gif/${sentimentResult.sticker}`;
          }
          setMessages(prev =>
            prev.map(m =>
              m.id === assistantMsg.id
                ? { ...m, subAgentResults: subAgentResults, stickerUrl: stickerUrl || undefined }
                : m
            )
          );
          continue;
        }

        // Handle annotations (e.g., web search citations)
        if (chunk.annotations && chunk.annotations.length > 0) {
          setMessages(prev =>
            prev.map(m =>
              m.id === assistantMsg.id
                ? { ...m, annotations: [...(m.annotations || []), ...chunk.annotations!] }
                : m
            )
          );
          continue;
        }

        if (chunk.delta) {
          accumulated += chunk.delta;
        }
        if (chunk.thinking_delta) {
          thinkingContent += chunk.thinking_delta;
        }

        dirty = true;
        if (!flushTimer) {
          flushTimer = setTimeout(() => {
            flushTimer = null;
            flushUI();
          }, 33);
        }

        if (chunk.done) break;
      }

      // Final flush
      if (flushTimer) clearTimeout(flushTimer);
      flushUI();

      const finalContent = accumulated;
      const finalThinking = thinkingContent || undefined;
      const responseTimeMs = Date.now() - streamStartTime;
      setMessages(prev =>
        prev.map(m =>
          m.id === assistantMsg.id
            ? {
                ...m,
                content: finalContent,
                renderedHtml: renderContent(finalContent),
                thinking: finalThinking,
                subAgentResults: subAgentResults,
                stickerUrl: stickerUrl,
                isStreaming: false,
                responseTimeMs,
              }
            : m
        )
      );
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Unknown error';
      // If user stopped generation, keep partial content and mark as stopped
      if (errorMsg === '已停止生成') {
        setMessages(prev =>
          prev.map(m =>
            m.id === assistantMsg.id
              ? {
                  ...m,
                  isStreaming: false,
                  renderedHtml: m.content ? renderContent(m.content) : undefined,
                }
              : m
          )
        );
      } else {
        setError(errorMsg);
        setMessages(prev =>
          prev.map(m =>
            m.id === assistantMsg.id
              ? { ...m, content: `[Error] ${errorMsg}`, isStreaming: false }
              : m
          )
        );
      }
    } finally {
      setIsStreaming(false);
      isStreamingRef.current = false;
      streamingMsgIdRef.current = null;
    }
  }, [options.getSystemInstructions, options.companionMode]);

  const stopGeneration = useCallback(() => {
    if (!isStreamingRef.current) return;
    clientRef.current.abort();
  }, []);

  const clearMessages = useCallback(() => {
    setMessages([]);
    setError(null);
    setCompanionState(null);
    sessionIdRef.current = crypto.randomUUID();
  }, []);

  return { messages, isStreaming, error, toolEvents, agentProgress, companionState, sendMessage, stopGeneration, clearMessages, sessionId: sessionIdRef.current };
}
