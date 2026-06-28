import { useState, useRef, useCallback, useEffect } from 'react';
import { AgentTeamsClient } from '../api/client';
import type { ChatMessage, ChatRequest, ToolStatusEvent, SubAgentResultSummary, AgentProgress, HttpSource, CompanionState } from '../api/types';
import type { Session } from './useSession';
import { renderContent } from '../utils/renderContent';

const nextId = () => crypto.randomUUID();

/** Extract [[tool:name]] from message content, returns tool name or null */
function extractForceTool(content: string): string | null {
  const match = content.match(/\[\[tool:(\w+)\]\]/);
  return match ? match[1] : null;
}

/** Extract HTTP sources (URLs + titles) from http_request tool output */
function extractHttpSources(output: unknown): HttpSource[] {
  if (!output || typeof output !== 'object') return [];
  const data = output as Record<string, unknown>;
  const sources: HttpSource[] = [];
  const seen = new Set<string>();

  const addUrl = (url: string, title?: string) => {
    if (!url || !url.startsWith('http')) return;
    // Skip search engine internal URLs
    if (/baidu\.com\/s[?&]|bing\.com\/search|google\.com\/search/.test(url)) return;
    if (seen.has(url)) return;
    seen.add(url);
    sources.push({ url, title });
  };

  // Batch results: results[].url + results[].title
  if (Array.isArray(data.results)) {
    for (const r of data.results) {
      if (r && typeof r === 'object') {
        const url = (r as Record<string, unknown>).url;
        const title = (r as Record<string, unknown>).title;
        if (typeof url === 'string') addUrl(url, typeof title === 'string' ? title : undefined);
      }
    }
  }

  // Merged links from search results: merged_links[].href + merged_links[].text
  if (Array.isArray(data.merged_links)) {
    for (const link of data.merged_links) {
      if (link && typeof link === 'object') {
        const href = (link as Record<string, unknown>).href;
        const text = (link as Record<string, unknown>).text;
        if (typeof href === 'string') addUrl(href, typeof text === 'string' ? text : undefined);
      }
    }
  }

  // Single request: output.url
  if (typeof data.url === 'string' && sources.length === 0) {
    addUrl(data.url);
  }

  return sources;
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
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toolEvents, setToolEvents] = useState<ToolStatusEvent[]>([]);
  const [agentProgress, setAgentProgress] = useState<AgentProgress[]>([]);
  const [companionState, setCompanionState] = useState<CompanionState | null>(null);
  const streamingMsgIdRef = useRef<string | null>(null);

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
    if (isStreaming && !forceResend) return;

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
    streamingMsgIdRef.current = assistantMsg.id;

    // All messages go through /chat — unified path
    // Extract [[tool:name]] hint and pass as force_tool to backend
    const forceTool = extractForceTool(content.trim());

    const history = messages
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
      let httpSources: HttpSource[] | undefined;
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
          if (chunk.tool_status.status === 'completed' && options.onToolResult) {
            options.onToolResult(chunk.tool_status);
          }
          // Extract HTTP sources from http_request tool output
          if (chunk.tool_status.status === 'completed' && chunk.tool_status.tool_name === 'http_request' && chunk.tool_status.success) {
            const sources = extractHttpSources(chunk.tool_status.output);
            if (sources.length > 0) {
              httpSources = httpSources ? [...httpSources, ...sources] : sources;
            }
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
                httpSources: httpSources,
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
      streamingMsgIdRef.current = null;
    }
  }, [isStreaming, messages, options.getSystemInstructions]);

  const stopGeneration = useCallback(() => {
    if (!isStreaming) return;
    clientRef.current.abort();
  }, [isStreaming]);

  const clearMessages = useCallback(() => {
    setMessages([]);
    setError(null);
    setCompanionState(null);
    sessionIdRef.current = crypto.randomUUID();
  }, []);

  return { messages, isStreaming, error, toolEvents, agentProgress, companionState, sendMessage, stopGeneration, clearMessages, sessionId: sessionIdRef.current };
}
