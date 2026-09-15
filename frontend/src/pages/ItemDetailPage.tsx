import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { Item, Playlist } from "../api/types";
import { useAuth } from "../auth/AuthContext";
import { EpisodeNavigation } from "../components/EpisodeNavigation";
import { ItemGrid } from "../components/ItemGrid";
import { ItemImage } from "../components/ItemImage";
import { Spinner } from "../components/Spinner";
import { StreamsTable } from "../components/StreamsTable";
import { ICONS, Icon } from "../components/icons";
import { formatBytes, formatTicks, formatMediaType } from "../lib/format";
import { readCatalogCache, writeCatalogCache } from "../lib/catalogCache";

export function ItemDetailPage() {
  const { id = "" } = useParams();
  const { user } = useAuth();
  const [item, setItem] = useState<Item | null>(() => readCatalogCache<Item>(`item:${id}`));
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [selectedPlaylist, setSelectedPlaylist] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [children, setChildren] = useState<Item[]>([]);
  const [childTotal, setChildTotal] = useState(0);
  const [childPage, setChildPage] = useState(0);
  const [childrenLoading, setChildrenLoading] = useState(false);
  const [childError, setChildError] = useState<string | null>(null);
  const [imageRevision, setImageRevision] = useState(0);
  const isAdmin = user?.Policy?.IsAdministrator ?? false;

  useEffect(() => {
    let cancelled = false;
    setItem(readCatalogCache<Item>(`item:${id}`));
    setNotice(null);
    setChildren([]);
    setChildPage(0);
    setError(null);
    api
      .getItem(id)
      .then((data) => {
        if (!cancelled) { setItem(data); writeCatalogCache(`item:${id}`, data); }
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    api
      .playlists()
      .then((data) => {
        if (!cancelled) {
          setPlaylists(data.Items);
          if (data.Items.length > 0) setSelectedPlaylist(data.Items[0].Id);
        }
      })
      .catch(() => {
        // Playlist listing is optional on this page.
      });
    return () => {
      cancelled = true;
    };
  }, [id]);

  useEffect(() => {
    if (!item?.IsFolder) return;
    let cancelled = false;
    setChildrenLoading(true);
    setChildError(null);
    api.getItems({ ParentId: item.Id, StartIndex: childPage * 100, Limit: 100 })
      .then((data) => { if (!cancelled) { setChildren(data.Items); setChildTotal(data.TotalRecordCount); } })
      .catch((e) => { if (!cancelled) setChildError(String(e.message ?? e)); })
      .finally(() => { if (!cancelled) setChildrenLoading(false); });
    return () => { cancelled = true; };
  }, [item?.Id, item?.IsFolder, childPage]);

  if (error) {
    return (
      <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm
        text-danger" role="alert">
        {error}
      </div>
    );
  }
  if (!item) return <Spinner label="Loading item…" />;

  const userData = item.UserData;
  const isFavorite = userData?.IsFavorite ?? false;
  const isPlayed = userData?.Played ?? false;
  const resumeTicks = userData?.PlaybackPositionTicks ?? 0;
  const genres = (item.Genres ?? []).filter(Boolean);
  const tmdbUrl = item.Type === "Movie" && item.TmdbId
    ? `https://www.themoviedb.org/movie/${item.TmdbId}`
    : item.SeriesTmdbId
      ? `https://www.themoviedb.org/tv/${item.SeriesTmdbId}${item.Type === "Season" ? `/season/${item.IndexNumber}` : item.Type === "Episode" ? `/season/${item.ParentIndexNumber}/episode/${item.IndexNumber}` : ""}`
      : null;

  async function toggleFavorite() {
    if (!user || !item) return;
    try {
      await api.setFavorite(user.Id, item.Id, !isFavorite);
      const fresh = await api.getItem(item.Id);
      setItem(fresh);
      writeCatalogCache(`item:${item.Id}`, fresh);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  }

  async function togglePlayed() {
    if (!user || !item) return;
    try {
      await api.setPlayed(user.Id, item.Id, !isPlayed);
      const fresh = await api.getItem(item.Id);
      setItem(fresh);
      writeCatalogCache(`item:${item.Id}`, fresh);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  }

  async function addToPlaylist() {
    if (!selectedPlaylist || !item) return;
    try {
      await api.appendPlaylistItems(selectedPlaylist, [item.Id]);
      setNotice("Added to playlist.");
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  }

  async function refreshMetadata() {
    if (!item) return;
    setRefreshing(true);
    setNotice(null);
    try {
      const result = await api.refreshMetadata(item.Id);
      const fresh = await api.getItem(item.Id);
      setItem(fresh);
      writeCatalogCache(`item:${item.Id}`, fresh);
      setImageRevision((value) => value + 1);
      setNotice(
        result.Matched
          ? "TMDb metadata updated."
          : "TMDb had no match for this title."
      );
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    } finally {
      setRefreshing(false);
    }
  }

  const videoStreams = (item.MediaStreams ?? []).filter((s) => s.Type === "Video");
  const audioStreams = (item.MediaStreams ?? []).filter((s) => s.Type === "Audio");
  const subtitleStreams = (item.MediaStreams ?? []).filter((s) => s.Type === "Subtitle");

  return (
    <section>
      <nav aria-label="Breadcrumb" className="mb-5 flex flex-wrap items-center gap-2 text-sm text-ink-muted">
        <Link className="rounded-md px-2 py-1 transition hover:bg-surface-hover hover:text-ink"
          to={`/library/${item.LibraryId ?? item.ParentId}`}>Library</Link>
        {item.SeriesId && item.SeriesId !== item.Id && <><span aria-hidden="true">›</span>
          <Link className="rounded-md px-2 py-1 transition hover:bg-surface-hover hover:text-ink"
            to={`/items/${item.SeriesId}`}>{item.SeriesName ?? "Series"}</Link></>}
        {item.Type === "Episode" && <><span aria-hidden="true">›</span>
          <Link className="rounded-md px-2 py-1 transition hover:bg-surface-hover hover:text-ink"
            to={`/items/${item.ParentId}`}>{item.ParentIndexNumber === 0 ? "Specials" : `Season ${item.ParentIndexNumber}`}</Link></>}
        <span aria-hidden="true">›</span><span className="px-2 py-1 font-medium text-ink">{item.Name}</span>
      </nav>
      {item.Type === "Episode" && <EpisodeNavigation item={item} />}
      {/* Hero */}
      <div className="overflow-hidden rounded-2xl border border-edge bg-surface-raised">
        <div className="flex flex-col gap-6 p-6 sm:flex-row">
          <div className={item.Type === "Episode" ? "w-full shrink-0 sm:w-80 lg:w-[28rem]" : "w-40 shrink-0 sm:w-48"}>
            <div className={`${item.Type === "Episode" ? "aspect-video" : "aspect-[2/3]"} overflow-hidden rounded-xl border border-edge shadow-lg shadow-black/30`}>
              <ItemImage itemId={item.Id} name={item.Name} revision={imageRevision} className="h-full w-full" />
            </div>
          </div>
          <div className="min-w-0 flex-1">
            <h1 className="text-2xl font-semibold tracking-tight">{item.Name}</h1>
            <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
              <span className="rounded-full bg-brand/15 px-2.5 py-1 font-medium text-brand-strong">
                {formatMediaType(item.Type)}
              </span>
              {item.Year != null && (
                <span className="rounded-full border border-edge px-2.5 py-1 text-ink-muted">
                  {item.Year}
                </span>
              )}
              {item.CommunityRating != null && (
                <span className="flex items-center gap-1 rounded-full border border-edge px-2.5
                  py-1 text-amber-400">
                  <Icon path={ICONS.star} className="h-3.5 w-3.5" />
                  {item.CommunityRating.toFixed(1)}
                </span>
              )}
              {item.Container && (
                <span className="rounded-full border border-edge px-2.5 py-1 uppercase
                  text-ink-muted">
                  {item.Container}
                </span>
              )}
              {formatTicks(item.RunTimeTicks) && (
                <span className="rounded-full border border-edge px-2.5 py-1 text-ink-muted">
                  {formatTicks(item.RunTimeTicks)}
                </span>
              )}
              {formatBytes(item.Size) && (
                <span className="rounded-full border border-edge px-2.5 py-1 text-ink-muted">
                  {formatBytes(item.Size)}
                </span>
              )}
            </div>

            {genres.length > 0 && (
              <p className="mt-3 text-sm text-ink-muted">{genres.join(" · ")}</p>
            )}
            {item.Overview && (
              <p className="mt-3 max-w-prose text-sm leading-relaxed text-ink">
                {item.Overview}
              </p>
            )}
            {tmdbUrl && (
              <a href={tmdbUrl} target="_blank" rel="noreferrer"
                className="mt-3 inline-flex items-center gap-1.5 text-xs font-medium
                  text-brand-strong hover:underline">
                View on TMDb
                <span aria-hidden="true">↗</span>
              </a>
            )}

            {resumeTicks > 0 && !isPlayed && (
              <p className="mt-3 text-sm text-ink-muted">
                Resume from {formatTicks(resumeTicks)}
              </p>
            )}

            <div className="mt-5 flex flex-wrap items-center gap-2">
              {!item.IsFolder && <Link to={`/play/${item.Id}`} className="btn btn-primary">
                {resumeTicks > 0 && !isPlayed ? "▶ Resume" : "▶ Play"}
              </Link>}
              <button type="button" className="btn" onClick={toggleFavorite}>
                {isFavorite ? "★ Favorited" : "☆ Favorite"}
              </button>
              {!item.IsFolder && <button type="button" className="btn" onClick={togglePlayed}>
                {isPlayed ? "Mark unplayed" : "Mark played"}
              </button>}
              {isAdmin && ["Movie", "Series", "Season", "Episode"].includes(item.Type) && (
                <button type="button" className="btn" onClick={refreshMetadata}
                  disabled={refreshing}>
                  {refreshing ? "Refreshing…" : "↻ Refresh metadata"}
                </button>
              )}
              {!item.IsFolder && playlists.length > 0 && (
                <span className="flex items-center gap-2">
                  <select className="input w-auto" value={selectedPlaylist}
                    onChange={(e) => setSelectedPlaylist(e.target.value)}
                    aria-label="Choose playlist">
                    {playlists.map((p) => (
                      <option key={p.Id} value={p.Id}>
                        {p.Name}
                      </option>
                    ))}
                  </select>
                  <button type="button" className="btn" onClick={addToPlaylist}>
                    Add to playlist
                  </button>
                </span>
              )}
            </div>
            {notice && (
              <p className="mt-3 rounded-lg border border-edge bg-surface px-3 py-2 text-sm
                text-ink-muted">
                {notice}
              </p>
            )}
          </div>
        </div>
      </div>

      {item.IsFolder ? <section className="mt-6" aria-label={item.Type === "Series" ? "Seasons" : "Episodes"}>
        <h2 className="mb-4 text-xl font-semibold">{item.Type === "Series" ? "Seasons" : "Episodes"}</h2>
        {childError && <p role="alert">{childError}</p>}
        {childrenLoading ? <Spinner label="Loading…" /> : <ItemGrid items={children}
          layout={item.Type === "Season" ? "episode" : "poster"}
          emptyMessage="No scanned items found." />}
        {childTotal > 100 && <div className="mt-4 flex gap-4">
          <button className="btn" disabled={childPage === 0} onClick={() => setChildPage(childPage - 1)}>Previous</button>
          <span>Page {childPage + 1} of {Math.ceil(childTotal / 100)}</span>
          <button className="btn" disabled={(childPage + 1) * 100 >= childTotal} onClick={() => setChildPage(childPage + 1)}>Next</button>
        </div>}
      </section> : <div className="mt-6 grid gap-4">
        <StreamsTable
          title="Video streams"
          streams={videoStreams}
          fallback="No video stream metadata."
        />
        <StreamsTable
          title="Audio streams"
          streams={audioStreams}
          fallback="No audio stream metadata."
        />
        <StreamsTable
          title="Subtitles"
          streams={subtitleStreams}
          fallback="No subtitle streams."
        />
      </div>}
    </section>
  );
}
