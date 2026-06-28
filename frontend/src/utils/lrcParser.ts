export interface LrcLine {
  time: number; // seconds
  text: string;
}

/**
 * Parse LRC format string into sorted line array.
 * Supports: [mm:ss.xx], [mm:ss], [mm:ss.xxx]
 * Ignores metadata tags like [ar:], [ti:], [al:], [by:]
 */
export function parseLrc(lrc: string): LrcLine[] {
  const lines: LrcLine[] = [];
  const timeRe = /\[(\d{1,3}):(\d{2})(?:[.:·](\d{1,3}))?\]/g;
  const metaRe = /^\[[a-z]+:.*\]$/i;

  for (const raw of lrc.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;

    // Skip pure metadata lines with no time tags
    if (!line.includes('[') || metaRe.test(line)) continue;

    const times: number[] = [];
    let match: RegExpExecArray | null;
    let lastIdx = 0;

    while ((match = timeRe.exec(line)) !== null) {
      const mm = parseInt(match[1], 10);
      const ss = parseInt(match[2], 10);
      const msStr = match[3] || '0';
      // Normalize ms: "5" → 500, "50" → 500, "500" → 500
      const ms = msStr.length === 1 ? parseInt(msStr, 10) * 100
        : msStr.length === 2 ? parseInt(msStr, 10) * 10
        : parseInt(msStr, 10);
      times.push(mm * 60 + ss + ms / 1000);
      lastIdx = match.index + match[0].length;
    }

    const text = line.slice(lastIdx).trim();
    if (!text) continue;

    for (const t of times) {
      lines.push({ time: t, text });
    }
  }

  lines.sort((a, b) => a.time - b.time);
  return lines;
}

/**
 * Find the current lyric index for a given time using binary search.
 * Returns -1 if before the first line.
 *
 * The algorithm finds the last line whose start time <= current time.
 * This ensures lyrics switch exactly when the next line begins.
 */
export function findLyricIndex(lines: LrcLine[], time: number): number {
  if (lines.length === 0 || time < lines[0].time) return -1;

  // Handle single line case
  if (lines.length === 1) {
    return time >= lines[0].time ? 0 : -1;
  }

  // Binary search: find the last line whose time <= current time
  let lo = 0, hi = lines.length - 1;
  let result = -1;

  while (lo <= hi) {
    const mid = (lo + hi) >>> 1;
    if (lines[mid].time <= time) {
      result = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }

  return result;
}
