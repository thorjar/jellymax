import type {
  DirectoryListing,
  Item,
  ItemList,
  Library,
  LoginResponse,
  PlaybackInfo,
  PlaylistList,
  ScanStatus,
  SystemInfo,
  RemoteServer,
  Recommendation,
  User,
  UserData,
  SubtitleSearchResult,
  ObjectStoreConnection,
  NewObjectStore,
  ProviderStatus,
  MediaSegments,
} from "./types";

const TOKEN_KEY = "jellymax_token";
const LEGACY_TOKEN_KEY = "jellyfin_rust_token";

// Clean up the previous application-owned full-resolution poster cache.
// Posters now use resized server thumbnails and the browser's managed HTTP cache.
if ("caches" in window) void caches.delete("jellyfin-artwork-v1");

// Base URL for API calls. In development the Vite proxy forwards API prefixes
// to the backend, so this is empty. Set VITE_API_BASE to point directly at a
// backend (e.g. http://127.0.0.1:8097) when not using the proxy.
export const apiBase: string =
  (import.meta.env.VITE_API_BASE as string | undefined) ?? "";

export function getToken(): string | null {
  const oldToken = localStorage.getItem(LEGACY_TOKEN_KEY);
  if (oldToken && !localStorage.getItem(TOKEN_KEY)) localStorage.setItem(TOKEN_KEY, oldToken);
  if (oldToken) localStorage.removeItem(LEGACY_TOKEN_KEY);
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string | null): void {
  if (localStorage.getItem(TOKEN_KEY) !== token) {
    for (let index = sessionStorage.length - 1; index >= 0; index -= 1) {
      const key = sessionStorage.key(index);
      if (key?.startsWith("catalog:")) sessionStorage.removeItem(key);
    }
    if ("caches" in window) void caches.delete("jellyfin-artwork-v1");
  }
  if (token) {
    localStorage.setItem(TOKEN_KEY, token);
  } else {
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(LEGACY_TOKEN_KEY);
  }
}

// Prefix a relative backend URL (such as a DirectStreamUrl) with the API base.
export function resolveUrl(path: string): string {
  if (path.startsWith("http://") || path.startsWith("https://")) return path;
  return `${apiBase}${path}`;
}

interface ApiError extends Error {
  status?: number;
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers = new Headers(options.headers);
  if (token) headers.set("X-Emby-Token", token);
  if (options.body && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }
  const response = await fetch(resolveUrl(path), { ...options, headers });
  if (!response.ok) {
    let message = `Request failed (${response.status})`;
    try {
      const data = (await response.json()) as { Error?: string };
      if (data && typeof data.Error === "string") message = data.Error;
    } catch {
      // Non-JSON error body; keep the generic message.
    }
    const error: ApiError = new Error(message);
    error.status = response.status;
    throw error;
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

function uploadPartRequest(
  storeId: string,
  uploadId: string,
  index: number,
  part: Blob,
  onProgress: (loaded: number) => void,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open(
      "PUT",
      resolveUrl(`/ObjectStores/${encodeURIComponent(storeId)}/Uploads/${encodeURIComponent(uploadId)}/Part?Index=${index}`),
    );
    const token = getToken();
    if (token) xhr.setRequestHeader("X-Emby-Token", token);
    xhr.setRequestHeader("Content-Type", "application/octet-stream");
    xhr.upload.onprogress = (event) => onProgress(event.loaded);
    xhr.onerror = () => reject(new Error("Upload failed because the server could not be reached"));
    xhr.onabort = () => reject(new Error("Upload cancelled"));
    xhr.onload = () => {
      let body: { Error?: string } = {};
      try {
        body = JSON.parse(xhr.responseText) as typeof body;
      } catch {
        // Keep the HTTP status error when an intermediary returned a non-JSON response.
      }
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new Error(body.Error || `Upload failed (${xhr.status})`));
        return;
      }
      resolve();
    };
    xhr.send(part);
  });
}

async function uploadRequest(
  storeId: string,
  path: string,
  file: File,
  onProgress: (loaded: number, total: number) => void,
): Promise<{ Key: string; Size: number }> {
  const query = new URLSearchParams({ Path: path, ContentType: file.type || "application/octet-stream" });
  const started = await request<{ UploadId: string; PartSize: number }>(
    `/ObjectStores/${encodeURIComponent(storeId)}/Uploads?${query}`,
    { method: "POST" },
  );
  let offset = 0;
  let index = 0;
  try {
    while (offset < file.size) {
      const end = Math.min(offset + started.PartSize, file.size);
      await uploadPartRequest(storeId, started.UploadId, index, file.slice(offset, end), loaded =>
        onProgress(offset + loaded, file.size),
      );
      offset = end;
      index += 1;
      onProgress(offset, file.size);
    }
    return await request<{ Key: string; Size: number }>(
      `/ObjectStores/${encodeURIComponent(storeId)}/Uploads/${encodeURIComponent(started.UploadId)}`,
      { method: "POST" },
    );
  } catch (error) {
    try {
      await request<void>(
        `/ObjectStores/${encodeURIComponent(storeId)}/Uploads/${encodeURIComponent(started.UploadId)}`,
        { method: "DELETE" },
      );
    } catch {
      // The server also expires abandoned multipart sessions.
    }
    throw error;
  }
}

