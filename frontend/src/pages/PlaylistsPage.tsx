import { useEffect, useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import type { Playlist } from "../api/types";
import { Spinner } from "../components/Spinner";

export function PlaylistsPage() {
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [name, setName] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function load() {
    try {
      const data = await api.playlists();
      setPlaylists(data.Items);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load();
  }, []);

  async function create(event: FormEvent) {
    event.preventDefault();
    if (!name.trim()) return;
    setNotice(null);
    try {
      await api.createPlaylist(name.trim(), []);
      setName("");
      await load();
      setNotice("Playlist created.");
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  }

  async function remove(id: string) {
    if (!window.confirm("Delete this playlist?")) return;
    try {
      await api.deletePlaylist(id);
      await load();
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <section>
      <h1 className="page-title mb-6">Playlists</h1>
      <form className="mb-6 flex max-w-md gap-2" onSubmit={create}>
        <input
          className="input"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="New playlist name…"
          aria-label="New playlist name"
          maxLength={128}
        />
        <button type="submit" className="btn btn-primary">
          Create
        </button>
      </form>
      {notice && (
        <p className="mb-4 rounded-xl border border-edge bg-surface-raised px-4 py-3 text-sm
          text-ink-muted">
          {notice}
        </p>
      )}
      {error && (
        <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm
          text-danger" role="alert">
          {error}
        </div>
      )}
      {loading ? (
        <Spinner label="Loading playlists…" />
      ) : playlists.length === 0 ? (
        <div className="rounded-xl border border-dashed border-edge py-16 text-center
          text-sm text-ink-muted">
          No playlists yet. Create one above.
        </div>
      ) : (
        <ul className="divide-y divide-edge overflow-hidden rounded-xl border border-edge
          bg-surface-raised">
          {playlists.map((playlist) => (
            <li key={playlist.Id} className="flex items-center justify-between gap-4 px-4 py-3">
              <Link to={`/playlists/${playlist.Id}`} className="min-w-0 flex-1">
                <span className="block truncate font-medium hover:text-brand-strong">
                  {playlist.Name}
                </span>
                <span className="text-xs text-ink-muted">
                  {playlist.ChildCount} item{playlist.ChildCount === 1 ? "" : "s"}
                </span>
              </Link>
              <button className="btn btn-danger" onClick={() => remove(playlist.Id)}>
                Delete
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}