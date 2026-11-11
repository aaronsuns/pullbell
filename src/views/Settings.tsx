import { useEffect, useState } from "react";
import { api, onChanged, type ChromeProfile, type Settings, type SettingsView as View, type Team } from "../api";
import { ShortcutRecorder } from "./ShortcutRecorder";

const lines = (s: string) => s.split(/[\n,]/).map((x) => x.trim()).filter(Boolean);

export function SettingsView() {
  const [view, setView] = useState<View | null>(null);
  const [teams, setTeams] = useState<Team[] | null>(null);
  const [teamError, setTeamError] = useState("");
  const [profiles, setProfiles] = useState<ChromeProfile[]>([]);
  const [extra, setExtra] = useState("");
  const [muted, setMuted] = useState("");

  const load = () =>
    void api.getSettings().then((v) => {
      setView(v);
      setExtra(v.settings.repos.extra.join("\n"));
      setMuted(v.settings.repos.muted.join("\n"));
    });
  useEffect(() => {
    load();
    void api.chromeProfiles().then(setProfiles);
    return onChanged(() => void api.getSettings().then((v) => setView((old) => (old ? { ...old, team_errors: v.team_errors, signed_in: v.signed_in, login: v.login } : v))));
  }, []);

  if (!view) return <div className="page muted">Loading…</div>;
  const s = view.settings;
  const save = async (next: Settings) => {
    setView({ ...view, settings: next });
    const saved = await api.saveSettings(next);
    setView((v) => (v ? { ...v, settings: saved } : v));
  };
  const loadTeams = () => {
    setTeamError("");
    api.listTeams().then(setTeams, (e) => setTeamError(String(e)));
  };
  const allTeams = Array.from(new Set([...(teams ?? []).map((t) => `${t.org}/${t.slug}`), ...s.repos.teams]));
  const sc = s.shortcuts;
  const others = (skip: keyof Settings["shortcuts"]) =>
    (Object.keys(sc) as (keyof Settings["shortcuts"])[]).filter((k) => k !== skip).map((k) => sc[k]).filter((x): x is string => !!x);

  return (
    <div className="page">
      <h2>Repos</h2>
      <div className="check">
        <input type="radio" id="all" checked={s.repos.mode === "everything"} onChange={() => void save({ ...s, repos: { ...s.repos, mode: "everything" } })} />
        <label htmlFor="all">Watch everything</label>
      </div>
      <div className="check">
        <input type="radio" id="allow" checked={s.repos.mode === "allowlist"} onChange={() => { void save({ ...s, repos: { ...s.repos, mode: "allowlist" } }); if (!teams) loadTeams(); }} />
        <label htmlFor="allow">Only repos of my teams, plus the repos listed below</label>
      </div>
      {s.repos.mode === "allowlist" && (
        <div style={{ marginLeft: 22 }}>
          <div className="field">
            <span className="muted">Teams</span>
            <button onClick={loadTeams}>{teams ? "Reload teams" : "Load my teams"}</button>
          </div>
          {teamError && <div className="error">{teamError}</div>}
          {view.team_errors.map((e) => (
            <div className="error" key={e}>Team lookup failed: {e} (only the listed repos are watched)</div>
          ))}
          {allTeams.length > 0 && (
            <div className="teams">
              {allTeams.map((t) => (
                <div className="check" key={t}>
                  <input
                    type="checkbox"
                    id={`t-${t}`}
                    checked={s.repos.teams.includes(t)}
                    onChange={(e) =>
                      void save({ ...s, repos: { ...s.repos, teams: e.target.checked ? [...s.repos.teams, t] : s.repos.teams.filter((x) => x !== t) } })
                    }
                  />
                  <label htmlFor={`t-${t}`}>{t}</label>
                </div>
              ))}
            </div>
          )}
          <p className="muted">Extra repos (owner/name, one per line)</p>
          <textarea value={extra} onChange={(e) => setExtra(e.target.value)} onBlur={() => void save({ ...s, repos: { ...s.repos, extra: lines(extra) } }).then(load)} />
        </div>
      )}
      <p className="muted">Muted repos (always ignored)</p>
      <textarea value={muted} onChange={(e) => setMuted(e.target.value)} onBlur={() => void save({ ...s, repos: { ...s.repos, muted: lines(muted) } }).then(load)} />

      <h2>Alerts</h2>
      <p className="muted">Banner on (unticked kinds still show up as FYI):</p>
      {view.kinds.map((k) => (
        <div className="check" key={k.kind}>
          <input
            type="checkbox"
            id={`k-${k.kind}`}
            checked={s.alerts.banners[k.kind] ?? k.kind !== "other"}
            onChange={(e) => void save({ ...s, alerts: { ...s.alerts, banners: { ...s.alerts.banners, [k.kind]: e.target.checked } } })}
          />
          <label htmlFor={`k-${k.kind}`}>{k.emoji} {k.label}</label>
        </div>
      ))}
      <div className="field">
        <label>Daily digest</label>
        <select
          value={s.alerts.digest_hour ?? ""}
          onChange={(e) => void save({ ...s, alerts: { ...s.alerts, digest_hour: e.target.value === "" ? null : Number(e.target.value) } })}
        >
          <option value="">Off</option>
          {[6, 7, 8, 9, 10, 11, 12].map((h) => (
            <option key={h} value={h}>{String(h).padStart(2, "0")}:00</option>
          ))}
        </select>
      </div>
      <div className="field">
        <label>Deploy workflow pattern</label>
        <input
          defaultValue={s.alerts.failure_pattern}
          onBlur={(e) => void save({ ...s, alerts: { ...s.alerts, failure_pattern: e.target.value } }).then(load)}
        />
      </div>

      <h2>General</h2>
      <div className="field"><label>Open queue</label><ShortcutRecorder value={sc.queue} others={others("queue")} onChange={(v) => void save({ ...s, shortcuts: { ...sc, queue: v } })} /></div>
      <div className="field"><label>Open my PRs</label><ShortcutRecorder value={sc.my_prs} others={others("my_prs")} onChange={(v) => void save({ ...s, shortcuts: { ...sc, my_prs: v } })} /></div>
      <div className="field"><label>Refresh now</label><ShortcutRecorder value={sc.refresh} others={others("refresh")} onChange={(v) => void save({ ...s, shortcuts: { ...sc, refresh: v } })} /></div>
      <div className="field">
        <label>Check GitHub every</label>
        <select value={s.poll_minutes} onChange={(e) => void save({ ...s, poll_minutes: Number(e.target.value) })}>
          {[1, 3, 5, 10].map((m) => (
            <option key={m} value={m}>{m} min</option>
          ))}
        </select>
      </div>
      <div className="check">
        <input type="checkbox" id="login" checked={s.start_at_login} onChange={(e) => void save({ ...s, start_at_login: e.target.checked })} />
        <label htmlFor="login">Start at login</label>
      </div>
      <div className="check">
        <input type="checkbox" id="upd" checked={s.updates.check} onChange={(e) => void save({ ...s, updates: { ...s.updates, check: e.target.checked } })} />
        <label htmlFor="upd">Check for updates</label>
      </div>
      <div className="check">
        <input type="checkbox" id="auto" disabled={!s.updates.check} checked={s.updates.auto_install} onChange={(e) => void save({ ...s, updates: { ...s.updates, auto_install: e.target.checked } })} />
        <label htmlFor="auto">Install updates automatically</label>
      </div>
      <div className="field">
        <span className="muted">
          Pullbell {view.version}
          {!view.updater_configured && " · updates not configured in this build"}
        </span>
        {view.updater_configured && <button onClick={() => void api.checkUpdates()}>Check now</button>}
      </div>
      <div className="field">
        <span className="muted">{view.signed_in ? `Signed in${view.login ? ` as ${view.login}` : ""}` : "Not signed in"}</span>
        {view.signed_in && <button onClick={() => void api.signOut().then(load)}>Sign out</button>}
      </div>

      <h2>Browser</h2>
      <div className="check">
        <input type="radio" id="b-sys" checked={s.browser.type === "system"} onChange={() => void save({ ...s, browser: { type: "system" } })} />
        <label htmlFor="b-sys">System default</label>
      </div>
      <div className="check">
        <input type="radio" id="b-app" checked={s.browser.type === "app"} onChange={() => void save({ ...s, browser: { type: "app", name: "Safari" } })} />
        <label htmlFor="b-app">Browser app</label>
        {s.browser.type === "app" && (
          <input defaultValue={s.browser.name} placeholder="Safari, Firefox, Arc…" onBlur={(e) => void save({ ...s, browser: { type: "app", name: e.target.value || "Safari" } })} />
        )}
      </div>
      <div className="check">
        <input
          type="radio"
          id="b-chrome"
          disabled={profiles.length === 0}
          checked={s.browser.type === "chrome_profile"}
          onChange={() => profiles[0] && void save({ ...s, browser: { type: "chrome_profile", dir: profiles[0].dir, label: profiles[0].name } })}
        />
        <label htmlFor="b-chrome">Chrome profile{profiles.length === 0 && " (Chrome not found)"}</label>
        {s.browser.type === "chrome_profile" && (
          <select
            value={s.browser.dir}
            onChange={(e) => {
              const p = profiles.find((x) => x.dir === e.target.value);
              if (p) void save({ ...s, browser: { type: "chrome_profile", dir: p.dir, label: p.name } });
            }}
          >
            {profiles.map((p) => (
              <option key={p.dir} value={p.dir}>{p.name}{p.email ? ` (${p.email})` : ""}</option>
            ))}
          </select>
        )}
      </div>
      <p className="muted">Changes apply on the next poll.</p>
    </div>
  );
}
