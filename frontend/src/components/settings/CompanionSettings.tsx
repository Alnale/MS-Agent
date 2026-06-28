interface Props {
  companionMode: boolean;
  showEmotionPanel: boolean;
  onCompanionModeChange: (enabled: boolean) => void;
  onShowEmotionPanelChange: (show: boolean) => void;
}

export function CompanionSettings({
  companionMode, showEmotionPanel,
  onCompanionModeChange, onShowEmotionPanelChange,
}: Props) {
  return (
    <div className="settings-section">
      <label className="settings-label">陪伴</label>
      <div className="settings-toggle-row">
        <div className="settings-toggle-info">
          <span className="settings-toggle-title">陪伴模式</span>
          <span className="settings-toggle-desc">启用情感分析，让回复更有真实情绪和个性</span>
        </div>
        <button
          className={`toggle-switch${companionMode ? ' active' : ''}`}
          onClick={() => onCompanionModeChange(!companionMode)}
          role="switch" aria-checked={companionMode}
        >
          <span className="toggle-knob">
            <span className="toggle-icon toggle-icon-off">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z" />
              </svg>
            </span>
            <span className="toggle-icon toggle-icon-on">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z" />
              </svg>
            </span>
          </span>
        </button>
      </div>
      <div className={`settings-collapse-body${companionMode ? ' open' : ''}`}>
        <div className="settings-collapse-inner">
          <div className="settings-toggle-row">
            <div className="settings-toggle-info">
              <span className="settings-toggle-title">显示情绪面板</span>
              <span className="settings-toggle-desc">在聊天区域上方显示实时情绪状态</span>
            </div>
            <button
              className={`toggle-switch${showEmotionPanel ? ' active' : ''}`}
              onClick={() => onShowEmotionPanelChange(!showEmotionPanel)}
              role="switch" aria-checked={showEmotionPanel}
            >
              <span className="toggle-knob">
                <span className="toggle-icon toggle-icon-off">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z" /><circle cx="12" cy="12" r="3" />
                  </svg>
                </span>
                <span className="toggle-icon toggle-icon-on">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M9.88 9.88a3 3 0 1 0 4.24 4.24" /><path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68" /><path d="M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61" /><line x1="2" x2="22" y1="2" y2="22" />
                  </svg>
                </span>
              </span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
