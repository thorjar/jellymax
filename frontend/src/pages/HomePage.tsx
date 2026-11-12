import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import type { Item, Library } from "../api/types";
import { ItemCard } from "../components/ItemCard";
import { Icon, ICONS } from "../components/icons";
import { Spinner } from "../components/Spinner";
import { readCatalogCache, writeCatalogCache } from "../lib/catalogCache";
import { useAuth } from "../auth/AuthContext";
import { FAVORITES_CHANGED_EVENT } from "../lib/favoriteEvents";

interface RowData { library: Library; items: Item[]; }
interface HomeData { resume: Item[]; nextUp: Item[]; favorites: Item[]; recommended: Item[]; recent: Item[]; rows: RowData[]; }
interface HomeLibraryLayout { order: string[]; hidden: string[]; }

function layoutKey(userId: string) { return `jellymax:home-libraries:${userId}`; }
function readLayout(userId: string): HomeLibraryLayout {
  try {
    const value=localStorage.getItem(layoutKey(userId));
    if(value){const parsed=JSON.parse(value) as HomeLibraryLayout;return {order:Array.isArray(parsed.order)?parsed.order:[],hidden:Array.isArray(parsed.hidden)?parsed.hidden:[]};}
  } catch { /* Ignore corrupt browser preferences. */ }
  return {order:[],hidden:[]};
}
function arrangeRows(rows: RowData[], layout: HomeLibraryLayout): RowData[] {
  const byId=new Map(rows.map(row=>[row.library.ItemId,row]));
  const arranged=layout.order.flatMap(id=>{const row=byId.get(id);if(row)byId.delete(id);return row?[row]:[]});
  return [...arranged,...[...byId.values()].filter(row=>!layout.hidden.includes(row.library.ItemId))];
}

function MediaRow({ title, items, href, onRemove, landscape = false }: { title: string; items: Item[]; href?: string; onRemove?: (itemId: string) => void; landscape?: boolean }) {
  const rail = useRef<HTMLDivElement>(null);
  const move = (direction: number) => rail.current?.scrollBy({ left: direction * rail.current.clientWidth * 0.82, behavior: "smooth" });
  if (items.length === 0) return null;
  return <section className="group/row relative">
    <div className="mb-3 flex items-end justify-between gap-4"><h2 className="text-xl font-semibold tracking-tight">{title}</h2>{href&&<Link to={href} className="text-sm font-medium text-brand-strong hover:underline">View all <span aria-hidden="true">→</span></Link>}</div>
    <button type="button" aria-label={`Scroll ${title} left`} onClick={()=>move(-1)} className="absolute -left-3 top-1/2 z-10 hidden h-12 w-10 -translate-y-1/2 place-items-center rounded-full border border-edge bg-surface/90 text-white shadow-xl backdrop-blur transition hover:bg-brand sm:grid sm:opacity-0 sm:group-hover/row:opacity-100"><Icon path={ICONS.chevronLeft}/></button>
    <div ref={rail} className="flex snap-x snap-mandatory gap-4 overflow-x-auto pb-3 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      {items.map(item=><div key={item.Id} className={landscape?"w-[72vw] max-w-80 shrink-0 snap-start sm:w-72":"w-[42vw] max-w-44 shrink-0 snap-start sm:w-40 lg:w-44"}><ItemCard item={item} layout={landscape?"landscape":"poster"} onRemove={onRemove}/></div>)}
      {href&&<Link to={href} className="group grid w-[42vw] max-w-44 shrink-0 snap-start place-items-center rounded-xl border border-edge bg-surface-raised transition hover:border-brand/60 hover:bg-surface-hover sm:w-40 lg:w-44"><span className="grid gap-3 text-center text-sm font-semibold text-ink-muted group-hover:text-ink"><span className="mx-auto grid h-12 w-12 place-items-center rounded-full bg-brand/15 text-brand-strong"><Icon path={ICONS.chevronRight}/></span>See everything</span></Link>}
    </div>
    <button type="button" aria-label={`Scroll ${title} right`} onClick={()=>move(1)} className="absolute -right-3 top-1/2 z-10 hidden h-12 w-10 -translate-y-1/2 place-items-center rounded-full border border-edge bg-surface/90 text-white shadow-xl backdrop-blur transition hover:bg-brand sm:grid sm:opacity-0 sm:group-hover/row:opacity-100"><Icon path={ICONS.chevronRight}/></button>
  </section>;
}

