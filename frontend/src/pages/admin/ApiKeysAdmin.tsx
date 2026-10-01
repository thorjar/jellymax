import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { ProviderStatus } from "../../api/types";
import { Spinner } from "../../components/Spinner";

type Provider = "tmdb" | "introdb" | "opensubtitles";

const PROVIDERS: Array<{
  id: Provider;
  name: string;
  description: string;
  link: string;
  linkLabel: string;
  optional?: boolean;
}> = [
  {
    id: "tmdb",
    name: "TMDB",
    description: "Movie and series names, descriptions, artwork, ratings, and genres.",
    link: "https://www.themoviedb.org/settings/api",
    linkLabel: "Get a TMDB API key",
  },
  {
    id: "introdb",
    name: "TheIntroDB",
    description: "Community intro timestamps used by the Skip Intro button. Public lookups work without a key; a personal key also includes your pending submissions.",
    link: "https://theintrodb.org/",
    linkLabel: "Open TheIntroDB",
    optional: true,
  },
  {
    id: "opensubtitles",
    name: "OpenSubtitles",
    description: "Search and download subtitles from the player.",
    link: "https://www.opensubtitles.com/consumers",
    linkLabel: "Get an OpenSubtitles API key",
  },
];

function configured(status: ProviderStatus, provider: Provider) {
  if (provider === "tmdb") return status.TmdbConfigured;
  if (provider === "introdb") return status.IntroDbConfigured;
  return status.OpenSubtitlesConfigured;
}

export function ApiKeysAdmin() {
  const [status, setStatus] = useState<ProviderStatus | null>(null);
  const [values, setValues] = useState<Record<Provider, string>>({ tmdb: "", introdb: "", opensubtitles: "" });
  const [busy, setBusy] = useState<Provider | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => { void api.providerSettings().then(setStatus).catch((reason) => setError(reason instanceof Error ? reason.message : String(reason))); }, []);

  async function save(provider: Provider, remove = false) {
    setBusy(provider); setError(null); setMessage(null);
    try {
      const value = remove ? "" : values[provider].trim();
      if (!remove && !value) throw new Error("Paste an API key first.");
      const update = provider === "tmdb" ? { TmdbApiKey: value }
        : provider === "introdb" ? { IntroDbApiKey: value }
        : { OpenSubtitlesApiKey: value };
      setStatus(await api.updateProviderSettings(update));
      setValues((current) => ({ ...current, [provider]: "" }));
      setMessage(remove ? `${PROVIDERS.find((entry) => entry.id === provider)?.name} key removed.` : `${PROVIDERS.find((entry) => entry.id === provider)?.name} key saved.`);
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Could not save the API key."); }
    finally { setBusy(null); }
  }

  if (!status && !error) return <Spinner label="Loading API providers…" />;
  return <div className="space-y-5">
    <div>
      <h2 className="text-xl font-semibold text-ink">API keys</h2>
      <p className="muted mt-1 text-sm">Add service keys here. Changes take effect immediately and remain after upgrades or restarts.</p>
    </div>
    {message && <div className="rounded-lg border border-brand/30 bg-brand/10 px-4 py-3 text-sm text-brand-strong" role="status">{message}</div>}
    {error && <div className="rounded-lg border border-danger/40 bg-danger/10 px-4 py-3 text-sm text-danger" role="alert">{error}</div>}
    {status && PROVIDERS.map((provider) => {
      const active = configured(status, provider.id);
      return <section key={provider.id} className="rounded-xl border border-edge bg-surface-raised p-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div><h3 className="font-semibold text-ink">{provider.name}</h3><p className="muted mt-1 max-w-2xl text-sm">{provider.description}</p></div>
          <span className={`rounded-full px-2.5 py-1 text-xs font-semibold ${active ? "bg-emerald-500/15 text-emerald-400" : provider.optional ? "bg-brand/15 text-brand-strong" : "bg-white/10 text-ink-muted"}`}>
            {active ? "Configured" : provider.optional ? "Public access active" : "Not configured"}
          </span>
        </div>
        <label className="mt-4 block text-sm font-medium text-ink" htmlFor={`${provider.id}-key`}>API key</label>
        <div className="mt-2 flex flex-col gap-2 sm:flex-row">
          <input id={`${provider.id}-key`} type="password" autoComplete="off" value={values[provider.id]}
            onChange={(event) => setValues((current) => ({ ...current, [provider.id]: event.target.value }))}
            placeholder={active ? "Enter a replacement key" : provider.optional ? "Optional personal key" : "Paste API key"}
            className="min-w-0 flex-1 rounded-lg border border-edge bg-surface px-3 py-2 text-sm text-ink outline-none focus:border-brand" />
          <button type="button" disabled={busy !== null} onClick={() => void save(provider.id)} className="rounded-lg bg-brand px-4 py-2 text-sm font-semibold text-white disabled:opacity-50">{busy === provider.id ? "Saving…" : "Save key"}</button>
          {active && <button type="button" disabled={busy !== null} onClick={() => void save(provider.id, true)} className="rounded-lg border border-edge px-4 py-2 text-sm text-ink-muted hover:bg-surface-hover disabled:opacity-50">Remove</button>}
        </div>
        <a href={provider.link} target="_blank" rel="noreferrer" className="mt-3 inline-block text-sm text-brand-strong hover:underline">{provider.linkLabel} ↗</a>
      </section>;
    })}
  </div>;
}
