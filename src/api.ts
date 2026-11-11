import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Kind =
  | "review_requested" | "comment" | "reply" | "mention" | "approved" | "changes_requested"
  | "ready_to_merge" | "conflict" | "deploy_failure" | "digest" | "other";

export interface Item {
  key: string;
  kind: Kind;
  repo: string;
  number: number | null;
  title: string;
  actor: string;
  action: string;
  text: string;
  url: string;
  updated_at: string;
  needs_you: boolean;
  emoji: string;
  row: string;
}

export interface Pr {
  repo: string;
  number: number;
  title: string;
  url: string;
  author: string;
  is_draft: boolean;
  ci_glyph: string;
  review_glyph: string;
}

export interface Snapshot {
  items: Item[];
  mine: Pr[];
  waiting: Pr[];
  status_label: string | null;
  signed_in: boolean;
  login: string | null;
  needs_you: number;
  fyi: number;
  version: string;
}

export type Browser =
  | { type: "system" }
  | { type: "app"; name: string }
  | { type: "chrome_profile"; dir: string; label: string };

export interface Settings {
  version: number;
  repos: { mode: "everything" | "allowlist"; teams: string[]; extra: string[]; muted: string[] };
  alerts: { banners: Record<string, boolean>; digest_hour: number | null; failure_pattern: string; ci_runs: "ignore" | "fyi" };
  shortcuts: { queue: string | null; my_prs: string | null; refresh: string | null };
  poll_minutes: number;
  start_at_login: boolean;
  updates: { check: boolean; auto_install: boolean };
  browser: Browser;
  welcome_done: boolean;
}

export interface SettingsView {
  settings: Settings;
  kinds: { kind: Kind; label: string; emoji: string }[];
  team_errors: string[];
  autostart: boolean;
  version: string;
  updater_configured: boolean;
  signed_in: boolean;
  login: string | null;
}

export interface Team { org: string; slug: string; name: string }
export interface ChromeProfile { dir: string; name: string; email: string }
export type SignInStart = { mode: "device"; user_code: string; verification_uri: string } | { mode: "gh" };

export const api = {
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  markDone: (key: string, open: boolean) => invoke<void>("mark_done", { key, open }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  refreshNow: () => invoke<void>("refresh_now"),
  getSettings: () => invoke<SettingsView>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  validateShortcut: (candidate: string, others: string[]) =>
    invoke<string>("validate_shortcut", { candidate, others }),
  pauseShortcuts: () => invoke<void>("pause_shortcuts"),
  resumeShortcuts: () => invoke<void>("resume_shortcuts"),
  listTeams: () => invoke<Team[]>("list_teams"),
  chromeProfiles: () => invoke<ChromeProfile[]>("chrome_profiles"),
  signInStart: () => invoke<SignInStart>("sign_in_start"),
  signInCancel: () => invoke<void>("sign_in_cancel"),
  signOut: () => invoke<void>("sign_out"),
  testBanner: () => invoke<void>("test_banner"),
  finishWelcome: () => invoke<void>("finish_welcome"),
  checkUpdates: () => invoke<void>("check_updates"),
};

/** Re-run `fn` whenever the backend says data changed. Returns an unsubscribe function. */
export function onChanged(fn: () => void): () => void {
  const p = listen("pullbell://changed", fn);
  return () => void p.then((un) => un());
}

export function onAuth(fn: (payload: { signed_in?: boolean; error?: string }) => void): () => void {
  const p = listen<{ signed_in?: boolean; error?: string }>("pullbell://auth", (e) => fn(e.payload));
  return () => void p.then((un) => un());
}
