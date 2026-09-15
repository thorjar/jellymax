import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { api } from "../api/client";
import type { Item } from "../api/types";
import { ItemGrid } from "../components/ItemGrid";
import { Spinner } from "../components/Spinner";

export function SearchPage() {
  const [params] = useSearchParams();
  const query = params.get("q") ?? "";
  const [items, setItems] = useState<Item[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!query) {
      setItems([]);
      setTotal(0);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    api
      .getItems({ SearchTerm: query, Limit: 200 })
      .then((data) => {
        if (cancelled) return;
        setItems(data.Items);
        setTotal(data.TotalRecordCount);
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
  }, [query]);

  return (
    <section>
      <h1 className="page-title mb-6">
        Search results for <span className="text-brand-strong">“{query}”</span>
      </h1>
      {error && (
        <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm
          text-danger" role="alert">
          {error}
        </div>
      )}
      {loading ? (
        <Spinner label="Searching…" />
      ) : (
        <ItemGrid items={items} emptyMessage={`No matches for “${query}”.`} />
      )}
      {!loading && total > 0 && (
        <p className="mt-6 text-sm text-ink-muted">
          {total} result{total === 1 ? "" : "s"}
        </p>
      )}
    </section>
  );
}