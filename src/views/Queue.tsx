import { api } from "../api";
import { ListWindow, type Row } from "./ListWindow";
import { useSnapshot } from "./useSnapshot";

export function Queue() {
  const snap = useSnapshot();
  const rows: Row[] = (snap?.items ?? []).map((i) => ({
    id: i.key,
    section: i.needs_you ? "Needs you" : "FYI",
    glyph: i.emoji,
    line1: `${i.repo}${i.number ? `#${i.number}` : ""} · ${i.actor ? `${i.actor} ` : ""}${i.action}`,
    line2: i.text || i.title,
    dim: !i.needs_you,
    repo: i.repo,
    actor: i.actor,
    text: i.text,
    title: i.title,
  }));
  return (
    <ListWindow
      rows={rows}
      placeholder="Filter by repo, person or text"
      hint="↑↓ select · ⏎ open and mark done · ⇧⏎ mark done · esc close"
      empty={snap?.signed_in === false ? "Not signed in." : "Inbox zero. Nothing waiting."}
      onOpen={(r) => void api.markDone(r.id, true)}
      onDone={(r) => void api.markDone(r.id, false)}
    />
  );
}
