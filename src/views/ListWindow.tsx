import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { filterItems, listAction, moveSelection, type Searchable } from "../lib/filter";

export interface Row extends Searchable {
  id: string;
  section: string;
  glyph: string;
  line1: string;
  line2: string;
  dim?: boolean;
}

interface Props {
  rows: Row[];
  placeholder: string;
  hint: string;
  empty: string;
  onOpen: (r: Row) => void;
  onDone?: (r: Row) => void;
}

/** A searchable keyboard-driven list shared by the queue and my-PRs windows. */
export function ListWindow({ rows, placeholder, hint, empty, onOpen, onDone }: Props) {
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);
  const shown = useMemo(() => filterItems(rows, query), [rows, query]);

  useEffect(() => setSel((s) => Math.min(Math.max(s, 0), shown.length - 1)), [shown.length]);
  useEffect(() => {
    listRef.current?.querySelector(".row.sel")?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const onKey = (e: React.KeyboardEvent) => {
    const action = listAction(e.key, e.shiftKey);
    if (!action) return;
    e.preventDefault();
    const current = shown[sel];
    if (action === "up") setSel((s) => moveSelection(s, -1, shown.length));
    else if (action === "down") setSel((s) => moveSelection(s, 1, shown.length));
    else if (action === "close") void getCurrentWindow().close();
    else if (action === "open" && current) onOpen(current);
    else if (action === "done" && current && onDone) onDone(current);
  };

  let lastSection = "";
  return (
    <div className="list-view" onKeyDown={onKey}>
      <input
        className="search"
        autoFocus
        placeholder={placeholder}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setSel(0);
        }}
      />
      <ul className="list" ref={listRef}>
        {shown.length === 0 && <li className="empty">{empty}</li>}
        {shown.map((r, i) => {
          const header = r.section !== lastSection ? r.section : null;
          lastSection = r.section;
          return [
            header && (
              <li key={`h-${header}`} className="section">
                {header}
              </li>
            ),
            <li
              key={r.id}
              className={`row${i === sel ? " sel" : ""}${r.dim ? " fyi" : ""}`}
              onMouseEnter={() => setSel(i)}
              onClick={() => onOpen(r)}
            >
              <span className="glyph">{r.glyph}</span>
              <span className="main">
                <div className="line1">{r.line1}</div>
                <div className="line2">{r.line2}</div>
              </span>
            </li>,
          ];
        })}
      </ul>
      <div className="hint">{hint}</div>
    </div>
  );
}
