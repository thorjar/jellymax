import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api } from "../../api/client";
import type { ObjectStoreConnection } from "../../api/types";
import { Spinner } from "../../components/Spinner";
import { librariesChanged } from "../../lib/libraryEvents";

const COLLECTION_TYPES = ["movies", "tvshows", "music", "homevideos"];

export function ObjectStoresAdmin() {
  const [stores, setStores] = useState<ObjectStoreConnection[] | null>(null);
  const [provider, setProvider] = useState<"r2" | "s3">("r2");
  const [name, setName] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [region, setRegion] = useState("auto");
  const [bucket, setBucket] = useState("");
  const [prefix, setPrefix] = useState("");
  const [accessKey, setAccessKey] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [collectionType, setCollectionType] = useState("movies");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const load = useCallback(async () => {
    try { setStores(await api.objectStores()); }
    catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
  }, []);
  useEffect(() => { void load(); }, [load]);

  function changeProvider(value: "r2" | "s3") {
    setProvider(value);
    setRegion(value === "r2" ? "auto" : "us-east-1");
    if (value === "s3") setEndpoint("");
  }

  async function connect(event: FormEvent) {
    event.preventDefault(); setBusy(true); setMessage("Connecting and scanning the bucket…");
    try {
      await api.connectObjectStore({
        Name: name.trim(), Endpoint: endpoint.trim() || null, Region: region.trim(),
        Bucket: bucket.trim(), Prefix: prefix.trim(), AccessKeyId: accessKey.trim(),
        SecretAccessKey: secretKey, CollectionType: collectionType,
      });
      setName(""); setBucket(""); setPrefix(""); setAccessKey(""); setSecretKey("");
      await load(); librariesChanged(); setMessage("Object storage connected and its media library is ready.");
    } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  }

  async function sync(store: ObjectStoreConnection) {
    setBusy(true);
    try { const result=await api.syncObjectStore(store.Id); librariesChanged(); setMessage(`Scanned ${result.Items} media objects from ${store.Name}.`); }
    catch(error){setMessage(error instanceof Error?error.message:String(error));}
    finally{setBusy(false);}
  }

  async function remove(store: ObjectStoreConnection) {
    if(!window.confirm(`Disconnect “${store.Name}”? Objects remain in the bucket.`)) return;
    setBusy(true);
    try { await api.removeObjectStore(store.Id); await load(); librariesChanged(); setMessage("Object storage disconnected."); }
    catch(error){setMessage(error instanceof Error?error.message:String(error));}
    finally{setBusy(false);}
  }

  return <div className="card p-6">
    <h2 className="text-lg font-semibold">Connect object storage</h2>
    <p className="muted mt-1 text-sm">Read media directly from a private AWS S3 or Cloudflare R2 bucket. Credentials stay on this Jellymax server.</p>
    <form className="mt-4 grid gap-4 sm:grid-cols-2" onSubmit={connect}>
      <div><label className="label" htmlFor="store-provider">Provider</label><select id="store-provider" className="input" value={provider} onChange={e=>changeProvider(e.target.value as "r2"|"s3")}><option value="r2">Cloudflare R2</option><option value="s3">AWS S3</option></select></div>
      <div><label className="label" htmlFor="store-name">Library name</label><input id="store-name" className="input" value={name} onChange={e=>setName(e.target.value)} placeholder="Cloud movies" required /></div>
      <div className="sm:col-span-2"><label className="label" htmlFor="store-endpoint">S3 endpoint {provider==="s3"&&"(leave blank for AWS)"}</label><input id="store-endpoint" className="input" type="url" value={endpoint} onChange={e=>setEndpoint(e.target.value)} placeholder={provider==="r2"?"https://ACCOUNT_ID.r2.cloudflarestorage.com":"Optional custom S3-compatible endpoint"} required={provider==="r2"} /></div>
      <div><label className="label" htmlFor="store-region">Region</label><input id="store-region" className="input" value={region} onChange={e=>setRegion(e.target.value)} placeholder={provider==="r2"?"auto":"us-east-1"} required /></div>
      <div><label className="label" htmlFor="store-bucket">Bucket</label><input id="store-bucket" className="input" value={bucket} onChange={e=>setBucket(e.target.value)} required /></div>
      <div><label className="label" htmlFor="store-prefix">Folder prefix (optional)</label><input id="store-prefix" className="input" value={prefix} onChange={e=>setPrefix(e.target.value)} placeholder="Movies/" /></div>
      <div><label className="label" htmlFor="store-type">Collection type</label><select id="store-type" className="input" value={collectionType} onChange={e=>setCollectionType(e.target.value)}>{COLLECTION_TYPES.map(type=><option key={type} value={type}>{type}</option>)}</select></div>
      <div><label className="label" htmlFor="store-access">Access key ID</label><input id="store-access" className="input" autoComplete="off" value={accessKey} onChange={e=>setAccessKey(e.target.value)} required /></div>
      <div><label className="label" htmlFor="store-secret">Secret access key</label><input id="store-secret" className="input" type="password" autoComplete="new-password" value={secretKey} onChange={e=>setSecretKey(e.target.value)} required /></div>
      <div className="sm:col-span-2"><button className="btn btn-primary" disabled={busy}>{busy?"Connecting…":"Connect and scan"}</button></div>
    </form>
    {message&&<p className="mt-4 rounded-lg border border-edge bg-surface-hover px-3 py-2 text-sm">{message}</p>}
    <h2 className="mt-8 text-lg font-semibold">Connected object stores</h2>
    {!stores?<div className="mt-4"><Spinner label="Loading object stores…" /></div>:stores.length===0?<p className="muted mt-4 rounded-xl border border-dashed border-edge py-10 text-center text-sm">No object storage connected.</p>:
      <ul className="mt-4 divide-y divide-edge overflow-hidden rounded-xl border border-edge bg-surface-raised">{stores.map(store=><li key={store.Id} className="flex flex-wrap items-center justify-between gap-3 px-4 py-3"><div className="min-w-0"><div className="font-medium">{store.Name}</div><div className="muted truncate text-sm">{store.Bucket}{store.Prefix?`/${store.Prefix}`:""} · {store.Region}</div></div><div className="flex gap-2"><button className="btn" disabled={busy} onClick={()=>void sync(store)}>Sync</button><button className="btn btn-danger" disabled={busy} onClick={()=>void remove(store)}>Disconnect</button></div></li>)}</ul>}
  </div>;
}
