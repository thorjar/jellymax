import { useCallback, useEffect, useState } from "react";
import { api } from "../api/client";
import type { DirectoryChild } from "../api/types";
import { Spinner } from "./Spinner";

interface DirectoryPickerProps {
  open: boolean;
  onClose: () => void;
  onSelect: (path: string) => void;
}

// The media folder lives on the machine running the backend, so the browser
// cannot pick it with <input type="file">. This dialog walks the server's
// filesystem through the admin-only /Library/Paths endpoint.
export function DirectoryPicker({ open, onClose, onSelect }: DirectoryPickerProps) {
  const [current, setCurrent] = useState<string | null>(null);
  const [parent, setParent] = useState<string | null>(null);
  const [directories, setDirectories] = useState<DirectoryChild[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (path?: string) => {
    setLoading(true);
    setError(null);
    try {
      const listing = await api.listDirectories(path);
      setCurrent(listing.Path ?? null);
      setParent(listing.Parent ?? null);
      setDirectories(listing.Directories);
    } catch (e) {
      setDirectories([]);
      setParent(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // Every time the dialog opens, start from the server's default location.
  useEffect(() => {
    if (open) {
      setCurrent(null);
      setParent(null);
      setDirectories([]);
      void load();
    }
  }, [open, load]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4"
      onClick={onClose}>
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" aria-hidden="true" />
      <div
        className="relative flex max-h-[85vh] w-full max-w-lg flex-col rounded-2xl border
          border-edge bg-surface shadow-2xl"
        role="dialog"
        aria-modal="true"
        aria-label="Choose a folder on the server"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-edge px-5 py-4">
          <h2 className="font-semibold">Choose a folder on the server</h2>
          <button type="button" onClick={onClose} aria-label="Close"
            className="rounded-lg p-1.5 text-ink-muted hover:bg-surface-hover hover:text-ink">
            ✕
          </button>
        </header>
        <div className="flex-1 overflow-y-auto px-5 py-4">
          <p className="muted text-sm">
            Folders are read on the machine running the jellymax server.
          </p>
          <div className="mt-3 truncate rounded-lg border border-edge bg-surface-raised
            px-3 py-2 font-mono text-xs text-ink-muted" title={current ?? ""}>
            {current ?? "Loading…"}
          </div>
          {error ? (
            <div className="mt-3 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2
              text-sm text-danger" role="alert">
              {error}
            </div>
          ) : loading ? (
            <div className="mt-6">
              <Spinner label="Loading folders…" />
            </div>
          ) : (
            <ul className="mt-3 overflow-hidden rounded-xl border border-edge
              bg-surface-raised">
              {parent && (
                <li>
                  <button type="button"
                    className="flex w-full items-center gap-2.5 border-b border-edge px-3
                      py-2.5 text-sm text-ink-muted hover:bg-surface-hover hover:text-ink"
                    onClick={() => load(parent)}>
                    ↰ <span className="truncate">.. (parent)</span>
                  </button>
                </li>
              )}
              {directories.length === 0 ? (
                <li className="px-3 py-6 text-center text-sm text-ink-muted">
                  {parent ? "No subdirectories here." : "No directories found."}
                </li>
              ) : (
                directories.map((directory, index) => (
                  <li key={directory.Path}>
                    <button type="button"
                      className={`flex w-full items-center gap-2.5 px-3 py-2.5 text-sm
                        hover:bg-surface-hover ${index < directories.length - 1
                          ? "border-b border-edge" : ""}`}
                      onClick={() => load(directory.Path)}>
                      <span aria-hidden="true">📁</span>
                      <span className="truncate">{directory.Name}</span>
                    </button>
                  </li>
                ))
              )}
            </ul>
          )}
        </div>
        <footer className="flex justify-end gap-2 border-t border-edge px-5 py-4">
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="btn btn-primary" disabled={!current}
            onClick={() => current && onSelect(current)}>
            Use this folder
          </button>
        </footer>
      </div>
    </div>
  );
}