import type { Library } from "../api/types";

export interface LibraryGroup {
  key: string;
  name: string;
  isLocal: boolean;
  libraries: Library[];
}

export function groupLibraries(libraries: Library[], localServerName: string): LibraryGroup[] {
  const groups = new Map<string, LibraryGroup>();
  for (const library of libraries) {
    const isLocal = !library.IsRemote;
    const name = isLocal ? localServerName : library.RemoteServerName || "Remote server";
    const key = `${isLocal ? "local" : "remote"}:${name}`;
    const group = groups.get(key) ?? { key, name, isLocal, libraries: [] };
    group.libraries.push(library);
    groups.set(key, group);
  }
  return [...groups.values()]
    .sort((a, b) => Number(b.isLocal) - Number(a.isLocal) || a.name.localeCompare(b.name))
    .map(group => ({ ...group, libraries: group.libraries.sort((a, b) => a.Name.localeCompare(b.Name)) }));
}
