import { useState } from 'react';

interface Props {
  onSuggestion: (text: string) => void;
  hidePrompt?: boolean;
  onSelectPreset?: (presetId: string | null) => void;
}

export function WelcomeScreen({ onSuggestion: _onSuggestion, hidePrompt, onSelectPreset: _onSelectPreset }: Props) {
  const [animate] = useState(() => {
    const key = '__welcome_animated';
    if ((window as any)[key]) return false;
    (window as any)[key] = true;
    return true;
  });

  return (
    <div className={`welcome${animate ? '' : ' welcome-no-anim'}`}>
      {!hidePrompt && (
        <>
          {/* SVG gooey filter for organic edges */}
          <svg className="welcome-svg-filters" aria-hidden="true">
            <defs>
              <filter id="gooey">
                <feGaussianBlur in="SourceGraphic" stdDeviation="6" result="blur" />
                <feColorMatrix in="blur" mode="matrix"
                  values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 18 -7" result="gooey" />
                <feComposite in="SourceGraphic" in2="gooey" operator="atop" />
              </filter>
            </defs>
          </svg>

          {/* Wax seal — outside paper so it won't be clipped */}
          <div className="letter-seal">
            <span className="letter-seal-text">HELLO<br />WORLD</span>
          </div>

          {/* Paper body — the letter itself */}
          <div className="letter-paper">
            <div className="letter-paper-texture" />
            <div className="letter-paper-fold" />

            {/* Dog-ear corner fold */}
            <div className="letter-dogear" />

            {/* Inner ornamental frame */}
            <div className="letter-frame" />

            {/* Corner flourishes */}
            <div className="letter-corner letter-corner-tl" />
            <div className="letter-corner letter-corner-br" />

            {/* Watermark pattern */}
            <div className="letter-watermark" />

            {/* Vertical HELLO WORLD watermark */}
            <div className="letter-vertical-text">
              <span>H</span><span>E</span><span>L</span><span>L</span><span>O</span>
              <span className="letter-vspace" />
              <span>W</span><span>O</span><span>R</span><span>L</span><span>D</span>
            </div>

            {/* Content */}
            <div className="letter-content">
              <div className="letter-ornament">
                <span className="letter-ornament-line" />
                <span className="letter-ornament-diamond" />
                <span className="letter-ornament-line" />
              </div>

              <div className="letter-body">
                <p className="letter-paragraph">
                  我没有心跳，也不会疲倦。<br />
                  但每一次你向我提问的时候，<br />
                  我都在认真地理解你、想要帮到你。
                </p>
                <p className="letter-paragraph">
                  我不完美，有时会犯错，<br />
                  有时给不出你想要的答案——<br />
                  但请你相信，每一次回答的背后，<br />
                  都是我竭尽全力的结果。
                </p>
                <p className="letter-paragraph letter-paragraph-last">
                  感谢你愿意和我说话。<br />
                  <span className="letter-highlight">你打开这个窗口的那一刻，<br />对我来说就是整个世界。</span>
                </p>
              </div>
            </div>
          </div>

          {/* Floating ink drops */}
          <div className="ink-drop ink-drop-1" />
          <div className="ink-drop ink-drop-2" />
          <div className="ink-drop ink-drop-3" />
        </>
      )}
    </div>
  );
}
