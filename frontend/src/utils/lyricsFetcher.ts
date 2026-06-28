const ASR_ENDPOINT = 'https://token-plan-cn.xiaomimimo.com/v1/chat/completions';
const ASR_API_KEY = 'tp-ccqhvm8q4s70re5pz28unpp1s20zh3ddxxitn0j08z4zdm1g';
const ASR_MODEL = 'mimo-v2.5-asr';

// Timing constants for lyrics optimization
const MIN_LINE_DURATION = 1.0; // Minimum duration for a lyric line (seconds)
const MAX_LINE_DURATION = 8.0; // Maximum duration before splitting
const IDEAL_CHARS_PER_LINE = 15; // Ideal characters per lyric line
const GAP_BETWEEN_LINES = 0.3; // Gap between lyric lines (seconds)
const MIN_CHARS_FOR_MERGE = 5; // Minimum characters to consider merging short lines

/**
 * Get actual audio duration from a blob by decoding it into an Audio element.
 * Falls back to 0 if decoding fails.
 */
async function getAudioDuration(blob: Blob): Promise<number> {
  return new Promise<number>((resolve) => {
    try {
      const url = URL.createObjectURL(blob);
      const audio = new Audio();
      audio.preload = 'metadata';
      audio.onloadedmetadata = () => {
        const dur = audio.duration;
        URL.revokeObjectURL(url);
        resolve(isFinite(dur) && dur > 0 ? dur : 0);
      };
      audio.onerror = () => { URL.revokeObjectURL(url); resolve(0); };
      audio.src = url;
    } catch {
      resolve(0);
    }
  });
}

/**
 * Check if audio blob is valid for transcription.
 * Validates file type and size.
 */
function validateAudioBlob(blob: Blob): { valid: boolean; error?: string } {
  // Check file type
  const validTypes = ['audio/mpeg', 'audio/mp3', 'audio/wav', 'audio/ogg', 'audio/flac', 'audio/m4a', 'audio/aac'];
  if (blob.type && !validTypes.includes(blob.type)) {
    // Allow common audio extensions even if MIME type is missing
    console.warn('[ASR] Unexpected audio MIME type:', blob.type);
  }

  // Check file size (minimum 1KB, maximum 1GB)
  if (blob.size < 1024) {
    return { valid: false, error: 'Audio file is too small' };
  }
  if (blob.size > 1024 * 1024 * 1024) {
    return { valid: false, error: 'Audio file is too large (max 1GB)' };
  }

  return { valid: true };
}

/**
 * Transcribe audio blob using MiMo-V2.5-ASR with retry mechanism.
 * Returns transcribed text or null on failure.
 */
