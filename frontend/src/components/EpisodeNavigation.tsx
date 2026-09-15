import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import type { Item } from "../api/types";

export function EpisodeNavigation({ item, playback = false }: { item: Item; playback?: boolean }) {
  const [adjacent, setAdjacent] = useState<{ PreviousId: string | null; NextId: string | null } | null>(null);
  useEffect(() => {
    let cancelled = false;
    setAdjacent(null);
    api.adjacentEpisodes(item.Id).then((data) => { if (!cancelled) setAdjacent(data); }).catch(() => {});
    return () => { cancelled = true; };
  }, [item.Id]);
  const prefix = playback ? "/play" : "/items";
  return <nav aria-label="Episode navigation" className="mb-5 grid grid-cols-[1fr_auto_1fr]
    items-stretch overflow-hidden rounded-xl border border-edge bg-surface-raised shadow-sm">
    {adjacent?.PreviousId ? <Link to={`${prefix}/${adjacent.PreviousId}`}
      className="group flex min-h-16 items-center gap-3 px-4 py-3 transition hover:bg-surface-hover">
      <span className="text-xl text-brand-strong transition group-hover:-translate-x-0.5">←</span>
      <span><span className="block text-[10px] font-semibold uppercase tracking-widest text-ink-muted">Previous</span>
        <span className="text-sm font-medium text-ink">Episode</span></span>
    </Link> : <div aria-hidden="true" />}
    <Link to={`/items/${item.ParentId}`} className="flex min-w-32 flex-col items-center justify-center
      border-x border-edge px-4 py-3 text-center transition hover:bg-surface-hover">
      <span className="text-[10px] font-semibold uppercase tracking-widest text-ink-muted">Browse season</span>
      <span className="mt-0.5 text-sm font-semibold text-brand-strong">
        {item.ParentIndexNumber === 0 ? "Specials" : `Season ${item.ParentIndexNumber}`}
      </span>
    </Link>
    {adjacent?.NextId ? <Link to={`${prefix}/${adjacent.NextId}`}
      className="group flex min-h-16 items-center justify-end gap-3 px-4 py-3 text-right transition
        hover:bg-surface-hover">
      <span><span className="block text-[10px] font-semibold uppercase tracking-widest text-ink-muted">Next</span>
        <span className="text-sm font-medium text-ink">Episode</span></span>
      <span className="text-xl text-brand-strong transition group-hover:translate-x-0.5">→</span>
    </Link> : <div aria-hidden="true" />}
  </nav>;
}
