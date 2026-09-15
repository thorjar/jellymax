export const FAVORITES_CHANGED_EVENT = "jellyfin:favorites-changed";

export function announceFavoritesChanged(): void {
  window.dispatchEvent(new Event(FAVORITES_CHANGED_EVENT));
}
