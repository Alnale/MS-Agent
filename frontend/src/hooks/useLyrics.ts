import { useMemo } from 'react';
import { parseLrc, findLyricIndex, type LrcLine } from '../utils/lrcParser';

interface UseLyricsOptions {
  /** LRC string to parse */
  lrc: string | null;
  /** Current playback time in seconds */
  currentTime: number;
  /** Timing offset in seconds. Positive = lyrics delayed, negative = lyrics advanced. */
  offset?: number;
}

export interface UseLyricsReturn {
  /** All parsed lyric lines */
  lines: LrcLine[];
  /** Current lyric line index (-1 if before first line) */
  currentIndex: number;
  /** Current lyric text (empty string if no lyrics or before first line) */
  currentText: string;
  /** Next lyric text (for preview) */
  nextText: string;
  /** Progress within current line (0-1) */
  lineProgress: number;
  /** Current lyric line time (for debugging) */
  currentLineTime: number | null;
  /** Whether lyrics are loaded */
  hasLyrics: boolean;
}

export function useLyrics({ lrc, currentTime, offset = 0 }: UseLyricsOptions): UseLyricsReturn {
  const lines = useMemo(() => (lrc ? parseLrc(lrc) : []), [lrc]);
  // Apply timing offset: subtract offset from currentTime so positive offset delays lyrics
  const adjustedTime = currentTime - offset;
  const currentIndex = useMemo(() => findLyricIndex(lines, adjustedTime), [lines, adjustedTime]);

  // Calculate progress within the current line (0-1)
  const lineProgress = useMemo(() => {
    if (currentIndex < 0 || currentIndex >= lines.length) return 0;

    const currentLine = lines[currentIndex];
    const nextLine = currentIndex + 1 < lines.length ? lines[currentIndex + 1] : null;

    if (!nextLine) {
      // Last line: progress based on remaining time
      return Math.min(1, (adjustedTime - currentLine.time) / 5); // Assume 5 seconds for last line
    }

    const lineDuration = nextLine.time - currentLine.time;
    if (lineDuration <= 0) return 0;

    return Math.max(0, Math.min(1, (adjustedTime - currentLine.time) / lineDuration));
  }, [lines, currentIndex, adjustedTime]);

  // Get the time of the current lyric line (for debugging)
  const currentLineTime = currentIndex >= 0 ? lines[currentIndex].time : null;

  return {
    lines,
    currentIndex,
    currentText: currentIndex >= 0 ? lines[currentIndex].text : '',
    nextText: currentIndex >= 0 && currentIndex + 1 < lines.length ? lines[currentIndex + 1].text : '',
    lineProgress,
    currentLineTime,
    hasLyrics: lines.length > 0,
  };
}
