import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api } from "../../api/client";
import type { RemoteServer } from "../../api/types";
import { Spinner } from "../../components/Spinner";
import { librariesChanged } from "../../lib/libraryEvents";

export function RemoteServersAdmin() {
  const [servers, setServers] = useState<RemoteServer[] | null>(null);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const load = useCallback(async () => { try { setServers(await api.remoteServers()); } catch (e) { setMessage(e instanceof Error ? e.message : String(e)); } }, []);
  useEffect(() => { void load(); }, [load]);
  async function connect(event: FormEvent) {
    event.preventDefault(); setBusy(true); setMessage("Connecting and importing the remote catalog…");
    try { await api.connectRemote(name.trim(), url.trim(), username, password); setName(""); setUrl(""); setUsername(""); setPassword(""); await load(); librariesChanged(); setMessage("Remote server connected. Its libraries are now available in the sidebar."); }
    catch (e) { setMessage(e instanceof Error ? e.message : String(e)); } finally { setBusy(false); }
  }
  async function sync(server: RemoteServer) { setBusy(true); try { const result=await api.syncRemote(server.Id); await load(); librariesChanged(); setMessage(`Imported ${result.Items} items from ${server.Name}.`); } catch(e){setMessage(e instanceof Error?e.message:String(e));} finally{setBusy(false);} }
  async function remove(server: RemoteServer) { if(!window.confirm(`Disconnect “${server.Name}” and remove its imported catalog?`))return; setBusy(true); try{await api.removeRemote(server.Id);await load();librariesChanged();setMessage("Remote server disconnected.");}catch(e){setMessage(e instanceof Error?e.message:String(e));}finally{setBusy(false);} }
  return <div className="card p-6">
    <h2 className="text-lg font-semibold">Connect a Jellyfin server</h2>
    <p className="muted mt-1 text-sm">Sign in once to import its libraries. The access token stays on this server and media is streamed through it.</p>
    <form className="mt-4 grid gap-4 sm:grid-cols-2" onSubmit={connect}>
      <div><label className="label" htmlFor="remote-name">Display name</label><input id="remote-name" className="input" value={name} onChange={e=>setName(e.target.value)} placeholder="Home server" maxLength={128} required /></div>
      <div><label className="label" htmlFor="remote-url">Server URL</label><input id="remote-url" className="input" type="url" value={url} onChange={e=>setUrl(e.target.value)} placeholder="https://media.example.com" required /></div>
      <div><label className="label" htmlFor="remote-user">Jellyfin username</label><input id="remote-user" className="input" autoComplete="username" value={username} onChange={e=>setUsername(e.target.value)} required /></div>
      <div><label className="label" htmlFor="remote-password">Jellyfin password</label><input id="remote-password" className="input" type="password" autoComplete="current-password" value={password} onChange={e=>setPassword(e.target.value)} placeholder="Leave blank if this user has no password" /></div>
      <div className="sm:col-span-2"><button className="btn btn-primary" disabled={busy}>{busy ? "Working…" : "Connect and import"}</button></div>
    </form>
    {message&&<p className="mt-4 rounded-lg border border-edge bg-surface-hover px-3 py-2 text-sm">{message}</p>}
    <h2 className="mt-8 text-lg font-semibold">Connected servers</h2>
    {!servers?<div className="mt-4"><Spinner label="Loading remote servers…" /></div>:servers.length===0?<p className="muted mt-4 rounded-xl border border-dashed border-edge py-10 text-center text-sm">No remote servers connected.</p>:
      <ul className="mt-4 divide-y divide-edge overflow-hidden rounded-xl border border-edge bg-surface-raised">{servers.map(server=><li key={server.Id} className="flex flex-wrap items-center justify-between gap-3 px-4 py-3"><div className="min-w-0"><div className="font-medium">{server.Name}</div><div className="muted truncate text-sm">{server.Url}{server.LastSync?` · Synced ${new Date(server.LastSync*1000).toLocaleString()}`:""}</div>{server.LastError&&<div className="mt-1 text-sm text-danger">{server.LastError}</div>}</div><div className="flex gap-2"><button className="btn" disabled={busy} onClick={()=>void sync(server)}>Sync</button><button className="btn btn-danger" disabled={busy} onClick={()=>void remove(server)}>Disconnect</button></div></li>)}</ul>}
  </div>;
}
