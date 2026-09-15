// One tick is 100 ns on the backend (Jellyfin convention).
export const TICKS_PER_SECOND = 10_000_000;

export function ticksToSeconds(ticks?: number | null): number {
  if (!ticks || ticks <= 0) return 0;
  return Math.floor(ticks / TICKS_PER_SECOND);
}

export function formatTicks(ticks?: number | null): string {
  const totalSeconds = ticksToSeconds(ticks);
  if (totalSeconds <= 0) return "";
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const mm = String(minutes).padStart(2, "0");
  const ss = String(seconds).padStart(2, "0");
  return hours > 0 ? `${hours}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function formatSeconds(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return "0:00";
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = Math.floor(totalSeconds % 60);
  const mm = String(minutes).padStart(2, "0");
  const ss = String(seconds).padStart(2, "0");
  return hours > 0 ? `${hours}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function formatBytes(bytes?: number): string {
  if (!bytes || bytes <= 0) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function formatMediaType(type: string): string {
  switch (type) {
    case "Movie":
      return "Movie";
    case "Episode":
      return "Episode";
    case "Audio":
      return "Music";
    case "Video":
      return "Home Video";
    default:
      return type;
  }
}