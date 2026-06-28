// ─── Request types ────────────────────────────────────────────────

/** Request body for POST /chat */
export interface ChatRequest {
  message: string;
  session_id?: string;
  recent_history?: Record<string, unknown>[];
  /** System instructions for persona/preset injection */
  system_instructions?: string[];
  /** Stream mode: "simple" (text only, for external consumers) or "full" (all events). Default: "full". */
  stream_mode?: 'simple' | 'full';
  /** Force a specific tool to be used (e.g. from ToolSelector). Ignored in simple mode. */
  force_tool?: string;
  /** Enable companion mode (emotional state tracking) */
  companion_mode?: boolean;
}

/** Companion emotional state */
export interface CompanionState {
  mood: string;
  mood_intensity: number;
  affinity: number;
  energy: number;
  patience: number;
  trust: number;
  last_reason: string;
  sticker: string;
  turn_count: number;
}

// ─── Response types ───────────────────────────────────────────────

/** Error response from backend */
export interface RuntimeError {
  status: 'error';
  error: string;
  error_code: string;
  session_id?: string;
}

/** Health check response from GET /health */
export interface HealthResponse {
  status: string;
  version: string;
  provider: string;
  model: string;
}

// ─── Streaming event types ────────────────────────────────────────

/** SSE stream chunk from POST /chat */
export interface StreamChunk {
  type: 'delta' | 'done' | 'error' | 'tool_status' | 'sub_agent_results' | 'agent_progress' | 'companion_state';
  delta: string;
  thinking_delta?: string;
  done: boolean;
  usage?: {
    input_tokens: number;
    output_tokens: number;
    cached_tokens: number;
  };
  tool_status?: ToolStatusEvent;
  sub_agent_results?: SubAgentResultSummary[];
  agent_progress?: AgentProgress;
  companion_state?: CompanionState;
}

/** Tool execution status event */
export interface ToolStatusEvent {
  status: 'preparing' | 'executing' | 'completed' | 'approval_required' | 'approved' | 'rejected' | 'error';
  call_id: string;
  tool_name: string;
  arguments?: unknown;
  success?: boolean;
  output?: unknown;
  error?: string;
  duration_ms?: number;
  approval_message?: string;
  reason?: string;
}

/** Summary of a SubAgent's result */
export interface SubAgentResultSummary {
  agent_id: string;
  content_summary: string;
  thinking?: string;
  quality: number;
  /** Sticker filename recommended by sentiment agent */
  sticker?: string;
}

/** Real-time pipeline progress event */
export interface AgentProgress {
  stage: 'stage_started' | 'agent_started' | 'agent_completed' | 'synthesis_started';
  stage_name?: string;
  detail?: string;
  agent_id?: string;
  agent_type?: string;
  success?: boolean;
  duration_ms?: number;
}

// ─── Tool types ───────────────────────────────────────────────────

/** Single parameter schema from the tool's JSON Schema properties */
export interface ToolParamSchema {
  type?: string;
  description?: string;
  enum?: string[];
  default?: unknown;
}

/** Tool definition from GET /tools */
export interface ToolDefinition {
  name: string;
  description: string;
  parameters?: {
    schema: Record<string, unknown>;
    required?: string[];
  };
}

// ─── UI types ─────────────────────────────────────────────────────

/** A single HTTP source extracted from http_request tool output */
export interface HttpSource {
  url: string;
  title?: string;
}

/** Chat message for UI display */
export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: number;
  quality?: number;
  isStreaming?: boolean;
  renderedHtml?: string;
  thinking?: string;
  subAgentResults?: SubAgentResultSummary[];
  responseTimeMs?: number;
  httpSources?: HttpSource[];
  /** Sticker URL recommended by sentiment agent */
  stickerUrl?: string;
  /** Companion emotional state at time of this message */
  companionState?: CompanionState;
}

/** Preset definition from backend */
export interface PresetDef {
  id: string;
  name: string;
  icon: string;
  description: string;
  system_instructions: string[];
}

/** User-defined custom preset (stored in localStorage) */
export interface CustomPreset {
  id: string;
  name: string;
  icon: string;
  description: string;
  system_instructions: string[];
  isCustom: true;
}
