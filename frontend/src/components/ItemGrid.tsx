import type { Item } from "../api/types";
import { ItemCard } from "./ItemCard";

interface ItemGridProps {
  items: Item[];
  emptyMessage?: string;
  layout?: "poster" | "episode";
  onRemove?: (itemId: string) => void;
}

export function ItemGrid({ items, emptyMessage = "No items found.", layout = "poster", onRemove }: ItemGridProps) {
  if (items.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-edge py-16 text-center
        text-sm text-ink-muted">
        {emptyMessage}
      </div>
    );
  }
  return (
    <div className={layout === "episode"
      ? "grid gap-5 sm:grid-cols-2 xl:grid-cols-3"
      : "grid grid-cols-2 gap-x-4 gap-y-6 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"}>
      {items.map((item) => (
        <ItemCard key={item.Id} item={item} layout={layout} onRemove={onRemove} />
      ))}
    </div>
  );
}