export interface ItemQuery {
  ParentId?: string;
  Recursive?: boolean;
  SearchTerm?: string;
  StartIndex?: number;
  Limit?: number;
  IncludeItemTypes?: string;
  IsFavorite?: boolean;
  IsPlayed?: boolean;
  SortBy?: "DateCreated";
}

export const api = {
  // Public
  health: () => request<{ status: string }>("/health"),
  systemInfo: () => request<SystemInfo>("/System/Info/Public"),
  setupAdmin: (name: string, password: string) => request<User>("/System/Setup", {
    method: "POST",
    body: JSON.stringify({ Name: name, Password: password }),
  }),

  // Authentication
  login: (username: string, password: string) =>
    request<LoginResponse>("/Users/AuthenticateByName", {
      method: "POST",
      body: JSON.stringify({ Username: username, Pw: password }),
    }),
  logout: () => request<void>("/Sessions/Logout", { method: "POST" }),
  me: () => request<User>("/Users/Me"),

  // Admin: users
  users: () => request<User[]>("/Users/"),
  createUser: (name: string, password: string, isAdministrator: boolean) =>
    request<User>("/Users/New", {
      method: "POST",
      body: JSON.stringify({
        Name: name,
        Password: password,
        IsAdministrator: isAdministrator,
      }),
    }),

  // Provider settings
  providerSettings: () => request<ProviderStatus>("/System/Providers"),
  updateProviderSettings: (settings: { TmdbApiKey?: string; IntroDbApiKey?: string; OpenSubtitlesApiKey?: string }) =>
    request<ProviderStatus>("/System/Providers", { method: "PUT", body: JSON.stringify(settings) }),

  // Libraries
  libraries: () => request<Library[]>("/Library/VirtualFolders"),
  createLibrary: (name: string, collectionType: string, location: string) =>
    request<{ Id: string }>("/Library/VirtualFolders", {
      method: "POST",
      body: JSON.stringify({
        Name: name,
        CollectionType: collectionType,
        Locations: [location],
      }),
    }),
  deleteLibrary: (id: string) =>
    request<void>(`/Library/VirtualFolders/${encodeURIComponent(id)}`, {
      method: "DELETE",
    }),
  // Admin-only server-side folder navigation for the media-folder picker.
  listDirectories: (path?: string) => {
    const query = new URLSearchParams();
    if (path) query.set("Path", path);
    const qs = query.toString();
    return request<DirectoryListing>(`/Library/Paths${qs ? `?${qs}` : ""}`);
  },
  refreshLibrary: () =>
    request<{ Id: string }>("/Library/Refresh", { method: "POST" }),
  scanStatus: () => request<ScanStatus[]>("/ScheduledTasks"),
  remoteServers: () => request<RemoteServer[]>("/RemoteServers"),
  connectRemote: (name: string, url: string, username: string, password: string) =>
    request<{ Id: string; ServerId: string }>("/RemoteServers", {
      method: "POST", body: JSON.stringify({ Name: name, Url: url, Username: username, Password: password }),
    }),
  syncRemote: (id: string) => request<{ Items: number }>(`/RemoteServers/${encodeURIComponent(id)}/Sync`, { method: "POST" }),
  removeRemote: (id: string) => request<void>(`/RemoteServers/${encodeURIComponent(id)}`, { method: "DELETE" }),
  objectStores: () => request<ObjectStoreConnection[]>("/ObjectStores"),
  connectObjectStore: (input: NewObjectStore) =>
    request<{ Id: string; LibraryId: string }>("/ObjectStores", {
      method: "POST", body: JSON.stringify(input),
    }),
  syncObjectStore: (id: string) =>
    request<{ Items: number }>(`/ObjectStores/${encodeURIComponent(id)}/Sync`, { method: "POST" }),
  uploadObject: (
    id: string,
    path: string,
    file: File,
    onProgress: (loaded: number, total: number) => void,
  ) => uploadRequest(id, path, file, onProgress),
  removeObjectStore: (id: string) =>
    request<void>(`/ObjectStores/${encodeURIComponent(id)}`, { method: "DELETE" }),
  // Admin-only: re-run the TMDb match for one movie. Matched=false means
  // TMDb had no plausible hit (the item keeps whatever metadata it had).
  refreshMetadata: (id: string, name?: string, year?: number) => {
    const body: Record<string, unknown> = {};
    if (name) body.Name = name;
    if (year) body.Year = year;
    return request<{ Matched: boolean; ItemId: string }>(
      `/Items/${encodeURIComponent(id)}/Metadata/Refresh`,
      { method: "POST", body: JSON.stringify(body) },
    );
  },

  // Items
  getItems: (params: ItemQuery = {}) => {
    const query = new URLSearchParams();
    for (const [key, value] of Object.entries(params)) {
      if (value !== undefined && value !== null && value !== "") {
        query.set(key, String(value));
      }
    }
    const qs = query.toString();
    return request<ItemList>(`/Items${qs ? `?${qs}` : ""}`);
  },
  adjacentEpisodes: (id: string) => request<{ PreviousId: string | null; NextId: string | null }>(`/Items/${encodeURIComponent(id)}/AdjacentEpisodes`),
  getItem: (id: string) => request<Item>(`/Items/${encodeURIComponent(id)}`),
  resume: (userId: string, limit?: number) => {
    const suffix = limit ? `?Limit=${limit}` : "";
    return request<ItemList>(`/Users/${encodeURIComponent(userId)}/Items/Resume${suffix}`);
  },
  recommendations: (userId: string, itemLimit = 10) =>
    request<Recommendation[]>(`/Recommendations?UserId=${encodeURIComponent(userId)}&ItemLimit=${itemLimit}`),

  // Playback
  mediaSegments: (id: string) => request<MediaSegments>(`/Items/${encodeURIComponent(id)}/Segments`),
  playbackInfo: (id: string, supportsHevc = false, supportsMkv = false, supportsAc3 = false, supportsEac3 = false, deviceProfile?: unknown, startTimeTicks = 0) =>
    request<PlaybackInfo>(
      `/Items/${encodeURIComponent(id)}/PlaybackInfo?SupportsHevc=${supportsHevc}&SupportsMkv=${supportsMkv}&SupportsAc3=${supportsAc3}&SupportsEac3=${supportsEac3}&StartTimeTicks=${Math.max(0, startTimeTicks)}`,
      { method: "POST", body: JSON.stringify({ DeviceProfile: deviceProfile }) }
    ),
  searchSubtitles: (id: string, language: string) =>
    request<{ Results: SubtitleSearchResult[] }>(`/Items/${encodeURIComponent(id)}/SubtitleSearch?Language=${encodeURIComponent(language)}`),
  downloadSubtitle: (id: string, fileId: number) =>
    request<{ Content: string; Format: string }>(`/Items/${encodeURIComponent(id)}/SubtitleDownload`,
      { method: "POST", body: JSON.stringify({ FileId: fileId }) }),
  reportProgress: (itemId: string, positionTicks: number, playSessionId?: string) =>
    request<void>("/Sessions/Playing/Progress", {
      method: "POST",
      body: JSON.stringify({ ItemId: itemId, PositionTicks: positionTicks, PlaySessionId: playSessionId }),
    }),
  reportStopped: (itemId: string, positionTicks: number, playSessionId?: string) =>
    request<void>("/Sessions/Playing/Stopped", {
      method: "POST",
      keepalive: true,
      body: JSON.stringify({ ItemId: itemId, PositionTicks: positionTicks, PlaySessionId: playSessionId }),
    }),
  setFavorite: (userId: string, itemId: string, favorite: boolean) =>
    request<UserData>(`/Users/${encodeURIComponent(userId)}/FavoriteItems/${encodeURIComponent(itemId)}`, {
      method: favorite ? "POST" : "DELETE",
    }),
  setPlayed: (userId: string, itemId: string, played: boolean) =>
    request<UserData>(`/Users/${encodeURIComponent(userId)}/PlayedItems/${encodeURIComponent(itemId)}`, {
      method: played ? "POST" : "DELETE",
    }),

  // Playlists
  playlists: () => request<PlaylistList>("/Playlists"),
  createPlaylist: (name: string, ids: string[]) =>
    request<{ Id: string }>("/Playlists", {
      method: "POST",
      body: JSON.stringify({ Name: name, Ids: ids }),
    }),
  playlistItems: (id: string, startIndex = 0, limit = 200) =>
    request<ItemList>(
      `/Playlists/${encodeURIComponent(id)}/Items?StartIndex=${startIndex}&Limit=${limit}`
    ),
  appendPlaylistItems: (id: string, ids: string[]) =>
    request<void>(`/Playlists/${encodeURIComponent(id)}/Items`, {
      method: "POST",
      body: JSON.stringify({ Ids: ids }),
    }),
  deletePlaylist: (id: string) =>
    request<void>(`/Playlists/${encodeURIComponent(id)}`, { method: "DELETE" }),
};
