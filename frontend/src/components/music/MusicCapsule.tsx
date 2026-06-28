import { forwardRef } from 'react';

interface Props {
  musicPlaying: boolean;
  hideGlass: boolean;
  showCapsule: boolean;
  hasFile: boolean;
  label: string;
  hasLyrics: boolean;
  currentText: string | null;
  pct: number;
  duration: number;
  lyricLineRef: React.RefObject<HTMLSpanElement | null>;
}

export const MusicCapsule = forwardRef<HTMLDivElement, Props>(function MusicCapsule({
  musicPlaying, hideGlass, showCapsule, hasFile, label,
  hasLyrics, currentText, pct, duration, lyricLineRef,
}, ref) {
  return (
    <div
      ref={ref}
      className={`music-capsule${hideGlass ? ' hide-glass' : ''}${showCapsule ? ' visible' : ''}${musicPlaying ? ' playing' : ''}`}
    >
      <div className="music-cap-disc">
        <div className="music-cap-disc-inner">
          <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" />
          </svg>
        </div>
      </div>
      <div className="music-cap-info">
        {hasLyrics && currentText ? (
          <span className="music-cap-lyric" ref={lyricLineRef}>{currentText}</span>
        ) : (
          <span className="music-cap-name">{hasFile ? label : '未选择音乐'}</span>
        )}
      </div>
      {duration > 0 && <div className="music-cap-progress"><div className="music-cap-progress-fill" style={{ width: `${pct}%` }} /></div>}
    </div>
  );
});
