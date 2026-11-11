import { api, type Pr } from "../api";
import { ListWindow, type Row } from "./ListWindow";
import { useSnapshot } from "./useSnapshot";

const toRow = (section: string) => (p: Pr): Row => ({
  id: `${section}:${p.url}`,
  section,
  glyph: `${p.ci_glyph}${p.review_glyph}`,
  line1: `${p.repo}#${p.number} ${p.title}`,
  line2: p.author ? `by ${p.author}` : "",
  repo: p.repo,
  actor: p.author,
  title: p.title,
  text: `#${p.number}`,
});

export function MyPrs() {
  const snap = useSnapshot();
  const rows = [
    ...(snap?.waiting ?? []).map(toRow("Waiting for my review")),
    ...(snap?.mine ?? []).map(toRow("My open PRs")),
  ];
  return (
    <ListWindow
      rows={rows}
      placeholder="Filter pull requests"
      hint="↑↓ select · ⏎ open · esc close   🟢 green 🔴 failing 🟡 running · ✅ approved 👀 waiting ❌ changes 📝 draft ⚠️ conflict"
      empty="No open pull requests."
      onOpen={(r) => void api.openUrl(r.id.slice(r.id.indexOf(":") + 1))}
    />
  );
}
