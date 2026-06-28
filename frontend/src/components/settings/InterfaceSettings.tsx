interface Props {
  hideGlass: boolean;
  hideWelcomePrompt: boolean;
  useSolidBubble: boolean;
  autoTextEnabled: boolean;
  bgVideo: string | null;
  onHideGlassChange: (hide: boolean) => void;
  onHideWelcomePromptChange: (hide: boolean) => void;
  onUseSolidBubbleChange: (use: boolean) => void;
  onAutoTextEnabledChange: (enabled: boolean) => void;
}

export function InterfaceSettings({
  hideGlass, hideWelcomePrompt, useSolidBubble, autoTextEnabled, bgVideo,
  onHideGlassChange, onHideWelcomePromptChange, onUseSolidBubbleChange, onAutoTextEnabledChange,
}: Props) {
  return (
    <div className="settings-section">
      <label className="settings-label">界面</label>
      <div className="settings-toggle-row">
        <div className="settings-toggle-info">
          <span className="settings-toggle-title">隐藏顶栏和底栏遮罩层</span>
          <span className="settings-toggle-desc">始终隐藏状态栏和输入栏的毛玻璃背景</span>
        </div>
        <button className={`toggle-switch${hideGlass ? ' active' : ''}`} onClick={() => onHideGlassChange(!hideGlass)} role="switch" aria-checked={hideGlass}>
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
      <div className="settings-toggle-row">
        <div className="settings-toggle-info">
          <span className="settings-toggle-title">隐藏启动页提示</span>
          <span className="settings-toggle-desc">隐藏欢迎页的副标题和建议按钮</span>
        </div>
        <button className={`toggle-switch${hideWelcomePrompt ? ' active' : ''}`} onClick={() => onHideWelcomePromptChange(!hideWelcomePrompt)} role="switch" aria-checked={hideWelcomePrompt}>
          <span className="toggle-knob">
            <span className="toggle-icon toggle-icon-off">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" />
              </svg>
            </span>
            <span className="toggle-icon toggle-icon-on">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" /><line x1="8" x2="16" y1="2" y2="22" />
              </svg>
            </span>
          </span>
        </button>
      </div>
      <div className="settings-toggle-row">
        <div className="settings-toggle-info">
          <span className="settings-toggle-title">白底消息气泡</span>
          <span className="settings-toggle-desc">关闭后使用半透明消息气泡</span>
        </div>
        <button className={`toggle-switch${useSolidBubble ? ' active' : ''}`} onClick={() => onUseSolidBubbleChange(!useSolidBubble)} role="switch" aria-checked={useSolidBubble}>
          <span className="toggle-knob">
            <span className="toggle-icon toggle-icon-off">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="10" strokeDasharray="4 3" />
              </svg>
            </span>
            <span className="toggle-icon toggle-icon-on">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="10" />
              </svg>
            </span>
          </span>
        </button>
      </div>
      <div className={`settings-collapse-body${!useSolidBubble && bgVideo ? ' open' : ''}`}>
        <div className="settings-collapse-inner">
          <div className="settings-toggle-row">
            <div className="settings-toggle-info">
              <span className="settings-toggle-title">自适应文字颜色</span>
              <span className="settings-toggle-desc">根据视频背景自动调整气泡文字颜色</span>
            </div>
            <button className={`toggle-switch${autoTextEnabled ? ' active' : ''}`} onClick={() => onAutoTextEnabledChange(!autoTextEnabled)} role="switch" aria-checked={autoTextEnabled}>
              <span className="toggle-knob">
                <span className="toggle-icon toggle-icon-off">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M7 2h10" /><path d="M5 6h14" /><rect x="3" y="10" width="18" height="12" rx="2" /><path d="M9 14h6" /><path d="M9 18h4" />
                  </svg>
                </span>
                <span className="toggle-icon toggle-icon-on">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="13.5" cy="6.5" r="2.5" /><circle cx="17.5" cy="10.5" r="2.5" /><circle cx="8.5" cy="7.5" r="2.5" /><circle cx="6.5" cy="12.5" r="2.5" /><path d="M12 22C6.5 22 2 17.5 2 12S6.5 2 12 2s10 4.5 10 10" /><path d="M14 18.5c1.5-1.5 2-3.5 2-6.5" /><path d="M10 18.5c-1.5-1.5-2-3.5-2-6.5" />
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
