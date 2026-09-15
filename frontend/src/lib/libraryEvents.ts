export const LIBRARIES_CHANGED_EVENT = "jellymax:libraries-changed";

export function librariesChanged(): void {
  window.dispatchEvent(new Event(LIBRARIES_CHANGED_EVENT));
}
