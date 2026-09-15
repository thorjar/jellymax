import { useEffect, useState } from "react";
import { api } from "../api/client";
import type { Item } from "../api/types";
import { useAuth } from "../auth/AuthContext";
import { ItemGrid } from "../components/ItemGrid";
import { Spinner } from "../components/Spinner";

export function ResumePage() {
  const { user } = useAuth();
  const [items, setItems] = useState<Item[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!user) return;
    let cancelled = false;
    setLoading(true);
    api
      .resume(user.Id)
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
  }, [user]);

  return (
    <section>
      <h1 className="page-title mb-6">Continue watching</h1>
      {error && (
        <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm
          text-danger" role="alert">
          {error}
        </div>
      )}
      {loading ? (
        <Spinner label="Loading resume list…" />
      ) : (
        <ItemGrid
          items={items}
          onRemove={(itemId) => setItems((current) => current.filter((item) => item.Id !== itemId))}
          emptyMessage="Nothing to resume. Start playing something and it will appear here."
        />
      )}
    </section>
  );
}
