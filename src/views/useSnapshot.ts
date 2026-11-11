import { useEffect, useState } from "react";
import { api, onChanged, type Snapshot } from "../api";

export function useSnapshot(): Snapshot | null {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  useEffect(() => {
    const load = () => void api.snapshot().then(setSnap);
    load();
    const off = onChanged(load);
    window.addEventListener("focus", load);
    return () => {
      off();
      window.removeEventListener("focus", load);
    };
  }, []);
  return snap;
}
