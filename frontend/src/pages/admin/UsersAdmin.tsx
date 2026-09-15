import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api } from "../../api/client";
import type { User } from "../../api/types";
import { Spinner } from "../../components/Spinner";

export function UsersAdmin() {
  const [users, setUsers] = useState<User[] | null>(null);
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [isAdministrator, setIsAdministrator] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setUsers(await api.users());
      setMessage(null);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function create(event: FormEvent) {
    event.preventDefault();
    setMessage(null);
    try {
      await api.createUser(name.trim(), password, isAdministrator);
      setName("");
      setPassword("");
      setIsAdministrator(false);
      await load();
      setMessage("User created.");
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="card p-6">
      <h2 className="text-lg font-semibold">Create a user</h2>
      <form className="mt-4 grid gap-4 sm:grid-cols-2" onSubmit={create}>
        <div>
          <label className="label" htmlFor="user-name">Username</label>
          <input id="user-name" className="input" value={name}
            onChange={(e) => setName(e.target.value)} maxLength={128} required />
        </div>
        <div>
          <label className="label" htmlFor="user-password">Password (12–1024 characters)</label>
          <input id="user-password" className="input" type="password" value={password}
            onChange={(e) => setPassword(e.target.value)} minLength={12} required />
        </div>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" className="h-4 w-4 accent-[var(--brand)]"
            checked={isAdministrator}
            onChange={(e) => setIsAdministrator(e.target.checked)} />
          Administrator
        </label>
        <div className="sm:col-span-2">
          <button type="submit" className="btn btn-primary">Create user</button>
        </div>
      </form>

      <h2 className="mt-8 text-lg font-semibold">Users</h2>
      {message && (
        <p className="mt-3 rounded-lg border border-edge bg-surface-hover px-3 py-2 text-sm">
          {message}
        </p>
      )}
      {loading ? (
        <div className="mt-4">
          <Spinner label="Loading users…" />
        </div>
      ) : users ? (
        <ul className="mt-4 divide-y divide-edge overflow-hidden rounded-xl border
          border-edge bg-surface-raised">
          {users.map((user) => (
            <li key={user.Id} className="flex items-center justify-between px-4 py-3">
              <span className="font-medium">{user.Name}</span>
              {user.Policy.IsAdministrator && (
                <span className="rounded bg-brand/15 px-2 py-0.5 text-xs font-semibold
                  text-brand-strong">admin</span>
              )}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
