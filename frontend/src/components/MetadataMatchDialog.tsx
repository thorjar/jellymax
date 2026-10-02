import { FormEvent, useEffect, useState } from "react";
import { api } from "../api/client";
import type { MetadataMatch } from "../api/types";
import { Spinner } from "./Spinner";

interface Props {
  itemId: string;
  initialName: string;
  initialYear?: number | null;
  onClose: () => void;
  onApplied: () => Promise<void>;
}

export function MetadataMatchDialog({ itemId, initialName, initialYear, onClose, onApplied }: Props) {
  const [name, setName] = useState(initialName);
  const [year, setYear] = useState(initialYear?.toString() ?? "");
  const [matches, setMatches] = useState<MetadataMatch[]>([]);
  const [searching, setSearching] = useState(true);
  const [applying, setApplying] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function search(useFilenameDefaults = false) {
    setSearching(true);
    setError(null);
    try {
      const result = await api.metadataMatches(
        itemId,
        useFilenameDefaults ? undefined : name.trim() || undefined,
        useFilenameDefaults ? undefined : year ? Number(year) : undefined,
      );
      setMatches(result.Items);
      if (useFilenameDefaults) {
        setName(result.Query);
        setYear(result.Year?.toString() ?? "");
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSearching(false);
    }
  }

  useEffect(() => {
    void search(true);
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
    // Search only when the dialog opens; the form handles later searches.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function submit(event: FormEvent) {
    event.preventDefault();
    await search(false);
  }

  async function apply(match: MetadataMatch) {
    setApplying(match.TmdbId);
    setError(null);
    try {
      await api.applyMetadataMatch(itemId, match.TmdbId);
      await onApplied();
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setApplying(null);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 p-3 backdrop-blur-sm"
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section role="dialog" aria-modal="true" aria-labelledby="metadata-title"
        className="flex max-h-[92vh] w-full max-w-4xl flex-col overflow-hidden rounded-2xl border border-edge bg-surface-raised shadow-2xl">
        <header className="flex items-center justify-between border-b border-edge px-5 py-4">
          <div>
            <h2 id="metadata-title" className="text-xl font-semibold">Edit movie metadata</h2>
            <p className="mt-1 text-sm text-ink-muted">Choose the TMDB movie that matches this file.</p>
          </div>
          <button type="button" className="btn" onClick={onClose} aria-label="Close">Close</button>
        </header>
        <form onSubmit={submit} className="flex flex-wrap gap-3 border-b border-edge p-4">
          <label className="min-w-52 flex-1 text-sm">
            <span className="mb-1 block text-ink-muted">Movie title</span>
            <input className="input w-full" value={name} onChange={(event) => setName(event.target.value)}
              autoFocus required />
          </label>
          <label className="w-28 text-sm">
            <span className="mb-1 block text-ink-muted">Year</span>
            <input className="input w-full" type="number" min="1870" max="2200" value={year}
              onChange={(event) => setYear(event.target.value)} placeholder="Optional" />
          </label>
          <button type="submit" className="btn btn-primary self-end" disabled={searching}>
            {searching ? "Searching…" : "Search TMDB"}
          </button>
        </form>
        <div className="overflow-y-auto p-4">
          {error && <p role="alert" className="mb-4 rounded-lg border border-danger/40 bg-danger/10 p-3 text-sm text-danger">{error}</p>}
          {searching ? <Spinner label="Searching TMDB…" /> : matches.length === 0 ? (
            <p className="py-10 text-center text-ink-muted">No matches found. Try a shorter title or remove the year.</p>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2">
              {matches.map((match) => (
                <article key={match.TmdbId} className="flex gap-3 rounded-xl border border-edge bg-surface p-3">
                  <div className="h-32 w-20 shrink-0 overflow-hidden rounded-lg bg-surface-hover">
                    {match.PosterUrl ? <img src={match.PosterUrl} alt="" className="h-full w-full object-cover" /> : null}
                  </div>
                  <div className="flex min-w-0 flex-1 flex-col">
                    <h3 className="font-semibold">{match.Title}</h3>
                    <p className="mt-1 text-xs text-ink-muted">
                      {match.Year ?? "Year unknown"}
                      {match.CommunityRating != null ? ` · ★ ${match.CommunityRating.toFixed(1)}` : ""}
                    </p>
                    <p className="mt-2 line-clamp-3 text-xs leading-relaxed text-ink-muted">
                      {match.Overview || "No description available."}
                    </p>
                    <button type="button" className="btn btn-primary mt-auto self-start"
                      disabled={applying != null} onClick={() => void apply(match)}>
                      {applying === match.TmdbId ? "Applying…" : "Use this match"}
                    </button>
                  </div>
                </article>
              ))}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
