import { useState, useEffect, useCallback, useRef } from 'react';
import type { LgConfig, LgCategory } from '../App';
import { loadBool, loadNumber, loadString, loadFromStorage } from '../utils/storage';

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

  // Batch-persist all settings in a debounced effect
  const persistTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (persistTimerRef.current) clearTimeout(persistTimerRef.current);
    persistTimerRef.current = setTimeout(() => {
      try {
        localStorage.setItem('agent-teams-hide-glass', String(hideGlass));
        localStorage.setItem('agent-teams-hide-welcome-prompt', String(hideWelcomePrompt));
        localStorage.setItem('agent-teams-use-solid-bubble', String(useSolidBubble));
        localStorage.setItem('agent-teams-bubble-text-color', bubbleTextColor);
        localStorage.setItem('agent-teams-user-bubble-color', userBubbleColor);
        localStorage.setItem('agent-teams-user-bubble-alpha', String(userBubbleAlpha));
        localStorage.setItem('agent-teams-assistant-bubble-color', assistantBubbleColor);
        localStorage.setItem('agent-teams-assistant-bubble-alpha', String(assistantBubbleAlpha));
        localStorage.setItem('agent-teams-solid-user-bubble-color', solidUserBubbleColor);
        localStorage.setItem('agent-teams-solid-assistant-bubble-color', solidAssistantBubbleColor);
        localStorage.setItem('agent-teams-auto-text', String(autoTextEnabled));
        localStorage.setItem('agent-teams-lg-config', JSON.stringify(lgConfig));
      } catch {}
    }, 300);
    return () => { if (persistTimerRef.current) clearTimeout(persistTimerRef.current); };
  }, [hideGlass, hideWelcomePrompt, useSolidBubble, bubbleTextColor, userBubbleColor, userBubbleAlpha, assistantBubbleColor, assistantBubbleAlpha, solidUserBubbleColor, solidAssistantBubbleColor, autoTextEnabled, lgConfig]);

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
