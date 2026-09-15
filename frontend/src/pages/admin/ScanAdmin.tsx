import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import type { ScanStatus } from "../../api/types";
import { Spinner } from "../../components/Spinner";

export function ScanAdmin() {
  const [status, setStatus] = useState<ScanStatus | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

  const poll = useCallback(async () => {
    try {
      const list = await api.scanStatus();
      setStatus(list[0] ?? null);
      setMessage(null);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void poll();
    timer.current = window.setInterval(() => {
      void poll();
    }, 2000);
    return () => {
      if (timer.current) window.clearInterval(timer.current);
    };
  }, [poll]);

  async function startScan() {
    setMessage(null);
    try {
      await api.refreshLibrary();
      setMessage("Scan scheduled. It runs asynchronously; watch the status below.");
      await poll();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    }
  }

  const running = status?.State === "Running";

  return (
    <div className="card p-6">
      <h2 className="text-lg font-semibold">Library scan</h2>
      <p className="muted mt-1 text-sm">
        One scan runs at a time. Only one session touches the catalog at once.
      </p>
      <button className="btn btn-primary mt-4" onClick={startScan} disabled={running}>
        {running ? "Scan in progress…" : "Start library scan"}
      </button>

      {message && (
        <p className="mt-4 rounded-lg border border-edge bg-surface-hover px-3 py-2 text-sm">
          {message}
        </p>
      )}
      {!status ? (
        <div className="mt-6">
          <Spinner label="Loading scan status…" />
        </div>
      ) : (
        <dl className="mt-6 grid grid-cols-2 gap-4 sm:grid-cols-3">
          <div className="rounded-lg border border-edge bg-surface-raised p-3">
            <dt className="text-xs uppercase tracking-wide text-ink-muted">State</dt>
            <dd className={`mt-1 font-medium ${running ? "text-brand-strong" : ""}`}>
              {status.State}
            </dd>
          </div>
          <div className="rounded-lg border border-edge bg-surface-raised p-3">
            <dt className="text-xs uppercase tracking-wide text-ink-muted">Files scanned</dt>
            <dd className="mt-1 font-medium">{status.Scanned}</dd>
          </div>
          <div className="rounded-lg border border-edge bg-surface-raised p-3">
            <dt className="text-xs uppercase tracking-wide text-ink-muted">Items removed</dt>
            <dd className="mt-1 font-medium">{status.Removed}</dd>
          </div>
          <div className="rounded-lg border border-edge bg-surface-raised p-3">
            <dt className="text-xs uppercase tracking-wide text-ink-muted">Probe failures</dt>
            <dd className="mt-1 font-medium">{status.ProbeFailures}</dd>
          </div>
          <div className="rounded-lg border border-edge bg-surface-raised p-3">
            <dt className="text-xs uppercase tracking-wide text-ink-muted">Metadata provider</dt>
            <dd className="mt-1 font-medium">
              {status.MetadataProvider}
              {status.MetadataProvider === "TMDb" && (
                <span className="ml-2 rounded bg-brand/15 px-1.5 py-0.5 text-xs
                  text-brand-strong">enriching</span>
              )}
            </dd>
          </div>
          <div className="rounded-lg border border-edge bg-surface-raised p-3">
            <dt className="text-xs uppercase tracking-wide text-ink-muted">Metadata failures</dt>
            <dd className={`mt-1 font-medium ${status.MetadataFailures > 0 ? "text-danger" : ""}`}>
              {status.MetadataFailures}
            </dd>
          </div>
          {status.Errors.length > 0 && (
            <div className="col-span-full rounded-lg border border-danger/40 bg-danger/10 p-3">
              <dt className="text-xs uppercase tracking-wide text-danger">Errors</dt>
              <dd className="mt-1">
                <ul className="list-disc space-y-1 pl-5 text-sm">
                  {status.Errors.map((error, index) => (
                    <li key={index}>{error}</li>
                  ))}
                </ul>
              </dd>
            </div>
          )}
        </dl>
      )}
      <p className="muted mt-6 text-sm">
        Note: scan status is kept in memory only, so it resets when the server restarts.
      </p>
    </div>
  );
}