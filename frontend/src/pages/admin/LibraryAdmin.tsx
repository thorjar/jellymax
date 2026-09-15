import { useCallback, useEffect, useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { api } from "../../api/client";
import type { Library } from "../../api/types";
import { DirectoryPicker } from "../../components/DirectoryPicker";
import { Spinner } from "../../components/Spinner";
import { librariesChanged } from "../../lib/libraryEvents";
import { groupLibraries } from "../../lib/libraryGroups";

const COLLECTION_TYPES = ["movies", "tvshows", "music", "homevideos"];

export function LibraryAdmin() {
  const [libraries, setLibraries] = useState<Library[] | null>(null);
  const [name, setName] = useState("");
  const [collectionType, setCollectionType] = useState("movies");
  const [location, setLocation] = useState("");
  const [pickerOpen, setPickerOpen] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [localServerName, setLocalServerName] = useState("This server");

  const load = useCallback(async () => {
    try {
      setLibraries(await api.libraries());
      setMessage(null);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    void api.systemInfo().then(info => setLocalServerName(info.ServerName)).catch(() => {});
  }, [load]);

  const groups = groupLibraries(libraries ?? [], localServerName);

  async function create(event: FormEvent) {
    event.preventDefault();
    setMessage(null);
    try {
      await api.createLibrary(name.trim(), collectionType, location.trim());
      setName("");
      setLocation("");
      await load();
      librariesChanged();
      try {
        await api.refreshLibrary();
        setMessage("Library created. Scanning for media now…");
      } catch (scanError) {
        setMessage(`Library created, but its scan could not start: ${scanError instanceof Error ? scanError.message : String(scanError)}`);
      }
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    }
  }

  async function remove(id: string, libraryName: string) {
    if (
      !window.confirm(
        `Remove local library “${libraryName}”? Media files stay on disk; catalog entries and user data are deleted.`
      )
    ) {
      return;
    }
    try {
      await api.deleteLibrary(id);
      await load();
      librariesChanged();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="card p-6">
      <h2 className="text-lg font-semibold">Add a library</h2>
      <form className="mt-4 grid gap-4 sm:grid-cols-2" onSubmit={create}>
        <div>
          <label className="label" htmlFor="lib-name">Name</label>
          <input id="lib-name" className="input" value={name}
            onChange={(e) => setName(e.target.value)} maxLength={128}
            placeholder="Movies" required />
        </div>
        <div>
          <label className="label" htmlFor="lib-type">Collection type</label>
          <select id="lib-type" className="input" value={collectionType}
            onChange={(e) => setCollectionType(e.target.value)}>
            {COLLECTION_TYPES.map((type) => (
              <option key={type} value={type}>{type}</option>
            ))}
          </select>
        </div>
        <div className="sm:col-span-2">
          <label className="label" htmlFor="lib-path">Media directory (on the server)</label>
          <div className="flex gap-2">
            <input id="lib-path" className="input flex-1" value={location}
              onChange={(e) => setLocation(e.target.value)}
              placeholder="Choose a folder on the server or type a path" required />
            <button type="button" className="btn shrink-0"
              onClick={() => setPickerOpen(true)}>
              Browse server folders…
            </button>
          </div>
        </div>
        <div className="sm:col-span-2">
          <button type="submit" className="btn btn-primary">Create library</button>
        </div>
      </form>

      <h2 className="mt-8 text-lg font-semibold">Libraries</h2>
      {message && (
        <p className="mt-3 rounded-lg border border-edge bg-surface-hover px-3 py-2 text-sm">
          {message}
        </p>
      )}
      {!libraries ? (
        <div className="mt-4">
          <Spinner label="Loading libraries…" />
        </div>
      ) : libraries.length === 0 ? (
        <div className="mt-4 rounded-xl border border-dashed border-edge py-12 text-center
          text-sm text-ink-muted">
          No libraries configured.
        </div>
      ) : (
        <div className="mt-5 space-y-7">
          {groups.map(group => (
            <section key={group.key} aria-label={`${group.name} libraries`}>
              <div className="mb-2 flex items-center gap-2 px-1">
                <span className={`h-2.5 w-2.5 rounded-full ${group.isLocal ? "bg-brand" : "bg-emerald-400"}`} />
                <div>
                  <h3 className="font-semibold">{group.name}</h3>
                  <p className="text-xs text-ink-muted">
                    {group.isLocal ? "Local server" : group.libraries[0]?.IsObjectStore ? "Object storage" : "Connected Jellyfin server"} · {group.libraries.length} {group.libraries.length === 1 ? "library" : "libraries"}
                  </p>
                </div>
              </div>
              <ul className="divide-y divide-edge overflow-hidden rounded-xl border border-edge bg-surface-raised">
                {group.libraries.map(library => (
                  <li key={library.ItemId}
                    className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
                    <Link to={`/library/${library.ItemId}`}
                      className="min-w-0 flex-1 rounded-md transition hover:text-brand-strong focus-visible:outline-2 focus-visible:outline-brand">
                      <div className="font-medium">{library.Name}</div>
                      <div className="muted truncate text-sm">
                        {library.CollectionType}
                        {library.Locations.length > 0 && ` · ${library.Locations.join(", ")}`}
                      </div>
                    </Link>
                    {group.isLocal && (
                      <button className="btn btn-danger shrink-0"
                        onClick={() => remove(library.ItemId, library.Name)}>
                        Remove
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}

      <DirectoryPicker
        open={pickerOpen}
        onClose={() => setPickerOpen(false)}
        onSelect={(path) => {
          setLocation(path);
          setPickerOpen(false);
        }}
      />
    </div>
  );
}
