import { useEffect, useState } from "react";
import { api } from "../api";

export function About() {
  const [version, setVersion] = useState("");
  useEffect(() => void api.snapshot().then((s) => setVersion(s.version)), []);
  return (
    <div className="page center">
      <h1 style={{ margin: "8px 0 0" }}>🔔 Pullbell</h1>
      <p className="muted">Version {version}</p>
      <p>A menu bar GitHub inbox for review requests and comments on your pull requests.</p>
      <p>
        <a onClick={() => void api.openUrl("https://pullbell.zexa.se")}>pullbell.zexa.se</a>
        {" · "}
        <a onClick={() => void api.openUrl("https://github.com/aaronsuns/pullbell")}>Source on GitHub</a>
      </p>
      <p className="muted">Open source under the MIT licence.</p>
    </div>
  );
}
