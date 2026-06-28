interface Props {
  hasLyrics: boolean;
  currentText: string | null;
  nextText: string | null;
  lineProgress: number;
  timingOffset: number;
  onOffsetChange: (offset: number) => void;
}

export function MusicLyrics({
  hasLyrics, currentText, nextText, lineProgress,
  timingOffset, onOffsetChange,
}: Props) {
  if (!hasLyrics) return null;

  return (
    <>
      <div className="music-exp-lyrics">
        <div className="music-exp-lyric-current">
          <span className="music-exp-lyric-line">
            {currentText || '···'}
          </span>
          {currentText && (
            <div className="music-exp-lyric-progress">
              <div className="music-exp-lyric-progress-fill" style={{ width: `${lineProgress * 100}%` }} />
            </div>
          )}
        </div>
        <span className={`music-exp-lyric-next${nextText ? '' : ' empty'}`}>
          {nextText || ' '}
        </span>
      </div>

      <div className="music-exp-offset">
        <button className="music-exp-offset-btn" onClick={() => onOffsetChange(Math.round((timingOffset - 0.5) * 10) / 10)}
          title="歌词提前 0.5 秒" aria-label="歌词提前">−</button>
        <span className="music-exp-offset-val" title="歌词时间偏移">
          {timingOffset > 0 ? '+' : ''}{timingOffset.toFixed(1)}s
        </span>
        <button className="music-exp-offset-btn" onClick={() => onOffsetChange(Math.round((timingOffset + 0.5) * 10) / 10)}
          title="歌词延后 0.5 秒" aria-label="歌词延后">+</button>
        {timingOffset !== 0 && (
          <button className="music-exp-offset-reset" onClick={() => onOffsetChange(0)}
            title="重置偏移" aria-label="重置偏移">↺</button>
        )}
      </div>
    </>
  );
}
