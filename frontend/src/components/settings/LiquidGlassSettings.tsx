import { useState } from 'react';
import type { LgConfig, LgCategory } from '../../App';

interface Props {
  lgConfig: LgConfig;
  onLgConfigChange: (config: LgConfig) => void;
}

export function LiquidGlassSettings({ lgConfig, onLgConfigChange }: Props) {
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [lgInfoOpen, setLgInfoOpen] = useState(false);
  const [lgActiveTab, setLgActiveTab] = useState<'mask' | 'card' | 'button' | 'companion'>('mask');

  function updateLgCat(cat: 'mask' | 'card' | 'button', patch: Partial<LgCategory>) {
    onLgConfigChange({ ...lgConfig, [cat]: { ...lgConfig[cat], ...patch } });
  }

  const tabs = [
    ['mask', '遮罩层', '状态栏 · 输入栏'],
    ['card', '信息卡片', '欢迎页 · 设置 · 侧栏'],
    ['button', '按钮', '工具 · 输入框 · 发送'],
    ['companion', '情绪面板', '左上角情绪面板'],
  ] as const;

  return (
    <>
      <div className="settings-collapse-header" style={{ display: 'flex', alignItems: 'center' }}>
        <button style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'space-between', background: 'none', border: 'none', color: 'inherit', cursor: 'pointer', padding: 0, font: 'inherit' }}
          onClick={() => setAdvancedOpen(!advancedOpen)} aria-expanded={advancedOpen}>
          <span className="settings-label">高级选项</span>
          <svg className={`settings-collapse-chevron${advancedOpen ? ' open' : ''}`} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>
        <button className="lg-info-btn" onClick={(e) => { e.stopPropagation(); setLgInfoOpen(!lgInfoOpen); }} title="参数说明" aria-label="查看参数详细说明">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="10" /><line x1="12" y1="16" x2="12" y2="12" /><line x1="12" y1="8" x2="12.01" y2="8" />
          </svg>
        </button>
      </div>

      <div className={`lg-info-collapse${lgInfoOpen ? ' open' : ''}`}>
        <div className="lg-info-collapse-inner">
          <div className="lg-info-card">
            <div className="lg-info-card-header">
              <span className="lg-info-card-title">毛玻璃参数说明</span>
              <button className="lg-info-close" onClick={() => setLgInfoOpen(false)} aria-label="关闭">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            </div>
            <div className="lg-info-section">
              <div className="lg-info-item">
                <span className="lg-info-name">折射强度</span>
                <span className="lg-info-value">0 – 150</span>
                <p className="lg-info-desc">SVG feDisplacementMap 的 scale 参数，控制像素位移的最大距离。值越大，背景扭曲越明显。设为 0 时无位移，仅保留模糊效果。典型值：50-90。</p>
              </div>
              <div className="lg-info-item">
                <span className="lg-info-name">模糊量</span>
                <span className="lg-info-value">0 – 1</span>
                <p className="lg-info-desc">backdrop-filter 的 blur 半径系数，实际模糊 = 基线 + 该值 × 32px。基线在暗色背景为 4px，亮色模式为 12px。控制玻璃的磨砂程度，值越大背景越模糊。典型值：0.05-0.2。</p>
              </div>
              <div className="lg-info-item">
                <span className="lg-info-name">饱和度</span>
                <span className="lg-info-value">0% – 300%</span>
                <p className="lg-info-desc">backdrop-filter 的 saturate 百分比。100% 为原始色彩，高于 100% 增强透过玻璃看到的色彩鲜艳度，低于 100% 则趋向灰度。典型值：120%-180%。</p>
              </div>
              <div className="lg-info-item">
                <span className="lg-info-name">色散强度</span>
                <span className="lg-info-value">0 – 10</span>
                <p className="lg-info-desc">RGB 三通道的位移差异系数。值越大，红/绿/蓝通道的位移差距越大，产生棱镜色散效果。设为 0 时三通道位移相同，无色散。该效果仅在边缘区域生效（通过 edge mask 实现），中心保持无色散。典型值：1-3。</p>
              </div>
              <div className="lg-info-item">
                <span className="lg-info-name">亮色背景适配</span>
                <span className="lg-info-value">开 / 关</span>
                <p className="lg-info-desc">开启后：模糊基线从 4px 提升至 12px，折射强度减半，叠加半透明白色渐变。适用于浅色/白色背景，在亮色背景下玻璃效果更明显。</p>
              </div>
              <div className="lg-info-item">
                <span className="lg-info-name">弹性</span>
                <span className="lg-info-value">0 – 1</span>
                <p className="lg-info-desc">鼠标悬停时玻璃元素的弹性跟随强度。0 为固定不动，1 为最大程度跟随。同时影响位移方向的拉伸效果。仅适用于卡片和按钮，遮罩层无鼠标交互故不显示。</p>
              </div>
              <div className="lg-info-item">
                <span className="lg-info-name">圆角半径</span>
                <span className="lg-info-value">0 – 999px</span>
                <p className="lg-info-desc">玻璃容器的 border-radius。999px 为完全胶囊形，0 为直角。仅适用于卡片和按钮，遮罩层为全宽条状故不显示。</p>
              </div>
            </div>
            <div className="lg-info-footer">
              基于 <a href="https://github.com/rdev/liquid-glass-react" target="_blank" rel="noopener noreferrer">liquid-glass-react</a> 实现
            </div>
          </div>
        </div>
      </div>

      <div className={`settings-collapse-body${advancedOpen ? ' open' : ''}`}>
        <div className="settings-collapse-inner">
          {/* Master toggle */}
          <div className="settings-toggle-row">
            <div className="settings-toggle-info">
              <span className="settings-toggle-title">毛玻璃效果</span>
            </div>
            <button
              className={`toggle-switch${lgConfig.enabled ? ' active' : ''}`}
              onClick={() => {
                const next = !lgConfig.enabled;
                onLgConfigChange({
                  ...lgConfig, enabled: next,
                  mask: { ...lgConfig.mask, enabled: next },
                  card: { ...lgConfig.card, enabled: next },
                  button: { ...lgConfig.button, enabled: next },
                  companionPanel: next,
                });
              }}
              role="switch" aria-checked={lgConfig.enabled}
            >
              <span className="toggle-knob">
                <span className="toggle-icon toggle-icon-off">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M8 2h8l4 10-4 10H8L4 12Z" /><path d="M4 12h16" />
                  </svg>
                </span>
                <span className="toggle-icon toggle-icon-on">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M8 2h8l4 10-4 10H8L4 12Z" /><path d="M4 12h16" /><path d="M12 2v20" opacity="0.4" />
                  </svg>
                </span>
              </span>
            </button>
          </div>

          <div className={`settings-collapse-body${lgConfig.enabled ? ' open' : ''}`}>
            <div className="settings-collapse-inner">
              {/* Category tabs */}
              <div className="lg-tab-bar">
                {tabs.map(([key, label, desc]) => (
                  <button key={key} className={`lg-tab${lgActiveTab === key ? ' active' : ''}`} onClick={() => setLgActiveTab(key)}>
                    <span className="lg-tab-label">{label}</span>
                    <span className="lg-tab-desc">{desc}</span>
                  </button>
                ))}
              </div>

              {/* Active category toggle */}
              <div className="settings-toggle-row">
                <div className="settings-toggle-info">
                  <span className="settings-toggle-title">启用{lgActiveTab === 'mask' ? '遮罩层' : lgActiveTab === 'card' ? '信息卡片' : lgActiveTab === 'companion' ? '情绪面板' : '按钮'}效果</span>
                </div>
                <button
                  className={`toggle-switch${(lgActiveTab === 'companion' ? lgConfig.companionPanel : lgConfig[lgActiveTab].enabled) ? ' active' : ''}`}
                  onClick={() => {
                    if (lgActiveTab === 'companion') {
                      onLgConfigChange({ ...lgConfig, companionPanel: !lgConfig.companionPanel });
                    } else {
                      updateLgCat(lgActiveTab, { enabled: !lgConfig[lgActiveTab].enabled });
                    }
                  }}
                  role="switch" aria-checked={lgActiveTab === 'companion' ? lgConfig.companionPanel : lgConfig[lgActiveTab].enabled}
                >
                  <span className="toggle-knob">
                    <span className="toggle-icon toggle-icon-off">
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M18.36 6.64A9 9 0 0 1 20.77 15" /><path d="M6.16 6.16a9 9 0 0 0-.73 12.08" /><line x1="12" x2="12" y1="2" y2="6" /><line x1="12" x2="12" y1="18" y2="22" /><line x1="2" x2="6" y1="12" y2="12" /><line x1="18" x2="22" y1="12" y2="12" />
                      </svg>
                    </span>
                    <span className="toggle-icon toggle-icon-on">
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M12 2v4" /><path d="M12 18v4" /><path d="M4.93 4.93l2.83 2.83" /><path d="M16.24 16.24l2.83 2.83" /><path d="M2 12h4" /><path d="M18 12h4" /><path d="M4.93 19.07l2.83-2.83" /><path d="M16.24 7.76l2.83-2.83" />
                      </svg>
                    </span>
                  </span>
                </button>
              </div>

              {/* Active category parameters */}
              <div className={`settings-collapse-body${(lgActiveTab === 'companion' ? lgConfig.companionPanel : lgConfig[lgActiveTab].enabled) ? ' open' : ''}`}>
                <div className="settings-collapse-inner">
                  {lgActiveTab === 'companion' ? (
                    <div style={{ padding: '8px 0', fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                      开启后情绪面板将使用毛玻璃效果，关闭则显示白底卡片样式。
                    </div>
                  ) : (() => {
                    const cat = lgConfig[lgActiveTab];
                    return (
                      <>
                        <div className="settings-toggle-row">
                          <div className="settings-toggle-info">
                            <span className="settings-toggle-title">亮色背景适配</span>
                            <span className="settings-toggle-desc">在浅色背景上优化显示效果</span>
                          </div>
                          <button className={`toggle-switch${cat.overLight ? ' active' : ''}`}
                            onClick={() => updateLgCat(lgActiveTab, { overLight: !cat.overLight })}
                            role="switch" aria-checked={cat.overLight}>
                            <span className="toggle-knob">
                              <span className="toggle-icon toggle-icon-off">
                                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                                  <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" />
                                </svg>
                              </span>
                              <span className="toggle-icon toggle-icon-on">
                                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                                  <circle cx="12" cy="12" r="4" /><path d="M12 2v2" /><path d="M12 20v2" /><path d="M4.93 4.93l1.41 1.41" /><path d="M17.66 17.66l1.41 1.41" /><path d="M2 12h2" /><path d="M20 12h2" /><path d="M6.34 17.66l-1.41 1.41" /><path d="M19.07 4.93l-1.41 1.41" />
                                </svg>
                              </span>
                            </span>
                          </button>
                        </div>

                        <div className="settings-section">
                          <div className="settings-slider-header">
                            <label className="settings-label">折射强度</label>
                            <span className="settings-slider-value">{cat.displacementScale}</span>
                          </div>
                          <div className="settings-slider-track">
                            <input type="range" min="0" max="150" step="1" value={cat.displacementScale}
                              onChange={(e) => updateLgCat(lgActiveTab, { displacementScale: parseFloat(e.target.value) })}
                              className="settings-slider" style={{ '--fill': `${(cat.displacementScale / 150) * 100}%` } as React.CSSProperties} />
                          </div>
                        </div>

                        <div className="settings-section">
                          <div className="settings-slider-header">
                            <label className="settings-label">模糊量</label>
                            <span className="settings-slider-value">{cat.blurAmount.toFixed(3)}</span>
                          </div>
                          <div className="settings-slider-track">
                            <input type="range" min="0" max="1" step="0.001" value={cat.blurAmount}
                              onChange={(e) => updateLgCat(lgActiveTab, { blurAmount: parseFloat(e.target.value) })}
                              className="settings-slider" style={{ '--fill': `${cat.blurAmount * 100}%` } as React.CSSProperties} />
                          </div>
                        </div>

                        <div className="settings-section">
                          <div className="settings-slider-header">
                            <label className="settings-label">饱和度</label>
                            <span className="settings-slider-value">{cat.saturation}%</span>
                          </div>
                          <div className="settings-slider-track">
                            <input type="range" min="0" max="300" step="1" value={cat.saturation}
                              onChange={(e) => updateLgCat(lgActiveTab, { saturation: parseFloat(e.target.value) })}
                              className="settings-slider" style={{ '--fill': `${(cat.saturation / 300) * 100}%` } as React.CSSProperties} />
                          </div>
                        </div>

                        <div className="settings-section">
                          <div className="settings-slider-header">
                            <label className="settings-label">色散强度</label>
                            <span className="settings-slider-value">{cat.aberrationIntensity}</span>
                          </div>
                          <div className="settings-slider-track">
                            <input type="range" min="0" max="10" step="0.1" value={cat.aberrationIntensity}
                              onChange={(e) => updateLgCat(lgActiveTab, { aberrationIntensity: parseFloat(e.target.value) })}
                              className="settings-slider" style={{ '--fill': `${(cat.aberrationIntensity / 10) * 100}%` } as React.CSSProperties} />
                          </div>
                        </div>

                        {lgActiveTab !== 'mask' && (
                          <div className="settings-section">
                            <div className="settings-slider-header">
                              <label className="settings-label">弹性</label>
                              <span className="settings-slider-value">{cat.elasticity.toFixed(2)}</span>
                            </div>
                            <div className="settings-slider-track">
                              <input type="range" min="0" max="1" step="0.01" value={cat.elasticity}
                                onChange={(e) => updateLgCat(lgActiveTab, { elasticity: parseFloat(e.target.value) })}
                                className="settings-slider" style={{ '--fill': `${cat.elasticity * 100}%` } as React.CSSProperties} />
                            </div>
                          </div>
                        )}

                        {lgActiveTab !== 'mask' && (
                          <div className="settings-section">
                            <div className="settings-slider-header">
                              <label className="settings-label">圆角半径</label>
                              <span className="settings-slider-value">{cat.cornerRadius}px</span>
                            </div>
                            <div className="settings-slider-track">
                              <input type="range" min="0" max="999" step="1" value={cat.cornerRadius}
                                onChange={(e) => updateLgCat(lgActiveTab, { cornerRadius: parseFloat(e.target.value) })}
                                className="settings-slider" style={{ '--fill': `${(cat.cornerRadius / 999) * 100}%` } as React.CSSProperties} />
                            </div>
                          </div>
                        )}
                      </>
                    );
                  })()}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
