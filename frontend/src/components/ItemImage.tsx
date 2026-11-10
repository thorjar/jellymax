import { useEffect, useState } from "react";
import { getToken, resolveUrl } from "../api/client";

interface ItemImageProps {
  itemId: string;
  name: string;
  className?: string;
  revision?: string | number;
  imageType?: "Primary" | "Backdrop";
}

// The artwork endpoint requires the X-Emby-Token header, which an <img> tag
// cannot send, so fetch it as a blob and render an object URL.
export function ItemImage({ itemId, name, className, revision = 0, imageType = "Primary" }: ItemImageProps) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    setUrl(null);
    let cancelled = false;
    let objectUrl: string | null = null;
    const token = getToken();
    const fetchArtwork = (type: "Primary" | "Backdrop") => fetch(
      resolveUrl(`/Items/${encodeURIComponent(itemId)}/Images/${type}?v=${type === "Backdrop" ? "b1" : "hq2"}-${revision}`), {
      headers: token ? { "X-Emby-Token": token } : {},
      cache: "default",
    });
    fetchArtwork(imageType)
      .then((response) => {
        if (!response.ok && imageType === "Backdrop") return fetchArtwork("Primary");
        return response;
      })
      .then((response) => {
        if (!response.ok) throw new Error("no artwork");
        return response.blob();
      })
      .then((blob) => {
        if (cancelled) return;
        objectUrl = URL.createObjectURL(blob);
        setUrl(objectUrl);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [itemId, revision, imageType]);

  if (!url) {
    return (
      <div aria-label={name}
        className={`bg-gradient-to-br from-surface-hover to-surface-raised
          ${className ?? ""}`} />
    );
  }
  return (
    <img className={`object-cover ${className ?? ""}`} src={url} alt={name} loading="lazy" />
  );
}