export function HomePage() {
  const { user } = useAuth();
  const cached = readCatalogCache<HomeData>("home");
  const [resume, setResume] = useState<Item[]>(cached?.resume ?? []);
  const [nextUp, setNextUp] = useState<Item[]>(cached?.nextUp ?? []);
  const [favorites, setFavorites] = useState<Item[]>(cached?.favorites ?? []);
  const [recommended, setRecommended] = useState<Item[]>(cached?.recommended ?? []);
  const [recent, setRecent] = useState<Item[]>(cached?.recent ?? []);
  const [rows, setRows] = useState<RowData[]>(cached?.rows ?? []);
  const [layout, setLayout] = useState<HomeLibraryLayout>(()=>user?readLayout(user.Id):{order:[],hidden:[]});
  const [customizing, setCustomizing] = useState(false);
  const [loading, setLoading] = useState(!cached);
  const [error, setError] = useState<string | null>(null);
  useEffect(()=>{if(!user)return;let cancelled=false;(async()=>{try{
    // Recommendations enhance the home page but must not make the entire
    // catalog unavailable while an older backend is being restarted/upgraded.
    const [libraries, resumeResult, nextUpResult, favoriteResult, recommendationGroups]=await Promise.all([api.libraries(),api.resume(user.Id,10),api.nextUp(user.Id,10),api.getItems({IsFavorite:true,Limit:10}),api.recommendations(user.Id,10).catch(()=>[])]);
    const [latest,...libraryResults]=await Promise.all([api.getItems({Recursive:true,IncludeItemTypes:"Movie,Series,Audio,Video",SortBy:"DateCreated",Limit:10}),...libraries.map(library=>api.getItems({ParentId:library.ItemId,Limit:10}))]);
    if(cancelled)return;
    const nextRows=libraries.map((library,index)=>({library,items:libraryResults[index].Items}));
    const nextRecommended=recommendationGroups.flatMap(group=>group.Items).slice(0,10);
    const nextData={resume:resumeResult.Items,nextUp:nextUpResult.Items,favorites:favoriteResult.Items,recommended:nextRecommended,recent:latest.Items,rows:nextRows};
    setResume(nextData.resume);setNextUp(nextData.nextUp);setFavorites(nextData.favorites);setRecommended(nextData.recommended);setRecent(nextData.recent);setRows(nextRows);writeCatalogCache("home",nextData);setError(null);
  }catch(e){if(!cancelled)setError(e instanceof Error?e.message:String(e));}finally{if(!cancelled)setLoading(false);}})();return()=>{cancelled=true};},[user]);
  useEffect(()=>{if(!user)return;const refresh=()=>{void api.getItems({IsFavorite:true,Limit:10}).then(data=>setFavorites(data.Items)).catch(()=>{})};window.addEventListener(FAVORITES_CHANGED_EVENT,refresh);return()=>window.removeEventListener(FAVORITES_CHANGED_EVENT,refresh)},[user]);
  useEffect(()=>{if(user)setLayout(readLayout(user.Id))},[user]);
  const saveLayout=(next:HomeLibraryLayout)=>{if(!user)return;setLayout(next);localStorage.setItem(layoutKey(user.Id),JSON.stringify(next));};
  const visibleRows=arrangeRows(rows,layout);
  const visibleIds=visibleRows.map(row=>row.library.ItemId);
  const moveLibrary=(id:string,direction:number)=>{const index=visibleIds.indexOf(id);const target=index+direction;if(index<0||target<0||target>=visibleIds.length)return;const order=[...visibleIds];[order[index],order[target]]=[order[target],order[index]];saveLayout({order,hidden:layout.hidden.filter(value=>value!==id)});};
  const hideLibrary=(id:string)=>saveLayout({order:visibleIds.filter(value=>value!==id),hidden:[...new Set([...layout.hidden,id])]});
  const showLibrary=(id:string)=>saveLayout({order:[...visibleIds,id],hidden:layout.hidden.filter(value=>value!==id)});
  if(loading)return <Spinner label="Building your home screen…"/>;
  if(error&&rows.length===0)return <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm text-danger">Could not load home: {error}</div>;
  return <div className="space-y-10 pb-8"><header className="flex items-end justify-between gap-4"><div><p className="mb-1 text-sm font-semibold uppercase tracking-[0.18em] text-brand-strong">Browse</p><h1 className="text-3xl font-semibold tracking-tight sm:text-4xl">What will you watch?</h1></div>{rows.length>0&&<button type="button" onClick={()=>setCustomizing(true)} className="grid h-11 w-11 place-items-center rounded-full border border-edge bg-surface-raised text-ink transition hover:border-brand/60 hover:bg-surface-hover" aria-label="Reorder home carousels" title="Reorder"><Icon path={ICONS.sliders}/></button>}</header><MediaRow title="Continue watching" items={resume} href="/resume" landscape onRemove={itemId=>setResume(items=>items.filter(item=>item.Id!==itemId))}/><MediaRow title="Next Up" items={nextUp} landscape/><MediaRow title="Favorites" items={favorites} href="/favorites"/><MediaRow title="Recommended for you" items={recommended}/><MediaRow title="Recently added" items={recent}/>{visibleRows.map(({library,items})=><MediaRow key={library.ItemId} title={library.IsRemote&&library.RemoteServerName?`${library.Name} · ${library.RemoteServerName}`:library.Name} items={items} href={`/library/${library.ItemId}`}/>)}{rows.length===0&&<div className="rounded-xl border border-dashed border-edge py-16 text-center text-sm text-ink-muted">No libraries yet. Add one from Administration.</div>}{customizing&&<div className="fixed inset-0 z-50 grid place-items-center bg-black/70 p-4" role="dialog" aria-modal="true" aria-labelledby="home-layout-title" onMouseDown={event=>{if(event.target===event.currentTarget)setCustomizing(false)}}><div className="max-h-[85vh] w-full max-w-xl overflow-y-auto rounded-2xl border border-edge bg-surface p-6 shadow-2xl"><div className="mb-5 flex items-center justify-between"><div><h2 id="home-layout-title" className="text-xl font-semibold">Reorder</h2><p className="mt-1 text-sm text-ink-muted">Choose and reorder library carousels.</p></div><button type="button" onClick={()=>setCustomizing(false)} className="rounded-lg px-3 py-2 text-sm font-semibold hover:bg-surface-hover">Done</button></div><div className="space-y-2">{visibleRows.map((row,index)=><div key={row.library.ItemId} className="flex items-center gap-3 rounded-xl border border-edge bg-surface-raised p-3"><span className="min-w-0 flex-1 truncate font-medium">{row.library.Name}</span><button type="button" disabled={index===0} onClick={()=>moveLibrary(row.library.ItemId,-1)} className="rounded-md px-3 py-2 hover:bg-surface-hover disabled:opacity-30" aria-label={`Move ${row.library.Name} up`}>↑</button><button type="button" disabled={index===visibleRows.length-1} onClick={()=>moveLibrary(row.library.ItemId,1)} className="rounded-md px-3 py-2 hover:bg-surface-hover disabled:opacity-30" aria-label={`Move ${row.library.Name} down`}>↓</button><button type="button" onClick={()=>hideLibrary(row.library.ItemId)} className="rounded-md px-3 py-2 text-danger hover:bg-danger/10">Remove</button></div>)}</div>{rows.some(row=>!visibleIds.includes(row.library.ItemId))&&<div className="mt-6"><h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-ink-muted">Available libraries</h3><div className="space-y-2">{rows.filter(row=>!visibleIds.includes(row.library.ItemId)).map(row=><div key={row.library.ItemId} className="flex items-center gap-3 rounded-xl border border-edge p-3"><span className="min-w-0 flex-1 truncate">{row.library.Name}</span><button type="button" onClick={()=>showLibrary(row.library.ItemId)} className="rounded-md bg-brand px-3 py-2 text-sm font-semibold text-white">Add</button></div>)}</div></div>}</div></div>}</div>;
}