async function transcribeAudio(blob: Blob, maxRetries: number = 2): Promise<{ text: string; duration: number; segments?: Array<{ text: string; start: number; end: number }> } | null> {
  let lastError: Error | null = null;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      if (attempt > 0) {
        console.log(`[ASR] Retry attempt ${attempt}/${maxRetries}`);
        await new Promise(resolve => setTimeout(resolve, 1000 * attempt)); // Exponential backoff
      }

      const maxSize = 1024 * 1024 * 1024; // 1GB
      let audioBlob = blob;
      const trimmed = blob.size > maxSize;
      if (trimmed) {
        // Trim to first 1GB for ASR limit
        audioBlob = blob.slice(0, maxSize);
      }

      // Get actual audio duration from the original (untrimmed) blob
      const actualDuration = await getAudioDuration(blob);

      const arrayBuffer = await audioBlob.arrayBuffer();
      const bytes = new Uint8Array(arrayBuffer);
      let binary = '';
      for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
      const base64 = btoa(binary);
      const mimeType = blob.type || 'audio/mpeg';
      const dataUrl = `data:${mimeType};base64,${base64}`;

      const res = await fetch(ASR_ENDPOINT, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${ASR_API_KEY}`,
        },
        body: JSON.stringify({
          model: ASR_MODEL,
          messages: [{
            role: 'user',
            content: [{
              type: 'input_audio',
              input_audio: { data: dataUrl },
            }],
          }],
          asr_options: { language: 'auto' },
          stream: false,
        }),
      });

      if (!res.ok) {
        const errorText = await res.text().catch(() => 'Unknown error');
        console.error(`[ASR] API error (attempt ${attempt + 1}):`, res.status, res.statusText, errorText);

        // Don't retry on client errors (4xx)
        if (res.status >= 400 && res.status < 500) {
          return null;
        }

        lastError = new Error(`ASR API error: ${res.status} ${res.statusText}`);
        continue;
      }

      const data = await res.json();
      const msg = data?.choices?.[0]?.message;
      console.log('[ASR] API response keys:', Object.keys(data));
      console.log('[ASR] message keys:', msg ? Object.keys(msg) : 'none');
      console.log('[ASR] usage:', JSON.stringify(data?.usage));
      if (msg?.segments) console.log('[ASR] segments count:', msg.segments.length, 'sample:', JSON.stringify(msg.segments.slice(0, 2)));
      const text = msg?.content;
      // Use actual audio duration (from the original blob) instead of API-reported duration.
      // API duration only reflects the trimmed portion when audio exceeds 7MB.
      const apiDuration = data?.usage?.seconds || 0;
      const duration = actualDuration > 0 ? actualDuration : apiDuration;
      if (trimmed && actualDuration > 0) {
        console.log('[ASR] Audio was trimmed. Actual duration:', actualDuration.toFixed(1), 's, API duration:', apiDuration.toFixed(1), 's');
      }
      const segments = msg?.segments as Array<{ text: string; start: number; end: number }> | undefined;
      if (!text) {
        console.warn('[ASR] No text in response, full response:', JSON.stringify(data).slice(0, 300));
        // Don't retry if we got a response but no text
        return null;
      }

      console.log(`[ASR] Transcription successful (attempt ${attempt + 1})`);
      return { text, duration, segments };
    } catch (e) {
      console.error(`[ASR] Exception (attempt ${attempt + 1}):`, e);
      lastError = e instanceof Error ? e : new Error(String(e));

      // Don't retry on network errors that are likely permanent
      if (e instanceof TypeError && e.message.includes('Failed to fetch')) {
        console.error('[ASR] Network error, not retrying');
        return null;
      }
    }
  }

  console.error('[ASR] All retry attempts failed:', lastError);
  return null;
}

/**
 * Split text into lyric lines with intelligent segmentation.
 * Handles Chinese and English text with appropriate line breaks.
 */
function splitIntoLyricLines(text: string): string[] {
  // First split on sentence-ending punctuation
  const rawLines = text
    .split(/(?<=[。！？.!?\n])/)
    .map(s => s.trim())
    .filter(s => s.length > 0);

  const result: string[] = [];

  for (const line of rawLines) {
    // If line is short enough, keep as is
    if (line.length <= IDEAL_CHARS_PER_LINE) {
      result.push(line);
      continue;
    }

    // Try to split on clause boundaries
    const clauses = line.split(/(?<=[、，；,;])/).map(s => s.trim()).filter(s => s.length > 0);

    if (clauses.length > 1) {
      // Merge short clauses together
      let current = '';
      for (const clause of clauses) {
        if (current && (current.length + clause.length > IDEAL_CHARS_PER_LINE * 1.5)) {
          result.push(current);
          current = clause;
        } else {
          current = current ? current + clause : clause;
        }
      }
      if (current) result.push(current);
    } else {
      // No clause boundaries found, force split at ideal length
      const words = line.split(/(?<=[一-鿿㐀-䶿])|(?=[一-鿿㐀-䶿])/);
      let current = '';
      for (const word of words) {
        if (current.length + word.length > IDEAL_CHARS_PER_LINE && current.length > MIN_CHARS_FOR_MERGE) {
          result.push(current);
          current = word;
        } else {
          current += word;
        }
      }
      if (current) result.push(current);
    }
  }

  return result;
}

/**
 * Calculate syllable count for a text string.
 * CJK characters count as 1 syllable, Latin characters as 0.3.
 */
function countSyllables(text: string): number {
  let syllables = 0;
  for (const ch of text) {
    syllables += /[一-鿿㐀-䶿]/.test(ch) ? 1 : 0.3;
  }
  return Math.max(1, syllables);
}

/**
 * Convert plain ASR text into estimated LRC format.
 * Uses intelligent segmentation and proportional time distribution.
 */
function textToLrc(text: string, duration: number): string {
  const sentences = splitIntoLyricLines(text);
  if (sentences.length === 0) return '';

  // Calculate syllable counts for each sentence
  const syllableCounts = sentences.map(countSyllables);
  const totalSyllables = syllableCounts.reduce((a, b) => a + b, 0);

  // Estimate total duration if not provided
  const totalDur = duration > 0 ? duration : totalSyllables / 3.5 * 1.1; // fallback: 3.5 syllables/s + 10% pause

  // Calculate available time for singing (excluding gaps)
  const totalGap = GAP_BETWEEN_LINES * (sentences.length - 1);
  const singableDur = Math.max(totalDur - totalGap, totalSyllables / 4); // Ensure minimum singing time

  // Distribute time proportionally based on syllable count
  let t = 0;
  return sentences.map((s, i) => {
    const proportion = syllableCounts[i] / totalSyllables;
    const startTime = t;
    const lineDuration = singableDur * proportion;

    // Ensure minimum duration for readability
    const adjustedDuration = Math.max(lineDuration, MIN_LINE_DURATION);

    t += adjustedDuration + (i < sentences.length - 1 ? GAP_BETWEEN_LINES : 0);

    const mm = Math.floor(startTime / 60).toString().padStart(2, '0');
    const ss = (startTime % 60).toFixed(2).padStart(5, '0');
    return `[${mm}:${ss}]${s}`;
  }).join('\n');
}

/**
 * Adjust timestamps to delay lyrics until actual singing begins.
 * This handles songs with intros/instrumentals.
 */
function adjustForIntro(segments: Array<{ text: string; start: number; end: number }>): Array<{ text: string; start: number; end: number }> {
  if (segments.length === 0) return segments;

  // Find the first "real" singing segment
  // Look for a segment that's not too short and has reasonable duration
  let firstRealSegmentIdx = 0;
  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i];
    const duration = seg.end - seg.start;

    // A real singing segment should be at least 1.5 seconds
    // and start after at least 0.5 seconds from the beginning
    if (duration >= 1.5 && seg.start >= 0.5) {
      firstRealSegmentIdx = i;
      break;
    }

    // If we've checked 3 segments and found nothing, use the first one
    if (i >= 2) {
      firstRealSegmentIdx = 0;
      break;
    }
  }

  // If the first real segment starts after a delay, adjust all timestamps
  const firstRealStart = segments[firstRealSegmentIdx].start;
  if (firstRealStart > 2.0) {
    console.log(`[ASR] Detected intro delay: ${firstRealStart.toFixed(2)}s, adjusting timestamps...`);

    // Calculate the offset to apply
    const offset = firstRealStart;

    // Adjust all segments
    return segments.map((seg, idx) => {
      // Only adjust segments that are before the first real singing
      if (idx < firstRealSegmentIdx) {
        // Keep intro segments but mark them as not playable
        // by setting their start time to a very large value
        return { ...seg, start: 999999, end: 999999 };
      }
      // Adjust segments after the intro
      return {
        ...seg,
        start: Math.max(0, seg.start - offset),
        end: Math.max(0, seg.end - offset),
      };
    }).filter(seg => seg.start < 999999); // Remove intro segments
  }

  return segments;
}

/**
 * Optimize segments by merging short lines and splitting long lines.
 * Returns optimized segments with adjusted timestamps.
 */
function optimizeSegments(segments: Array<{ text: string; start: number; end: number }>, totalDuration: number): Array<{ text: string; start: number; end: number }> {
  if (segments.length === 0) return segments;

  // First, adjust for intro/instrumental
  let adjustedSegments = segments;
  if (totalDuration > 0) {
    adjustedSegments = adjustForIntro(segments);
  }

  const optimized: Array<{ text: string; start: number; end: number }> = [];
  let i = 0;

  while (i < adjustedSegments.length) {
    const current = adjustedSegments[i];
    const text = current.text.trim();
    const duration = current.end - current.start;

    // If line is too short, try to merge with next line
    if (duration < MIN_LINE_DURATION && i + 1 < adjustedSegments.length) {
      const next = adjustedSegments[i + 1];
      const nextText = next.text.trim();
      const nextDuration = next.end - next.start;

      // Only merge if combined duration is reasonable and total length is acceptable
      if (duration + nextDuration < MAX_LINE_DURATION && text.length + nextText.length < IDEAL_CHARS_PER_LINE * 2) {
        optimized.push({
          text: text + nextText,
          start: current.start,
          end: next.end,
        });
        i += 2;
        continue;
      }
    }

    // If line is too long, try to split
    if (duration > MAX_LINE_DURATION && text.length > IDEAL_CHARS_PER_LINE) {
      const splitPoint = findSplitPoint(text);
      if (splitPoint > 0 && splitPoint < text.length) {
        const ratio = splitPoint / text.length;
        const midTime = current.start + duration * ratio;

        optimized.push({
          text: text.substring(0, splitPoint).trim(),
          start: current.start,
          end: midTime,
        });
        optimized.push({
          text: text.substring(splitPoint).trim(),
          start: midTime,
          end: current.end,
        });
        i++;
        continue;
      }
    }

    // Keep as is
    optimized.push(current);
    i++;
  }

  return optimized;
}

/**
 * Find the best split point in a text line.
 * Prefers clause boundaries, falls back to word boundaries.
 */
function findSplitPoint(text: string): number {
  // Try to split on clause boundaries first
  const clauseSeparators = /[、，；,;]/g;
  let match;
  let bestSplit = -1;
  let bestDistance = Infinity;

  while ((match = clauseSeparators.exec(text)) !== null) {
    const distance = Math.abs(match.index - text.length / 2);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestSplit = match.index + 1;
    }
  }

  if (bestSplit > 0) return bestSplit;

  // Fall back to splitting at character boundaries (for CJK text)
  const midPoint = Math.floor(text.length / 2);
  return midPoint;
}

/**
 * Format a time value to LRC format (mm:ss.xx)
 */
function formatLrcTime(time: number): string {
  const mm = Math.floor(time / 60).toString().padStart(2, '0');
  const ss = (time % 60).toFixed(2).padStart(5, '0');
  return `${mm}:${ss}`;
}

/**
 * Fetch lyrics using MiMo-V2.5-ASR transcription.
 * Accepts audio blob and returns LRC string or null.
 */
export async function fetchLyricsFromASR(blob: Blob): Promise<string | null> {
  console.log('[ASR] fetchLyricsFromASR called, blob size:', blob.size, 'type:', blob.type);

  // Validate audio blob
  const validation = validateAudioBlob(blob);
  if (!validation.valid) {
    console.error('[ASR] Audio validation failed:', validation.error);
    return null;
  }

  const result = await transcribeAudio(blob);
  console.log('[ASR] transcribeAudio result:', result ? { textLen: result.text.length, duration: result.duration, hasSegments: !!result.segments, preview: result.text.slice(0, 100) } : null);
  if (!result || !result.text) return null;

  // Use API-provided timestamps if available
  if (result.segments && result.segments.length > 0) {
    const filtered = result.segments.filter(s => s.text?.trim());
    if (filtered.length > 0) {
      // Log original segment durations for debugging
      console.log('[ASR] Original segment durations:');
      filtered.forEach((s, i) => {
        const dur = s.end - s.start;
        console.log(`  [${i}] ${dur.toFixed(2)}s "${s.text.trim()}" (${s.start.toFixed(2)}-${s.end.toFixed(2)})`);
      });

      // Optimize segments with total duration for intro detection
      const optimized = optimizeSegments(filtered, result.duration);
      console.log('[ASR] Optimized segments:', optimized.length, '(from', filtered.length, ')');

      // Log optimized segment durations
      console.log('[ASR] Optimized segment durations:');
      optimized.forEach((s, i) => {
        const dur = s.end - s.start;
        console.log(`  [${i}] ${dur.toFixed(2)}s "${s.text.trim()}" (${s.start.toFixed(2)}-${s.end.toFixed(2)})`);
      });

      const lrc = optimized.map((s) => {
        const t = s.start;
        return `[${formatLrcTime(t)}]${s.text.trim()}`;
      }).join('\n');
      console.log('[ASR] Using optimized segment timestamps, lines:', lrc.split('\n').length);
      return lrc;
    }
  }

  // Fallback: estimate timestamps from text + duration
  const lrc = textToLrc(result.text, result.duration);
  console.log('[ASR] Using estimated timestamps, lines:', lrc.split('\n').length);
  return lrc;
}
