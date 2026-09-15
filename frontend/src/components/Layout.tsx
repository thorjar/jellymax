import { useState, type FormEvent } from "react";
import { Outlet, useNavigate } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { ICONS, Icon } from "./icons";

export function Layout() {
  const navigate = useNavigate();
  const [searchTerm, setSearchTerm] = useState("");
  const [drawerOpen, setDrawerOpen] = useState(false);

  function submitSearch(event: FormEvent) {
    event.preventDefault();
    const term = searchTerm.trim();
    if (term) navigate(`/search?q=${encodeURIComponent(term)}`);
  }

  return (
    <div className="flex min-h-screen">
      {/* Desktop sidebar */}
      <aside className="sticky top-0 hidden h-screen w-64 shrink-0 flex-col border-r border-edge
        bg-surface-raised p-4 lg:flex">
        <Sidebar />
      </aside>

      {/* Mobile drawer */}
      {drawerOpen && (
        <div className="fixed inset-0 z-40 lg:hidden" role="dialog" aria-modal="true">
          <div className="absolute inset-0 bg-black/60" onClick={() => setDrawerOpen(false)} />
          <aside className="absolute inset-y-0 left-0 flex w-72 flex-col border-r border-edge
            bg-surface-raised p-4 shadow-2xl">
            <button type="button" aria-label="Close menu" onClick={() => setDrawerOpen(false)}
              className="absolute right-3 top-3 z-10 rounded-lg p-1.5 text-ink-muted
                hover:bg-surface-hover hover:text-ink">
              <Icon path={ICONS.close} />
            </button>
            <Sidebar onNavigate={() => setDrawerOpen(false)} />
          </aside>
        </div>
      )}

      {/* Content column */}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-30 flex items-center gap-3 border-b border-edge
          bg-surface/90 px-4 py-3 backdrop-blur sm:px-6">
          <button type="button" aria-label="Open menu" onClick={() => setDrawerOpen(true)}
            className="rounded-lg p-1.5 text-ink-muted hover:bg-surface-hover hover:text-ink lg:hidden">
            <Icon path={ICONS.menu} />
          </button>
          <form onSubmit={submitSearch} role="search" className="relative w-full max-w-md">
            <Icon path={ICONS.search}
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-ink-muted" />
            <input type="search" value={searchTerm} onChange={(e) => setSearchTerm(e.target.value)}
              placeholder="Search movies, episodes, songs…" aria-label="Search media"
              className="input pl-9" />
          </form>
        </header>
        <main className="mx-auto w-full max-w-6xl flex-1 px-4 py-6 sm:px-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
