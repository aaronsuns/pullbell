/** Pure helpers for the searchable lists. */

export interface Searchable {
  repo: string;
  actor?: string;
  text?: string;
  title?: string;
}

/** Every whitespace-separated word of the query must appear in repo, actor, title or text. */
export function matches(item: Searchable, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = [item.repo, item.actor ?? "", item.title ?? "", item.text ?? ""].join(" ").toLowerCase();
  return words.every((w) => hay.includes(w));
}

export function filterItems<T extends Searchable>(items: T[], query: string): T[] {
  return items.filter((i) => matches(i, query));
}

/** Move a selection index by delta, clamped to the list. -1 means nothing selectable. */
export function moveSelection(current: number, delta: number, length: number): number {
  if (length === 0) return -1;
  const start = current < 0 ? (delta > 0 ? -1 : length) : current;
  return Math.min(length - 1, Math.max(0, start + delta));
}

export type ListAction = "up" | "down" | "open" | "done" | "close" | null;

/** Map a keydown in a queue/PR list to an action. */
export function listAction(key: string, shift: boolean): ListAction {
  switch (key) {
    case "ArrowUp":
      return "up";
    case "ArrowDown":
      return "down";
    case "Enter":
      return shift ? "done" : "open";
    case "Escape":
      return "close";
    default:
      return null;
  }
}
