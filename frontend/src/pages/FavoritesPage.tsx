import { useCallback, useEffect, useState } from "react";
import { api } from "../api/client";
import type { Item } from "../api/types";
import { ItemGrid } from "../components/ItemGrid";
import { Spinner } from "../components/Spinner";
import { FAVORITES_CHANGED_EVENT } from "../lib/favoriteEvents";
import { readCatalogCache, writeCatalogCache } from "../lib/catalogCache";

export function FavoritesPage() {
  const cached = readCatalogCache<Item[]>("favorites");
  const [items, setItems] = useState<Item[]>(cached ?? []);
  const [loading, setLoading] = useState(!cached);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(() => {
    void api.getItems({ IsFavorite: true, Limit: 200 }).then(data => {
      setItems(data.Items); writeCatalogCache("favorites", data.Items); setError(null);
    }).catch(reason => setError(reason instanceof Error ? reason.message : String(reason))).finally(() => setLoading(false));
  }, []);
  useEffect(() => {
    refresh();
    window.addEventListener(FAVORITES_CHANGED_EVENT, refresh);
    return () => window.removeEventListener(FAVORITES_CHANGED_EVENT, refresh);
  }, [refresh]);
  if (loading) return <Spinner label="Loading favorites…"/>;
  return <section>
    <div className="mb-6"><p className="mb-1 text-sm font-semibold uppercase tracking-[0.18em] text-brand-strong">Your collection</p><h1 className="page-title">Favorites</h1></div>
    {error&&<div className="mb-5 rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm text-danger" role="alert">Could not load favorites: {error}</div>}
    <ItemGrid items={items} emptyMessage="Titles you add to favorites will appear here."/>
  </section>;
}
