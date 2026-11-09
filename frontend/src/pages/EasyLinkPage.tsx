import { FormEvent, useEffect, useState } from "react";
import { api } from "../api/client";
import type { User } from "../api/types";
import { useAuth } from "../auth/AuthContext";
import { ICONS, Icon } from "../components/icons";

export function EasyLinkPage() {
  const { user } = useAuth();
  const [code, setCode] = useState("");
  const [users, setUsers] = useState<User[]>([]);
  const [userId, setUserId] = useState(user?.Id ?? "");
  const [submitting, setSubmitting] = useState(false);
  const [success, setSuccess] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!user?.Policy?.IsAdministrator) return;
    void api.users().then(setUsers).catch(() => setUsers([]));
  }, [user?.Policy?.IsAdministrator]);

  async function authorize(event: FormEvent) {
    event.preventDefault();
    if (code.length !== 6) return;
    setSubmitting(true);
    setError("");
    try {
      await api.authorizeEasyLink(code, userId || undefined);
      setSuccess(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The EasyLink code could not be authorized");
    } finally {
      setSubmitting(false);
    }
  }

  function reset() {
    setCode("");
    setSuccess(false);
    setError("");
  }

  return (
    <section className="mx-auto max-w-xl py-4 sm:py-10">
      <div className="overflow-hidden rounded-2xl border border-edge bg-surface-raised shadow-xl">
        <div className="border-b border-edge bg-gradient-to-br from-brand/20 via-surface-raised to-surface-raised px-6 py-8 text-center sm:px-10">
          <span className="mx-auto mb-4 grid h-14 w-14 place-items-center rounded-2xl bg-brand text-white shadow-lg shadow-brand/20">
            <Icon path={ICONS.link} className="h-7 w-7" />
          </span>
          <h1 className="text-2xl font-semibold tracking-tight">EasyLink</h1>
          <p className="mx-auto mt-2 max-w-md text-sm leading-6 text-ink-muted">
            Sign in a TV or another Jellyfin-compatible app without entering your password on that device.
          </p>
        </div>

        {success ? (
          <div className="px-6 py-10 text-center sm:px-10">
            <span className="mx-auto mb-4 grid h-12 w-12 place-items-center rounded-full bg-emerald-500/15 text-emerald-400">
              <Icon path={ICONS.check} className="h-6 w-6" />
            </span>
            <h2 className="text-lg font-semibold">Device authorized</h2>
            <p className="mt-2 text-sm text-ink-muted">The device will finish signing in automatically.</p>
            <button type="button" className="button-secondary mt-6" onClick={reset}>Link another device</button>
          </div>
        ) : (
          <form onSubmit={authorize} className="space-y-5 px-6 py-7 sm:px-10">
            <div>
              <label className="label" htmlFor="easylink-code">Code shown on your device</label>
              <input
                id="easylink-code"
                className="input text-center font-mono text-2xl tracking-[0.35em]"
                value={code}
                onChange={(event) => setCode(event.target.value.replace(/\D/g, "").slice(0, 6))}
                inputMode="numeric"
                autoComplete="one-time-code"
                pattern="[0-9]{6}"
                placeholder="000000"
                autoFocus
                required
              />
            </div>

            {user?.Policy?.IsAdministrator && users.length > 1 && (
              <div>
                <label className="label" htmlFor="easylink-user">Sign in as</label>
                <select id="easylink-user" className="input" value={userId} onChange={(event) => setUserId(event.target.value)}>
                  {users.map((candidate) => <option key={candidate.Id} value={candidate.Id}>{candidate.Name}</option>)}
                </select>
              </div>
            )}

            {error && <p role="alert" className="rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>}
            <p className="text-xs leading-5 text-ink-muted">Only approve a code that is currently visible on a device you control. Codes expire after ten minutes.</p>
            <button type="submit" className="button-primary w-full" disabled={code.length !== 6 || submitting}>
              {submitting ? "Authorizing…" : "Authorize device"}
            </button>
          </form>
        )}
      </div>
    </section>
  );
}
