type IconName = "play" | "pause" | "back10" | "forward10" | "volume" | "muted" | "captions" | "audioTracks" | "fullscreen" | "exitFullscreen" | "pip" | "exitPip";

export function PlaybackIcon({ name, className = "h-5 w-5" }: { name: IconName; className?: string }) {
  const common = { width: 24, height: 24, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, className, "aria-hidden": true as const };
  switch (name) {
    case "play": return <svg {...common}><path d="M8 5.5v13l10-6.5z" fill="currentColor" stroke="none" /></svg>;
    case "pause": return <svg {...common}><path d="M7 5h3v14H7zM14 5h3v14h-3z" fill="currentColor" stroke="none" /></svg>;
    case "back10": return <svg {...common}><path d="M4.5 8V4.5M4.5 8H8" /><path d="M4.7 8a8 8 0 1 1-.6 6" /><text x="12" y="15" textAnchor="middle" fill="currentColor" stroke="none" fontSize="7" fontWeight="700">10</text></svg>;
    case "forward10": return <svg {...common}><path d="M19.5 8V4.5M19.5 8H16" /><path d="M19.3 8a8 8 0 1 0 .6 6" /><text x="12" y="15" textAnchor="middle" fill="currentColor" stroke="none" fontSize="7" fontWeight="700">10</text></svg>;
    case "volume": return <svg {...common}><path d="M4 9v6h4l4 3V6L8 9z" /><path d="M15 9a4 4 0 0 1 0 6M17.5 6.5a7.5 7.5 0 0 1 0 11" /></svg>;
    case "muted": return <svg {...common}><path d="M4 9v6h4l4 3V6L8 9zM16 9l5 6M21 9l-5 6" /></svg>;
    case "captions": return <svg {...common}><rect x="2" y="4.5" width="20" height="15" rx="3" /><text x="12" y="15.2" textAnchor="middle" fill="currentColor" stroke="none" fontFamily="Arial, sans-serif" fontSize="9.5" fontWeight="800" letterSpacing="-.4">CC</text></svg>;
    case "audioTracks": return <svg {...common}><path d="M5 8v8M9 5v14M13 9v6M17 6v12M21 10v4" /></svg>;
    case "fullscreen": return <svg {...common}><path d="M8 3H4a1 1 0 0 0-1 1v4M16 3h4a1 1 0 0 1 1 1v4M3 16v4a1 1 0 0 0 1 1h4M21 16v4a1 1 0 0 1-1 1h-4" /></svg>;
    case "exitFullscreen": return <svg {...common}><path d="M3 9h5V4M21 9h-5V4M3 15h5v5M21 15h-5v5" /></svg>;
    case "pip": return <svg {...common}><rect x="2.5" y="4" width="19" height="16" rx="2" /><rect x="11.5" y="11" width="7" height="5.5" rx=".8" fill="currentColor" stroke="none" /></svg>;
    case "exitPip": return <svg {...common}><rect x="2.5" y="4" width="19" height="16" rx="2" /><path d="M13 12h6v5h-6zM10 14l-3 3M10 17H7v-3" /></svg>;
  }
}
