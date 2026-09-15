import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import type { Item, Playlist, UserData } from "../api/types";
import { useAuth } from "../auth/AuthContext";
import { formatTicks } from "../lib/format";
import { announceFavoritesChanged } from "../lib/favoriteEvents";
import { Icon, ICONS } from "./icons";
import { ItemImage } from "./ItemImage";

interface ItemCardProps { item: Item; layout?: "poster" | "episode"; onRemove?: (itemId: string) => void; }

function DetailModal({ item, onClose }: { item: Item; onClose: () => void }) {
  useEffect(() => {
    const close = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    document.addEventListener("keydown", close);
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => { document.removeEventListener("keydown", close); document.body.style.overflow = previous; };
  }, [onClose]);
  return createPortal(
    <div className="fixed inset-0 z-[100] grid place-items-center overflow-y-auto bg-black/75 p-4 backdrop-blur-sm" role="dialog" aria-modal="true" aria-label={`${item.Name} details`} onMouseDown={onClose}>
      <article className="relative my-auto grid w-full max-w-3xl overflow-hidden rounded-2xl border border-edge bg-surface-raised shadow-2xl sm:grid-cols-[15rem_1fr]" onMouseDown={(event) => event.stopPropagation()}>
        <button type="button" onClick={onClose} aria-label="Close details" className="absolute right-3 top-3 z-10 grid h-10 w-10 place-items-center rounded-full bg-black/70 text-white backdrop-blur hover:bg-black"><Icon path={ICONS.close}/></button>
        <div className="aspect-[2/3] bg-surface sm:aspect-auto sm:min-h-[24rem]"><ItemImage itemId={item.Id} name={item.Name} className="h-full w-full"/></div>
        <div className="flex flex-col justify-center p-6 sm:p-8">
          <p className="mb-2 text-xs font-semibold uppercase tracking-[0.16em] text-brand-strong">{item.Type}</p>
          <h2 className="pr-10 text-3xl font-semibold tracking-tight text-ink">{item.Name}</h2>
          <p className="mt-3 text-sm text-ink-muted">{[item.Year,item.CommunityRating?`★ ${item.CommunityRating.toFixed(1)}`:null,formatTicks(item.RunTimeTicks)].filter(Boolean).join(" · ")}</p>
          {!!item.Genres?.length&&<p className="mt-3 text-sm text-ink-muted">{item.Genres.join(" · ")}</p>}
          <p className="mt-5 max-h-40 overflow-y-auto text-sm leading-6 text-ink-muted">{item.Overview||"No description is available for this title."}</p>
          <Link to={`/items/${item.Id}`} onClick={onClose} className="mt-7 inline-flex w-fit items-center gap-2 rounded-lg bg-brand px-4 py-2.5 text-sm font-semibold text-white hover:brightness-110"><Icon path={ICONS.info}/>Full details</Link>
        </div>
      </article>
    </div>, document.body);
}

