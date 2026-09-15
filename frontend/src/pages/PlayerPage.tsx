import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { Item } from "../api/types";
import { EpisodeNavigation } from "../components/EpisodeNavigation";
import { Player } from "../components/Player";
import { Spinner } from "../components/Spinner";

export function PlayerPage() {
  const { id = "" } = useParams();
  const [item, setItem] = useState<Item | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setItem(null);
    setError(null);
    api
      .getItem(id)
      .then((data) => {
        if (!cancelled) setItem(data);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [id]);

  return (
    <section>
      <Link to={`/items/${id}`} className="mb-4 inline-flex items-center gap-1.5 text-sm
        text-ink-muted hover:text-ink">
        ← Back to item
      </Link>
      {error && (
        <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm
          text-danger" role="alert">
          {error}
        </div>
      )}
      {!item && !error && <Spinner label="Loading item…" />}
      {item?.Type === "Episode" && <EpisodeNavigation item={item} playback />}
      {item && (item.IsFolder ? <Link to={`/items/${item.Id}`}>Browse {item.Name}</Link> : <Player key={item.Id} item={item} />)}
    </section>
  );
}