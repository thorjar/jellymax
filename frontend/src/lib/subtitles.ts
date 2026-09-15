export interface SubtitleCue { start: number; end: number; text: string }

function timestamp(value: string): number | null {
  const parts = value.replace(',', '.').split(':').map(Number);
  if (parts.length < 2 || parts.length > 3 || parts.some((part) => !Number.isFinite(part))) return null;
  return parts.reduce((seconds, part) => seconds * 60 + part, 0);
}

export function parseWebVtt(source: string): SubtitleCue[] {
  return source.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n').split(/\n\s*\n/).flatMap((block) => {
    const lines = block.split('\n');
    const timing = lines.findIndex((line) => line.includes('-->'));
    if (timing < 0) return [];
    const [rawStart, rawEnd] = lines[timing].split('-->').map((part) => part.trim().split(/\s+/)[0]);
    const start = timestamp(rawStart), end = timestamp(rawEnd);
    if (start === null || end === null || end <= start) return [];
    const text = lines.slice(timing + 1).join('\n').replace(/<[^>]*>/g, '')
      .replace(/&nbsp;/g, ' ').replace(/&lt;/g, '<').replace(/&gt;/g, '>')
      .replace(/&amp;/g, '&').trim();
    return text ? [{ start, end, text }] : [];
  });
}

export function visibleSubtitle(cues: SubtitleCue[], time: number): string {
  return cues.filter((cue) => cue.start <= time && time < cue.end).map((cue) => cue.text).join('\n');
}

export function toWebVtt(source: string, fileName: string): string {
  const text = source.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n');
  if (fileName.toLowerCase().endsWith('.vtt') || text.trimStart().startsWith('WEBVTT')) {
    if (!text.trimStart().startsWith('WEBVTT')) throw new Error('This VTT file is invalid.');
    return text;
  }
  if (!fileName.toLowerCase().endsWith('.srt')) throw new Error('Import an SRT or VTT subtitle file.');
  const converted = text.split('\n').map((line) => line.includes(' --> ') ? line.replace(/,/g, '.') : line).join('\n');
  const vtt = `WEBVTT\n\n${converted}`;
  if (parseWebVtt(vtt).length === 0) throw new Error('This subtitle file contains no readable cues.');
  return vtt;
}
