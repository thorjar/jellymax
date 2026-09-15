import { useEffect, useState } from "react";
import { Link, NavLink } from "react-router-dom";
import { useAuth } from "../auth/AuthContext";
import { api } from "../api/client";
import type { Library } from "../api/types";
import { ICONS, Icon } from "./icons";
import { readCatalogCache, writeCatalogCache } from "../lib/catalogCache";
import { LIBRARIES_CHANGED_EVENT } from "../lib/libraryEvents";
import { groupLibraries } from "../lib/libraryGroups";

function NavItem({ to, end, icon, children, onNavigate }: {
  to: string;
  end?: boolean;
  icon: string;
  children: string;
  onNavigate?: () => void;
}) {
  return (
    <NavLink to={to} end={end} onClick={onNavigate}
      className={({ isActive }) => `sidebar-link ${isActive ? "sidebar-link-active" : ""}`}>
      <Icon path={icon} />
      <span className="truncate">{children}</span>
    </NavLink>
  );
}

export function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const { user, logout } = useAuth();
  const [libraries, setLibraries] = useState<Library[]>(
    () => readCatalogCache<Library[]>("libraries") ?? []
  );
  const [localServerName, setLocalServerName] = useState("This server");
  // The logo <img> can fail to load (missing /jellymax-mark.svg, or a server
  // falling back to HTML with 200); fall back to an inline icon tile.
  const [markFailed, setMarkFailed] = useState(false);
  const isAdmin = user?.Policy?.IsAdministrator ?? false;

  useEffect(() => {
    let cancelled = false;
    const refresh = () => { void api.libraries()
      .then((libs) => {
        if (!cancelled) {
          setLibraries(libs);
          writeCatalogCache("libraries", libs);
        }
      })
      .catch(() => {
        // The sidebar still works without library entries.
      }); };
    refresh();
    void api.systemInfo().then(info => setLocalServerName(info.ServerName)).catch(() => {});
    window.addEventListener(LIBRARIES_CHANGED_EVENT, refresh);
    return () => {
      cancelled = true;
      window.removeEventListener(LIBRARIES_CHANGED_EVENT, refresh);
    };
  }, []);
  const groups = groupLibraries(libraries, localServerName);

  return (
    <>
      <Link to="/" onClick={onNavigate}
        className="mb-6 flex shrink-0 items-center gap-2.5 px-2 text-lg font-semibold tracking-tight text-ink">
        {markFailed ? (
          <span className="grid h-8 w-8 place-items-center rounded-lg bg-brand text-white">
            <Icon path={ICONS.play} className="h-4 w-4" />
          </span>
        ) : (
          <img src="/jellymax-mark.svg" alt="" className="h-8 w-8 rounded-lg"
            onError={() => setMarkFailed(true)} />
        )}
        <span>Jelly<span className="text-brand-strong">max</span></span>
      </Link>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain pr-1 [scrollbar-width:thin]">
        <nav className="space-y-1" aria-label="Primary">
          <NavItem to="/" end icon={ICONS.home} onNavigate={onNavigate}>Home</NavItem>
          <NavItem to="/libraries" icon={ICONS.folder} onNavigate={onNavigate}>Libraries</NavItem>
          <NavItem to="/resume" icon={ICONS.play} onNavigate={onNavigate}>Resume</NavItem>
          <NavItem to="/favorites" icon={ICONS.star} onNavigate={onNavigate}>Favorites</NavItem>
          <NavItem to="/playlists" icon={ICONS.list} onNavigate={onNavigate}>Playlists</NavItem>
        </nav>
        {groups.map(group => <div className="mt-6" key={group.key}>
          <p className="mb-1.5 flex items-center gap-2 px-3 text-[11px] font-semibold uppercase tracking-wider text-ink-muted/70"><span className={`h-1.5 w-1.5 rounded-full ${group.isLocal ? "bg-brand" : "bg-emerald-400"}`}/><span className="truncate">{group.name}</span></p>
          <nav className="space-y-1" aria-label={`${group.name} libraries`}>
            {group.libraries.map(library => <NavItem key={library.ItemId} to={`/library/${library.ItemId}`} icon={ICONS.folder} onNavigate={onNavigate}>{library.Name}</NavItem>)}
          </nav>
        </div>)}
        {isAdmin&&<div className="my-6"><p className="mb-1.5 px-3 text-[11px] font-semibold uppercase tracking-wider text-ink-muted/70">Administration</p><nav aria-label="Admin"><NavItem to="/admin" icon={ICONS.admin} onNavigate={onNavigate}>Dashboard</NavItem></nav></div>}
      </div>

      <div className="mt-3 shrink-0 border-t border-edge pt-4">
        <p className="truncate px-3 text-sm font-medium">{user?.Name ?? "…"}</p>
        <p className="px-3 text-xs text-ink-muted">{isAdmin ? "Administrator" : "User"}</p>
        <button type="button" onClick={() => void logout().then(onNavigate)}
          className="sidebar-link mt-2 w-full hover:text-danger">
          <Icon path={ICONS.logout} />
          <span>Log out</span>
        </button>
      </div>
    </>
  );
}
