/** Minimal inline icon set (no external icon dependency). */
export function Icon({ path, className = "h-5 w-5" }: { path: string; className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8}
      strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d={path} />
    </svg>
  );
}

export const ICONS = {
  home: "M3 10.5 12 3l9 7.5M5 9.5V21h5v-6h4v6h5V9.5",
  play: "M8 5.5v13l11-6.5-11-6.5Z",
  list: "M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01",
  search: "M21 21l-4.35-4.35M17 10.5a6.5 6.5 0 1 1-13 0 6.5 6.5 0 0 1 13 0Z",
  film: "M4 4h16v16H4V4Zm0 5.3h16M4 14.7h16M9.3 4v16M14.7 4v16",
  music:
    "M9 18.5V5.5l11-2v12M9 18.5a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0Zm11-3a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0Z",
  tv: "M3 6.5h18v11H3v-11ZM8.5 21h7",
  folder:
    "M3.5 6.5A1.5 1.5 0 0 1 5 5h4l2 2.5h8a1.5 1.5 0 0 1 1.5 1.5v9A1.5 1.5 0 0 1 19 19.5H5a1.5 1.5 0 0 1-1.5-1.5v-11.5Z",
  admin: "M12 3l7.5 3v5c0 4.5-3 8.5-7.5 10-4.5-1.5-7.5-5.5-7.5-10V6L12 3Z",
  logout:
    "M15 12H4m0 0 3.5-3.5M4 12l3.5 3.5M10 4.5h8a1.5 1.5 0 0 1 1.5 1.5v12a1.5 1.5 0 0 1-1.5 1.5h-8",
  menu: "M4 7h16M4 12h16M4 17h16",
  close: "M6 6l12 12M18 6 6 18",
  star: "m12 3.5 2.6 5.4 5.9.8-4.3 4.1 1 5.9-5.2-2.8-5.2 2.8 1-5.9-4.3-4.1 5.9-.8L12 3.5Z",
  refresh: "M20 11.5A8 8 0 0 0 6.3 6.3L4 8.5m0 0V3.5m0 5h5M4 12.5a8 8 0 0 0 13.7 5.2L20 15.5m0 0v5m0-5h-5",
  chevronLeft: "m15 18-6-6 6-6",
  chevronRight: "m9 18 6-6-6-6",
  more: "M5 12h.01M12 12h.01M19 12h.01",
  check: "m5 12 4 4L19 6",
  info: "M12 11v6M12 7h.01M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z",
  restart: "M4 8V3m0 5h5M4.8 16a8 8 0 1 0 .2-8",
};

const LIBRARY_ICONS: Record<string, string> = {
  movies: ICONS.film,
  tvshows: ICONS.tv,
  music: ICONS.music,
};

export function LibraryIcon({ type, className = "h-4.5 w-4.5 shrink-0" }: {
  type?: string;
  className?: string;
}) {
  return <Icon path={LIBRARY_ICONS[type ?? ""] ?? ICONS.folder} className={className} />;
}