export function ItemCard({ item, layout = "poster", onRemove }: ItemCardProps) {
  const { user } = useAuth();
  const [current,setCurrent]=useState(item),[menuOpen,setMenuOpen]=useState(false),[detailsOpen,setDetailsOpen]=useState(false),[busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null),[notice,setNotice]=useState<string|null>(null);
  const [playlists,setPlaylists]=useState<Playlist[]|null>(null),[playlistOpen,setPlaylistOpen]=useState(false);
  const menu=useRef<HTMLDivElement>(null);
  useEffect(()=>setCurrent(item),[item]);
  useEffect(()=>{if(!menuOpen)return;const close=(event:MouseEvent)=>{if(!menu.current?.contains(event.target as Node))setMenuOpen(false)};document.addEventListener("mousedown",close);return()=>document.removeEventListener("mousedown",close)},[menuOpen]);
  const duration=formatTicks(current.RunTimeTicks),resume=(current.UserData?.PlaybackPositionTicks??0)>0&&!current.UserData?.Played,episode=layout==="episode"||current.Type==="Episode";
  const episodeLabel=`S${String(current.ParentIndexNumber??0).padStart(2,"0")} · E${String(current.IndexNumber??0).padStart(2,"0")}`;
  const updateUserData=(data:UserData)=>setCurrent(value=>({...value,UserData:data}));
  async function togglePlayed(){if(!user||busy)return;setBusy(true);setError(null);try{const data=await api.setPlayed(user.Id,current.Id,!current.UserData?.Played);updateUserData(data);if(data.Played)onRemove?.(current.Id);setMenuOpen(false)}catch(reason){setError(reason instanceof Error?reason.message:String(reason))}finally{setBusy(false)}}
  async function removeResume(){if(busy)return;setBusy(true);setError(null);try{await api.reportStopped(current.Id,0);updateUserData({PlaybackPositionTicks:0,Played:false,IsFavorite:current.UserData?.IsFavorite??false});onRemove?.(current.Id);setMenuOpen(false)}catch(reason){setError(reason instanceof Error?reason.message:String(reason))}finally{setBusy(false)}}
  async function toggleFavorite(){if(!user||busy)return;setBusy(true);setError(null);setNotice(null);try{const data=await api.setFavorite(user.Id,current.Id,!current.UserData?.IsFavorite);updateUserData(data);announceFavoritesChanged();setNotice(data.IsFavorite?"Added to favorites.":"Removed from favorites.")}catch(reason){setError(reason instanceof Error?reason.message:String(reason))}finally{setBusy(false)}}
  async function showPlaylists(){setPlaylistOpen(open=>!open);setError(null);setNotice(null);if(playlists!==null)return;setBusy(true);try{const data=await api.playlists();setPlaylists(data.Items)}catch(reason){setError(reason instanceof Error?reason.message:String(reason));setPlaylists([])}finally{setBusy(false)}}
  async function addToPlaylist(playlist:Playlist){if(busy)return;setBusy(true);setError(null);setNotice(null);try{await api.appendPlaylistItems(playlist.Id,[current.Id]);setNotice(`Added to ${playlist.Name}.`);setPlaylistOpen(false)}catch(reason){setError(reason instanceof Error?reason.message:String(reason))}finally{setBusy(false)}}
  return <div className="group relative min-w-0">
    <Link to={`/items/${current.Id}`} className={episode?"block overflow-hidden rounded-xl border border-edge bg-surface-raised transition hover:border-brand/50 hover:bg-surface-hover hover:shadow-lg hover:shadow-brand/10":"block"}>
      <div className={`relative overflow-hidden bg-surface-raised ${episode?"aspect-video":"aspect-[2/3] rounded-xl border border-edge shadow-sm transition group-hover:border-brand/50 group-hover:shadow-lg group-hover:shadow-brand/10"}`}>
        <ItemImage itemId={current.Id} name={current.Name} className="h-full w-full transition duration-300 group-hover:scale-[1.035]"/>{episode&&<div className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/70 to-transparent"/>}{episode&&<span className="absolute bottom-2 left-2 rounded-md bg-black/70 px-2 py-1 text-[11px] font-semibold text-white">{episodeLabel}</span>}{duration&&<span className="absolute bottom-1.5 right-1.5 rounded-md bg-black/75 px-1.5 py-0.5 text-[11px] font-medium text-white">{duration}</span>}{resume&&<span className="absolute left-1.5 top-1.5 rounded-md bg-brand px-1.5 py-0.5 text-[11px] font-semibold text-white">Resume</span>}{current.UserData?.Played&&<span className="absolute left-1.5 top-1.5 grid h-8 w-8 place-items-center rounded-full bg-brand text-white shadow" aria-label="Played"><Icon path={ICONS.check} className="h-5 w-5"/></span>}
      </div>
      <div className={episode?"p-3.5":""}><p className={`${episode?"":"mt-2 text-sm"} truncate font-medium text-ink`} title={current.Name}>{current.Name}</p><p className={episode?"mt-1 line-clamp-2 min-h-10 text-xs leading-5 text-ink-muted":"text-xs text-ink-muted"}>{episode?current.Overview||`${episodeLabel}${current.Year?` · ${current.Year}`:""}`:current.IsFolder?`${current.ChildCount??0} ${current.Type==="Series"?"seasons":"episodes"}`:[current.Type,current.Year].filter(Boolean).join(" · ")}</p></div>
    </Link>
    <div ref={menu} className="absolute right-1.5 top-1.5 z-20">
      <button type="button" aria-label={`Actions for ${current.Name}`} aria-expanded={menuOpen} onClick={()=>setMenuOpen(open=>!open)} className="grid h-9 w-9 place-items-center rounded-full bg-black/75 text-white shadow backdrop-blur transition hover:bg-brand sm:opacity-0 sm:group-hover:opacity-100 sm:focus:opacity-100"><Icon path={ICONS.more}/></button>
      {menuOpen&&<div className="absolute left-0 top-11 w-56 overflow-hidden rounded-xl border border-edge bg-surface-raised p-1.5 text-sm text-ink shadow-2xl">
        {playlistOpen?<>
          <button type="button" onClick={()=>setPlaylistOpen(false)} className="flex w-full items-center gap-2 rounded-lg px-3 py-2.5 text-left font-medium hover:bg-surface-hover"><Icon path={ICONS.chevronLeft} className="h-4 w-4"/>Choose a playlist</button>
          <div className="max-h-40 overflow-y-auto border-t border-edge py-1">
            {playlists?.map(playlist=><button key={playlist.Id} type="button" disabled={busy} onClick={()=>addToPlaylist(playlist)} className="w-full truncate rounded-lg px-3 py-2 text-left text-ink-muted hover:bg-surface-hover hover:text-ink disabled:opacity-50">{playlist.Name}</button>)}
            {playlists?.length===0&&<p className="px-3 py-2 text-xs text-ink-muted">No playlists available.</p>}
            {playlists===null&&<p className="px-3 py-2 text-xs text-ink-muted">Loading playlists…</p>}
          </div>
        </>:<>
        <button type="button" disabled={busy} onClick={togglePlayed} className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-surface-hover disabled:opacity-50"><Icon path={ICONS.check} className={current.UserData?.Played?"h-5 w-5 text-brand-strong":"h-5 w-5"}/>{current.UserData?.Played?"Mark as unplayed":"Mark as played"}</button>
        <button type="button" disabled={busy} onClick={toggleFavorite} className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-surface-hover disabled:opacity-50"><Icon path={ICONS.star} className={current.UserData?.IsFavorite?"h-5 w-5 text-amber-400":"h-5 w-5"}/>{current.UserData?.IsFavorite?"Remove from favorites":"Add to favorites"}</button>
        {resume&&<button type="button" disabled={busy} onClick={removeResume} className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-surface-hover disabled:opacity-50"><Icon path={ICONS.restart}/>Remove from Continue Watching</button>}
        <button type="button" disabled={busy} onClick={showPlaylists} className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-surface-hover disabled:opacity-50"><Icon path={ICONS.list}/>Add to playlist<Icon path={playlistOpen?ICONS.chevronLeft:ICONS.chevronRight} className="ml-auto h-4 w-4"/></button>
        <button type="button" onClick={()=>{setMenuOpen(false);setDetailsOpen(true)}} className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-surface-hover"><Icon path={ICONS.info}/>Quick details</button>
        </>}
        {error&&<p className="px-3 py-2 text-xs text-danger" role="alert">{error}</p>}
        {notice&&<p className="px-3 py-2 text-xs text-brand-strong" role="status">{notice}</p>}
      </div>}
    </div>
    {detailsOpen&&<DetailModal item={current} onClose={()=>setDetailsOpen(false)}/>}
  </div>;
}
