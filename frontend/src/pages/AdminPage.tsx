import { useState } from "react";
import { LibraryAdmin } from "./admin/LibraryAdmin";
import { ScanAdmin } from "./admin/ScanAdmin";
import { UsersAdmin } from "./admin/UsersAdmin";
import { RemoteServersAdmin } from "./admin/RemoteServersAdmin";
import { ObjectStoresAdmin } from "./admin/ObjectStoresAdmin";

type Tab = "libraries" | "remote" | "storage" | "users" | "scan";

const TABS: { id: Tab; label: string }[] = [
  { id: "libraries", label: "Libraries" },
  { id: "remote", label: "Remote servers" },
  { id: "storage", label: "Object storage" },
  { id: "users", label: "Users" },
  { id: "scan", label: "Library scan" },
];

export function AdminPage() {
  const [tab, setTab] = useState<Tab>("libraries");

  return (
    <section>
      <h1 className="page-title mb-6">Administration</h1>
      <div className="mb-6 flex gap-1 rounded-xl border border-edge bg-surface-raised p-1"
        role="tablist">
        {TABS.map(({ id, label }) => (
          <button key={id} role="tab" aria-selected={tab === id}
            onClick={() => setTab(id)}
            className={`flex-1 rounded-lg px-3 py-2 text-sm font-medium transition
              ${tab === id
                ? "bg-brand text-white shadow-sm"
                : "text-ink-muted hover:bg-surface-hover hover:text-ink"}`}>
            {label}
          </button>
        ))}
      </div>
      {tab === "libraries" && <LibraryAdmin />}
      {tab === "remote" && <RemoteServersAdmin />}
      {tab === "storage" && <ObjectStoresAdmin />}
      {tab === "users" && <UsersAdmin />}
      {tab === "scan" && <ScanAdmin />}
    </section>
  );
}
