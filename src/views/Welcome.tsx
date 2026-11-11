import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, onAuth, type Settings, type SignInStart } from "../api";

export function Welcome() {
  const [signedIn, setSignedIn] = useState(false);
  const [login, setLogin] = useState<string | null>(null);
  const [code, setCode] = useState<SignInStart | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [bannerTested, setBannerTested] = useState(false);

  const refresh = () =>
    void api.getSettings().then((v) => {
      setSignedIn(v.signed_in);
      setLogin(v.login);
      setSettings(v.settings);
    });
  useEffect(() => {
    refresh();
    return onAuth((p) => {
      if (p.error) setError(p.error);
      setCode(null);
      setBusy(false);
      refresh();
    });
  }, []);

  const signIn = async () => {
    setBusy(true);
    setError("");
    try {
      const start = await api.signInStart();
      if (start.mode === "device") {
        setCode(start);
        await navigator.clipboard?.writeText(start.user_code).catch(() => undefined);
        void api.openUrl(start.verification_uri);
      } else {
        setBusy(false);
        refresh();
      }
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };
  const save = async (next: Settings) => setSettings(await api.saveSettings(next));
  const tick = (done: boolean) => <span className="tick">{done ? "✅" : "⬜️"}</span>;

  return (
    <div className="page">
      <h1 style={{ marginTop: 0 }}>🔔 Welcome to Pullbell</h1>
      <p className="muted">Pullbell sits in your menu bar and rings when a pull request needs you.</p>
      <ul className="checklist">
        <li>
          {tick(signedIn)}
          <div>
            <b>Sign in with GitHub</b>
            {signedIn ? (
              <div className="muted">Signed in{login ? ` as ${login}` : ""}.</div>
            ) : code?.mode === "device" ? (
              <div>
                <div>Enter this code on GitHub (it is on your clipboard):</div>
                <div className="code">{code.user_code}</div>
                <button onClick={() => void api.openUrl(code.verification_uri)}>Open GitHub</button>{" "}
                <button onClick={() => void api.signInCancel()}>Cancel</button>
              </div>
            ) : (
              <div>
                <button className="primary" disabled={busy} onClick={() => void signIn()}>Sign in…</button>
              </div>
            )}
            {error && <div className="error">{error}</div>}
          </div>
        </li>
        <li>
          {tick(signedIn && !!settings)}
          <div>
            <b>Choose what to watch</b>
            {settings && (
              <div>
                <div className="check">
                  <input type="radio" id="w-all" checked={settings.repos.mode === "everything"} onChange={() => void save({ ...settings, repos: { ...settings.repos, mode: "everything" } })} />
                  <label htmlFor="w-all">Everything</label>
                </div>
                <div className="check">
                  <input type="radio" id="w-some" checked={settings.repos.mode === "allowlist"} onChange={() => void save({ ...settings, repos: { ...settings.repos, mode: "allowlist" } })} />
                  <label htmlFor="w-some">Teams + repos (pick them in Settings)</label>
                </div>
              </div>
            )}
          </div>
        </li>
        <li>
          {tick(bannerTested)}
          <div>
            <b>Allow notifications</b>
            <div className="muted">Send a test banner; if macOS asks, click Allow.</div>
            <button onClick={() => void api.testBanner().then(() => setBannerTested(true))}>Send test banner</button>
          </div>
        </li>
        <li>
          {tick(!!settings?.start_at_login)}
          <div>
            <b>Start at login</b>
            {settings && (
              <div className="check">
                <input type="checkbox" id="w-login" checked={settings.start_at_login} onChange={(e) => void save({ ...settings, start_at_login: e.target.checked })} />
                <label htmlFor="w-login">Open Pullbell when I log in</label>
              </div>
            )}
          </div>
        </li>
      </ul>
      <p style={{ textAlign: "right" }}>
        <button className="primary" onClick={() => void api.finishWelcome().then(() => getCurrentWindow().close())}>
          Finish
        </button>
      </p>
    </div>
  );
}
