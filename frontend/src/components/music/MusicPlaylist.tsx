import type { MusicTrack } from '../MusicPlayer';

interface Props {
  playlist: MusicTrack[];
  playlistIndex: number;
  musicPlaying: boolean;
  onSelectTrack: (index: number) => void;
  onRemoveTrack: (index: number) => void;
}

export function MusicPlaylist({ playlist, playlistIndex, musicPlaying, onSelectTrack, onRemoveTrack }: Props) {
  if (playlist.length === 0) return null;

  return (
    <div className="music-exp-playlist">
      <div className="music-exp-pl-header">
        <span>播放列表</span>
        <span className="music-exp-pl-count">{playlist.length} 首</span>
      </div>
      <div className="music-exp-pl-list">
        {playlist.map((track, i) => (
          <div
            key={i}
            className={`music-exp-pl-item${i === playlistIndex ? ' active' : ''}`}
            onClick={() => onSelectTrack(i)}
          >
            <span className="music-exp-pl-idx">
              {i === playlistIndex && musicPlaying
                ? <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><polygon points="6 3 20 12 6 21 6 3" /></svg>
                : i + 1}
            </span>
            <span className="music-exp-pl-name">{track.name}</span>
            <button className="music-exp-pl-del" onClick={(e) => { e.stopPropagation(); onRemoveTrack(i); }} title="移除" aria-label="移除">
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
