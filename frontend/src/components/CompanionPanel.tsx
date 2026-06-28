import { memo } from 'react';
import type { CompanionState } from '../api/types';

interface Props {
  state: CompanionState;
  visible: boolean;
}

function getMoodEmoji(mood: string, intensity: number): string {
  const high = intensity > 0.6;
  const map: Record<string, [string, string]> = {
    '开心': ['😊', '🥰'],
    '兴奋': ['😆', '🤩'],
    '平静': ['😌', '😐'],
    '好奇': ['🧐', '🤔'],
    '无奈': ['😮‍💨', '😑'],
    '有点烦': ['😤', '😠'],
    '生气': ['😡', '🤬'],
    '感动': ['🥹', '😭'],
    '无语': ['😑', '😶'],
    '疲惫': ['😪', '😴'],
    '期待': ['🤗', '😍'],
    '失望': ['😞', '😔'],
    '尴尬': ['😅', '😬'],
  };
  for (const [key, [low, hi]] of Object.entries(map)) {
    if (mood.includes(key)) return high ? hi : low;
  }
  return high ? '😤' : '🙂';
}

function getAffinityLabel(v: number): string {
  if (v <= 15) return '非常反感';
  if (v <= 30) return '有些反感';
  if (v <= 45) return '略有保留';
  if (v <= 55) return '中立';
  if (v <= 70) return '有好感';
  if (v <= 85) return '挺喜欢';
  return '非常喜欢';
}

function getAffinityColor(v: number): string {
  if (v <= 20) return '#ef4444';
  if (v <= 40) return '#f97316';
  if (v <= 55) return '#eab308';
  if (v <= 70) return '#84cc16';
  return '#22c55e';
}

function getBarColor(value: number): string {
  if (value <= 25) return '#ef4444';
  if (value <= 50) return '#f97316';
  if (value <= 75) return '#eab308';
  return '#22c55e';
}

export const CompanionPanel = memo(function CompanionPanel({ state, visible }: Props) {
  if (!visible) return null;

  const moodEmoji = getMoodEmoji(state.mood, state.mood_intensity);
  const affinityLabel = getAffinityLabel(state.affinity);
  const affinityColor = getAffinityColor(state.affinity);

  return (
    <div className="companion-panel">
      <div className="companion-header">
        <span className="companion-mood-emoji">{moodEmoji}</span>
        <span className="companion-mood-text">{state.mood}</span>
        <span className="companion-mood-intensity">{Math.round(state.mood_intensity * 100)}%</span>
      </div>

      <div className="companion-bars">
        <div className="companion-bar-row">
          <span className="companion-bar-label">好感度</span>
          <div className="companion-bar-track">
            <div
              className="companion-bar-fill"
              style={{ width: `${state.affinity}%`, background: affinityColor }}
            />
          </div>
          <span className="companion-bar-value" style={{ color: affinityColor }}>
            {Math.round(state.affinity)} <small>{affinityLabel}</small>
          </span>
        </div>

        <div className="companion-bar-row">
          <span className="companion-bar-label">信任度</span>
          <div className="companion-bar-track">
            <div
              className="companion-bar-fill"
              style={{ width: `${state.trust}%`, background: getBarColor(state.trust) }}
            />
          </div>
          <span className="companion-bar-value">{Math.round(state.trust)}</span>
        </div>

        <div className="companion-bar-row">
          <span className="companion-bar-label">耐心</span>
          <div className="companion-bar-track">
            <div
              className="companion-bar-fill"
              style={{ width: `${state.patience}%`, background: getBarColor(state.patience) }}
            />
          </div>
          <span className="companion-bar-value">{Math.round(state.patience)}</span>
        </div>

        <div className="companion-bar-row">
          <span className="companion-bar-label">精力</span>
          <div className="companion-bar-track">
            <div
              className="companion-bar-fill"
              style={{ width: `${state.energy}%`, background: getBarColor(state.energy) }}
            />
          </div>
          <span className="companion-bar-value">{Math.round(state.energy)}</span>
        </div>
      </div>

      {state.last_reason && (
        <div className="companion-reason">
          <span className="companion-reason-icon">💭</span>
          <span className="companion-reason-text">{state.last_reason}</span>
        </div>
      )}
    </div>
  );
});
