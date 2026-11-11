import { useState } from "react";
import { api } from "../api";
import { display, modifierGlyphs, toAccelerator } from "../lib/shortcut";

interface Props {
  value: string | null;
  others: string[];
  onChange: (acc: string | null) => void;
}

/**
 * Click, then press the combination. While recording, Pullbell's global shortcuts are paused so
 * a combination another row uses can be recorded; they come back on blur or Esc.
 */
export function ShortcutRecorder({ value, others, onChange }: Props) {
  const [recording, setRecording] = useState(false);
  const [live, setLive] = useState("");
  const [error, setError] = useState("");

  const start = () => {
    setRecording(true);
    setError("");
    setLive("");
    void api.pauseShortcuts();
  };
  const stop = () => {
    setRecording(false);
    setLive("");
    void api.resumeShortcuts();
  };

  const onKeyDown = async (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      e.currentTarget.blur();
      return;
    }
    const acc = toAccelerator(e.nativeEvent);
    if (!acc) {
      setLive(modifierGlyphs(e.nativeEvent));
      return;
    }
    try {
      const ok = await api.validateShortcut(acc, others);
      setError("");
      onChange(ok);
      (e.target as HTMLElement).blur();
    } catch (err) {
      setError(String(err));
      setLive("");
    }
  };

  return (
    <span>
      <span style={{ display: "inline-flex", gap: 6, alignItems: "center" }}>
        <div
          tabIndex={0}
          className={`recorder${recording ? " recording" : ""}`}
          onFocus={start}
          onBlur={stop}
          onKeyDown={(e) => void onKeyDown(e)}
        >
          {recording ? live || "Press keys…" : display(value) || "None"}
        </div>
        <button onClick={() => onChange(null)} disabled={!value}>
          Clear
        </button>
      </span>
      {error && <div className="error">{error}</div>}
    </span>
  );
}
