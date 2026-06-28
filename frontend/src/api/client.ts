import type {
  ChatRequest,
  StreamChunk,
  HealthResponse,
  RuntimeError,
} from './types';

export class AgentTeamsClient {
  private baseUrl: string;
  private timeout: number;
  private activeController: AbortController | null = null;
  private userAborted = false;

  constructor(baseUrl: string = '', timeout: number = 300_000) {
    this.baseUrl = baseUrl.replace(/\/+$/, '');
    this.timeout = timeout;
  }

  /** Abort the currently active stream request (user-initiated) */
  abort(): void {
    if (this.activeController) {
      this.userAborted = true;
      this.activeController.abort();
    }
  }

  /** POST /chat — streaming chat (primary entry point) */
  async *chat(input: ChatRequest): AsyncGenerator<StreamChunk> {
    const controller = new AbortController();
    this.activeController = controller;
    let timer = setTimeout(() => controller.abort(), this.timeout);
    try {
      const res = await fetch(`${this.baseUrl}/chat`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Accept': 'text/event-stream',
        },
        body: JSON.stringify(input),
        signal: controller.signal,
      });

      if (!res.ok) {
        let err: RuntimeError;
        try {
          err = await res.json();
        } catch {
          err = { status: 'error', error: `HTTP ${res.status}: ${res.statusText}`, error_code: 'http_error' };
        }
        throw new AgentTeamsError(err);
      }

      const reader = res.body?.getReader();
      if (!reader) throw new Error('No readable stream');

      const decoder = new TextDecoder();
      let buffer = '';

      try {
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          // Re-arm timeout on each chunk received (idle timeout)
          clearTimeout(timer);
          timer = setTimeout(() => controller.abort(), this.timeout);

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split('\n');
          buffer = lines.pop() ?? '';

          for (const line of lines) {
            const trimmed = line.trim();
            if (!trimmed.startsWith('data:')) continue;
            const json = trimmed.slice(5).trim();
            if (!json || json === '[DONE]') continue;
            try {
              const chunk: StreamChunk = JSON.parse(json);
              yield chunk;
              if (chunk.done) return;
            } catch {
              // skip malformed SSE lines
            }
          }
        }
      } finally {
        reader.releaseLock();
      }
    } catch (err) {
      if (err instanceof DOMException && err.name === 'AbortError') {
        if (this.userAborted) {
          throw new Error('已停止生成');
        }
        throw new Error('请求超时，服务器未响应');
      }
      throw err;
    } finally {
      clearTimeout(timer);
      this.activeController = null;
      this.userAborted = false;
    }
  }

  /** GET /health — system status */
  async health(): Promise<HealthResponse> {
    const res = await fetch(`${this.baseUrl}/health`);
    if (!res.ok) throw new Error(`Health check failed: ${res.status}`);
    return res.json();
  }

  /** GET /tools — list available tools */
  async tools(): Promise<{ tools: { name: string; description: string; parameters?: { schema: unknown; required?: string[] } }[] }> {
    const res = await fetch(`${this.baseUrl}/tools`);
    if (!res.ok) throw new Error(`Tools fetch failed: ${res.status}`);
    return res.json();
  }

  /** GET /presets — list built-in presets */
  async presets(): Promise<{ presets: { id: string; name: string; icon: string; description: string; system_instructions: string[] }[] }> {
    const res = await fetch(`${this.baseUrl}/presets`);
    if (!res.ok) throw new Error(`Presets fetch failed: ${res.status}`);
    return res.json();
  }
}

class AgentTeamsError extends Error {
  errorCode: string;
  sessionId?: string;

  constructor(err: RuntimeError) {
    super(err.error);
    this.name = 'AgentTeamsError';
    this.errorCode = err.error_code;
    this.sessionId = err.session_id;
  }
}
