import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { Item } from "../api/types";
import { ItemGrid } from "../components/ItemGrid";
import { Spinner } from "../components/Spinner";

export function PlaylistDetailPage() {
  const { id = "" } = useParams();
  const [items, setItems] = useState<Item[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api
      .playlistItems(id)
      .then((data) => {
        if (!cancelled) setItems(data.Items);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [id]);

  return (
    <section>
      <div className="mb-6">
        <Link to="/playlists" className="text-sm text-ink-muted transition hover:text-ink">
          ← Playlists
        </Link>
        <h1 className="page-title mt-1">Playlist</h1>
      </div>
      {error && (
        <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm
          text-danger" role="alert">
          {error}
        </div>
      )}
      {loading ? (
        <Spinner label="Loading playlist…" />
      ) : (
        <ItemGrid items={items} emptyMessage="This playlist is empty." />
      )}
    </section>
  );
}