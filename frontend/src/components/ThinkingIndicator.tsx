import { useState, useEffect, useMemo, memo } from 'react';
import type { AgentProgress } from '../api/types';

interface Props {
  startTime?: number;
  agentProgress?: AgentProgress[];
}

function formatElapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}:${(s % 60).toString().padStart(2, '0')}`;
}

const STAGE_LABELS: Record<string, string> = {
  initializing: '初始化',
  planning: '规划',
  executing: '执行',
  routing: '路由',
  synthesis: '整合',
};

interface StageInfo {
  name: string;
  label: string;
  detail: string;
  status: 'done' | 'active' | 'pending';
}

function buildStages(progress: AgentProgress[]): StageInfo[] {
  const stages: StageInfo[] = [];
  const seen = new Set<string>();
  for (const p of progress) {
    if (p.stage === 'stage_started' && p.stage_name && !seen.has(p.stage_name)) {
      seen.add(p.stage_name);
      stages.push({ name: p.stage_name, label: STAGE_LABELS[p.stage_name] || p.stage_name, detail: p.detail || '', status: 'active' });
      for (let i = 0; i < stages.length - 1; i++) stages[i].status = 'done';
    }
    if (p.stage === 'synthesis_started' && !seen.has('synthesis')) {
      seen.add('synthesis');
      stages.push({ name: 'synthesis', label: '整合', detail: '整合结果', status: 'active' });
      for (let i = 0; i < stages.length - 1; i++) stages[i].status = 'done';
    }
  }
  return stages;
}

function buildAgentList(progress: AgentProgress[]) {
  const agents: Array<{ id: string; status: 'running' | 'done' | 'failed'; durationMs?: number }> = [];
  const map = new Map<string, typeof agents[0]>();
  for (const p of progress) {
    if (p.stage === 'agent_started' && p.agent_id) {
      const e = { id: p.agent_id, status: 'running' as const };
      map.set(p.agent_id, e);
      agents.push(e);
    }
    if (p.stage === 'agent_completed' && p.agent_id) {
      const e = map.get(p.agent_id);
      if (e) { e.status = p.success === false ? 'failed' : 'done'; e.durationMs = p.duration_ms; }
    }
  }
  return agents;
}

export const ThinkingIndicator = memo(function ThinkingIndicator({ startTime, agentProgress }: Props) {
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    if (!startTime) return;
    const tick = () => setElapsed(Date.now() - startTime);
    tick();
    const timer = setInterval(tick, 200);
    return () => clearInterval(timer);
  }, [startTime]);

  const stages = useMemo(() => buildStages(agentProgress || []), [agentProgress]);
  const agents = useMemo(() => buildAgentList(agentProgress || []), [agentProgress]);
  const current = stages.length > 0 ? stages[stages.length - 1] : null;
  const doneCount = agents.filter(a => a.status !== 'running').length;
  // Unique key for label to trigger crossfade animation
  const labelKey = current?.name || 'init';

  return (
    <div className="ti">
      {/* Prismatic top line */}
      <div className="ti-prism" />

      {/* Header */}
      <div className="ti-head">
        <div className="ti-pulse">
          <span className="ti-pulse-dot" />
          <span className="ti-pulse-ring" />
        </div>
        {/* Crossfade label: key change triggers re-mount + animation */}
        <span className="ti-label" key={labelKey}>
          <span className="ti-label-text">{current?.detail || '深度思考中'}</span>
        </span>
        <span className="ti-timer">{formatElapsed(elapsed)}</span>
      </div>

      {/* Stepper — minimal dots */}
      {stages.length > 1 && (
        <div className="ti-steps" role="progressbar" aria-label="处理进度">
          {stages.map((s, i) => (
            <div key={s.name} className={`ti-step ${s.status}`} title={s.label}>
              <div className="ti-node">
                {s.status === 'done' && (
                  <svg className="ti-check" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M3.5 8.5L6.5 11.5L12.5 4.5" />
                  </svg>
                )}
                {s.status === 'active' && <span className="ti-glow" />}
              </div>
              {i < stages.length - 1 && <div className="ti-line" />}
            </div>
          ))}
        </div>
      )}

      {/* Agent pills */}
      {agents.length > 0 && (
        <div className="ti-pills">
          {agents.map((a, i) => (
            <div
              key={a.id}
              className={`ti-pill ${a.status}`}
              style={{ animationDelay: `${i * 60}ms` }}
            >
              {a.status === 'running' && <span className="ti-shimmer" />}
              <span className="ti-pill-icon">
                {a.status === 'done' ? '✓' : a.status === 'failed' ? '✕' : <span className="ti-spinner" />}
              </span>
              <span className="ti-pill-name">{a.id}</span>
              {a.status !== 'running' && a.durationMs != null && a.durationMs > 0 && (
                <span className="ti-pill-dur">{(a.durationMs / 1000).toFixed(1)}s</span>
              )}
            </div>
          ))}
          {agents.length > 1 && (
            <span className="ti-count">{doneCount}/{agents.length}</span>
          )}
        </div>
      )}
    </div>
  );
});
