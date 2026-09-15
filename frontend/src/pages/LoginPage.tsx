import { useEffect, useState, type FormEvent } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { api } from "../api/client";
import { useAuth } from "../auth/AuthContext";
import { Spinner } from "../components/Spinner";
import { Icon, ICONS } from "../components/icons";

export function LoginPage() {
  const { user, login } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [setupRequired, setSetupRequired] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The logo <img> can fail to load (missing /jellymax-mark.svg, or a server
  // falling back to HTML with 200); fall back to an inline icon tile.
  const [markFailed, setMarkFailed] = useState(false);

  const from = (location.state as { from?: string } | null)?.from ?? "/";

  useEffect(() => {
    if (user) navigate(from, { replace: true });
  }, [user, from, navigate]);

  useEffect(() => {
    let cancelled = false;
    void api.systemInfo().then((info) => {
      if (!cancelled) {
        setSetupRequired(!info.StartupWizardCompleted);
        if (!info.StartupWizardCompleted) setUsername("admin");
      }
    }).catch((reason) => {
      if (!cancelled) setError(reason instanceof Error ? reason.message : "Could not reach the server.");
    });
    return () => { cancelled = true; };
  }, []);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (setupRequired) {
        if (password !== confirmPassword) throw new Error("Passwords do not match.");
        await api.setupAdmin(username, password);
        setSetupRequired(false);
      }
      await login(username, password);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      if (setupRequired) {
        void api.systemInfo().then((info) => {
          if (info.StartupWizardCompleted) setSetupRequired(false);
        }).catch(() => {});
      }
    } finally {
      setBusy(false);
    }
  }

  if (setupRequired === null && !error) return <div className="flex min-h-screen items-center justify-center"><Spinner label="Connecting to server…" /></div>;
  if (setupRequired === null) return <div className="flex min-h-screen items-center justify-center px-4">
    <div className="card w-full max-w-sm space-y-4 p-8 text-center">
      <p role="alert">{error}</p>
      <button type="button" className="btn btn-primary" onClick={() => window.location.reload()}>Retry connection</button>
    </div>
  </div>;

  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <form onSubmit={submit} className="card w-full max-w-sm p-8 shadow-2xl shadow-black/40">
        <div className="mb-6 flex flex-col items-center gap-2 text-center">
          {markFailed ? (
            <span className="grid h-12 w-12 place-items-center rounded-xl bg-brand text-white">
              <Icon path={ICONS.play} className="h-6 w-6" />
            </span>
          ) : (
            <img src="/jellymax-mark.svg" alt="" className="h-12 w-12 rounded-xl"
              onError={() => setMarkFailed(true)} />
          )}
          <h1 className="text-xl font-semibold tracking-tight">Jellymax</h1>
          <p className="muted text-sm">{setupRequired ? "Create the first administrator to finish setup." : "Sign in to your media server."}</p>
        </div>
        <div className="space-y-4">
          <div>
            <label className="label" htmlFor="login-username">Username</label>
            <input id="login-username" className="input" value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoComplete="username" autoFocus required />
          </div>
          <div>
            <label className="label" htmlFor="login-password">Password</label>
            <input id="login-password" className="input" type="password" value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete={setupRequired ? "new-password" : "current-password"} minLength={setupRequired ? 12 : undefined} required />
          </div>
          {setupRequired && <div>
            <label className="label" htmlFor="setup-confirm-password">Confirm password</label>
            <input id="setup-confirm-password" className="input" type="password" value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)} autoComplete="new-password" minLength={12} required />
            <p className="muted mt-1 text-xs">Use at least 12 characters. You can add media libraries after signing in.</p>
          </div>}
          {error && (
            <p className="rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm
              text-danger" role="alert">
              {error}
            </p>
          )}
          <button type="submit" className="btn btn-primary w-full" disabled={busy || setupRequired === null}>
            {busy ? (setupRequired ? "Creating administrator…" : "Signing in…") : (setupRequired ? "Create administrator" : "Sign in")}
          </button>
        </div>
      </form>
    </div>
  );
}
