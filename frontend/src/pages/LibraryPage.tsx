import { useEffect, useState, type FormEvent } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { Item, Library } from "../api/types";
import { ItemGrid } from "../components/ItemGrid";
import { Spinner } from "../components/Spinner";
import { readCatalogCache, writeCatalogCache } from "../lib/catalogCache";

const PAGE_SIZE = 100;

export function LibraryPage() {
  const { id = "" } = useParams();
  const initial = readCatalogCache<{ Items: Item[]; TotalRecordCount: number }>(`library:${id}:0:`);
  const [items, setItems] = useState<Item[]>(initial?.Items ?? []);
  const [total, setTotal] = useState(initial?.TotalRecordCount ?? 0);
  const [startIndex, setStartIndex] = useState(0);
  const [searchTerm, setSearchTerm] = useState("");
  const [appliedSearch, setAppliedSearch] = useState("");
  const [loading, setLoading] = useState(!initial);
  const [error, setError] = useState<string | null>(null);
  const [library, setLibrary] = useState<Library | null>(null);

  useEffect(() => {
    setStartIndex(0); setSearchTerm(""); setAppliedSearch(""); setLibrary(null);
    let cancelled = false;
    const cachedLibraries = readCatalogCache<Library[]>("libraries") ?? [];
    setLibrary(cachedLibraries.find((entry) => entry.ItemId === id) ?? null);
    api.libraries().then((libraries) => {
      if (!cancelled) {
        writeCatalogCache("libraries", libraries);
        setLibrary(libraries.find((entry) => entry.ItemId === id) ?? null);
      }
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [id]);

  useEffect(() => {
    let cancelled = false;
    const cacheKey = `library:${id}:${startIndex}:${appliedSearch}`;
    const cached = readCatalogCache<{ Items: Item[]; TotalRecordCount: number }>(cacheKey);
    if (cached) { setItems(cached.Items); setTotal(cached.TotalRecordCount); }
    setLoading(!cached);
    setError(null);
    api
      .getItems({
        ParentId: id,
        SearchTerm: appliedSearch || undefined,
        StartIndex: startIndex,
        Limit: PAGE_SIZE,
      })
      .then((data) => {
        if (cancelled) return;
        setItems(data.Items);
        setTotal(data.TotalRecordCount);
        writeCatalogCache(cacheKey, data);
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
  }, [id, appliedSearch, startIndex]);

  function submitSearch(event: FormEvent) {
    event.preventDefault();
    setStartIndex(0);
    setAppliedSearch(searchTerm.trim());
  }

  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const page = Math.floor(startIndex / PAGE_SIZE) + 1;

  return (
    <section>
      <div className="mb-6">
        <Link to="/" className="text-sm text-ink-muted transition hover:text-ink">
          ← Libraries
        </Link>
        <h1 className="page-title mt-1">{library?.Name ?? "Library"}</h1>
        <p className="text-sm text-ink-muted">
          {library?.IsRemote && library.RemoteServerName ? `${library.RemoteServerName} · ` : ""}
          {total} items
        </p>
      </div>
      <form className="mb-6 flex max-w-md gap-2" onSubmit={submitSearch}>
        <input
          type="search"
          className="input"
          value={searchTerm}
          onChange={(e) => setSearchTerm(e.target.value)}
          placeholder="Filter this library…"
          aria-label="Filter this library"
        />
        <button type="submit" className="btn">
          Apply
        </button>
      </form>

      {error && (
        <div className="mb-4 rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm
          text-danger" role="alert">
          {error}
        </div>
      )}
      {loading ? (
        <Spinner label="Loading items…" />
      ) : (
        <>
          <ItemGrid items={items} emptyMessage="No items in this library." />
          {total > PAGE_SIZE && (
            <div className="mt-8 flex items-center justify-between">
              <button
                className="btn"
                disabled={startIndex === 0}
                onClick={() => setStartIndex(Math.max(0, startIndex - PAGE_SIZE))}
              >
                ← Previous
              </button>
              <span className="text-sm text-ink-muted">
                Page {page} of {pageCount} ({total} items)
              </span>
              <button
                className="btn"
                disabled={startIndex + PAGE_SIZE >= total}
                onClick={() => setStartIndex(startIndex + PAGE_SIZE)}
              >
                Next →
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
