import { useState, useEffect, useCallback } from 'react';
import type { LgConfig, LgCategory } from '../App';

const LG_DEFAULTS: LgCategory = {
  enabled: false,
  mode: 'standard',
  overLight: false,
  displacementScale: 70,
  blurAmount: 0.0625,
  saturation: 140,
  aberrationIntensity: 2,
  elasticity: 0.15,
  cornerRadius: 999,
};

function loadFromStorage<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function loadString(key: string, fallback: string): string {
  try { return localStorage.getItem(key) || fallback; } catch { return fallback; }
}

function loadNumber(key: string, fallback: number): number {
  try { return parseFloat(localStorage.getItem(key) || String(fallback)); } catch { return fallback; }
}

function loadBool(key: string, fallback: boolean): boolean {
  try { return localStorage.getItem(key) === 'true' ? true : fallback === true ? false : localStorage.getItem(key) === null ? fallback : false; } catch { return fallback; }
}

export function useSettings() {
  const [hideGlass, setHideGlass] = useState(() => loadBool('agent-teams-hide-glass', false));
  const [hideWelcomePrompt, setHideWelcomePrompt] = useState(() => loadBool('agent-teams-hide-welcome-prompt', false));
  const [useSolidBubble, setUseSolidBubble] = useState(() => localStorage.getItem('agent-teams-use-solid-bubble') !== 'false');
  const [bubbleTextColor, setBubbleTextColor] = useState(() => loadString('agent-teams-bubble-text-color', '#1a1a2e'));
  const [userBubbleColor, setUserBubbleColor] = useState(() => loadString('agent-teams-user-bubble-color', '#f280a0'));
  const [userBubbleAlpha, setUserBubbleAlpha] = useState(() => loadNumber('agent-teams-user-bubble-alpha', 0.36));
  const [assistantBubbleColor, setAssistantBubbleColor] = useState(() => loadString('agent-teams-assistant-bubble-color', '#b8a9e8'));
  const [assistantBubbleAlpha, setAssistantBubbleAlpha] = useState(() => loadNumber('agent-teams-assistant-bubble-alpha', 0.35));
  const [solidUserBubbleColor, setSolidUserBubbleColor] = useState(() => loadString('agent-teams-solid-user-bubble-color', '#ffffff'));
  const [solidAssistantBubbleColor, setSolidAssistantBubbleColor] = useState(() => loadString('agent-teams-solid-assistant-bubble-color', '#ffffff'));
  const [autoTextEnabled, setAutoTextEnabled] = useState(() => loadBool('agent-teams-auto-text', false));
  const [companionMode, setCompanionMode] = useState(() => loadBool('agent-teams-companion-mode', false));
  const [showEmotionPanel, setShowEmotionPanel] = useState(() => loadBool('agent-teams-show-emotion-panel', false));

  const [lgConfig, setLgConfig] = useState<LgConfig>(() => {
    const saved = loadFromStorage<Partial<LgConfig>>('agent-teams-lg-config', {});
    return {
      enabled: saved.enabled ?? false,
      mask: { ...LG_DEFAULTS, ...saved.mask },
      card: { ...LG_DEFAULTS, ...saved.card },
      button: { ...LG_DEFAULTS, ...saved.button },
      companionPanel: saved.companionPanel ?? false,
    };
  });

  // Persist all settings
  useEffect(() => { localStorage.setItem('agent-teams-hide-glass', String(hideGlass)); }, [hideGlass]);
  useEffect(() => { localStorage.setItem('agent-teams-hide-welcome-prompt', String(hideWelcomePrompt)); }, [hideWelcomePrompt]);
  useEffect(() => { localStorage.setItem('agent-teams-use-solid-bubble', String(useSolidBubble)); }, [useSolidBubble]);
  useEffect(() => { localStorage.setItem('agent-teams-bubble-text-color', bubbleTextColor); }, [bubbleTextColor]);
  useEffect(() => { localStorage.setItem('agent-teams-user-bubble-color', userBubbleColor); }, [userBubbleColor]);
  useEffect(() => { localStorage.setItem('agent-teams-user-bubble-alpha', String(userBubbleAlpha)); }, [userBubbleAlpha]);
  useEffect(() => { localStorage.setItem('agent-teams-assistant-bubble-color', assistantBubbleColor); }, [assistantBubbleColor]);
  useEffect(() => { localStorage.setItem('agent-teams-assistant-bubble-alpha', String(assistantBubbleAlpha)); }, [assistantBubbleAlpha]);
  useEffect(() => { localStorage.setItem('agent-teams-solid-user-bubble-color', solidUserBubbleColor); }, [solidUserBubbleColor]);
  useEffect(() => { localStorage.setItem('agent-teams-solid-assistant-bubble-color', solidAssistantBubbleColor); }, [solidAssistantBubbleColor]);
  useEffect(() => { localStorage.setItem('agent-teams-auto-text', String(autoTextEnabled)); }, [autoTextEnabled]);
  useEffect(() => { localStorage.setItem('agent-teams-lg-config', JSON.stringify(lgConfig)); }, [lgConfig]);

  const handleCompanionModeChange = useCallback((v: boolean) => {
    setCompanionMode(v);
    try { localStorage.setItem('agent-teams-companion-mode', String(v)); } catch {}
  }, []);

  const handleShowEmotionPanelChange = useCallback((v: boolean) => {
    setShowEmotionPanel(v);
    try { localStorage.setItem('agent-teams-show-emotion-panel', String(v)); } catch {}
  }, []);

  return {
    hideGlass, setHideGlass,
    hideWelcomePrompt, setHideWelcomePrompt,
    useSolidBubble, setUseSolidBubble,
    bubbleTextColor, setBubbleTextColor,
    userBubbleColor, setUserBubbleColor,
    userBubbleAlpha, setUserBubbleAlpha,
    assistantBubbleColor, setAssistantBubbleColor,
    assistantBubbleAlpha, setAssistantBubbleAlpha,
    solidUserBubbleColor, setSolidUserBubbleColor,
    solidAssistantBubbleColor, setSolidAssistantBubbleColor,
    autoTextEnabled, setAutoTextEnabled,
    companionMode, handleCompanionModeChange,
    showEmotionPanel, handleShowEmotionPanelChange,
    lgConfig, setLgConfig,
  };
}
