import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import type { Library } from "../api/types";
import { Spinner } from "../components/Spinner";
import { LibraryIcon } from "../components/icons";
import { groupLibraries } from "../lib/libraryGroups";
import { readCatalogCache, writeCatalogCache } from "../lib/catalogCache";

const TYPE_LABELS: Record<string, string> = { movies: "Movies", tvshows: "TV Shows", music: "Music", homevideos: "Home Videos" };

export function LibrariesPage() {
  const cached = readCatalogCache<Library[]>("libraries");
  const [libraries, setLibraries] = useState<Library[] | null>(cached);
  const [localServerName, setLocalServerName] = useState("This server");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.libraries().then(data=>{setLibraries(data);writeCatalogCache("libraries",data)}).catch((e) => setError(e instanceof Error ? e.message : String(e)));void api.systemInfo().then(info=>setLocalServerName(info.ServerName)).catch(()=>{}); }, []);
  if (error) return <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm text-danger">Could not load libraries: {error}</div>;
  if (!libraries) return <Spinner label="Loading libraries…" />;
  const groups=groupLibraries(libraries,localServerName);
  return <section><h1 className="page-title mb-6">Libraries</h1>
    {libraries.length===0?<div className="rounded-xl border border-dashed border-edge py-16 text-center text-sm text-ink-muted">No libraries yet. Ask an administrator to add one in the Admin dashboard.</div>:
    <div className="space-y-10">{groups.map(group=><section key={group.key}><div className="mb-4 flex items-center gap-3"><span className={`h-2.5 w-2.5 rounded-full ${group.isLocal?"bg-brand":"bg-emerald-400"}`}/><div><h2 className="text-xl font-semibold text-ink">{group.name}</h2><p className="text-xs text-ink-muted">{group.isLocal?"Local server":group.libraries[0]?.IsObjectStore?"Object storage":"Connected Jellyfin server"} · {group.libraries.length} {group.libraries.length===1?"library":"libraries"}</p></div></div><div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">{group.libraries.map(library=><Link key={library.ItemId} to={`/library/${library.ItemId}`} className="group flex flex-col items-center gap-3 rounded-xl border border-edge bg-surface-raised p-6 text-center transition hover:border-brand/50 hover:bg-surface-hover"><span className="grid h-12 w-12 place-items-center rounded-xl bg-brand/15 text-brand-strong transition group-hover:bg-brand group-hover:text-white"><LibraryIcon type={library.CollectionType} className="h-6 w-6" /></span><span className="font-medium">{library.Name}</span><span className="text-xs text-ink-muted">{TYPE_LABELS[library.CollectionType]??library.CollectionType}</span></Link>)}</div></section>)}</div>}
  </section>;
}
