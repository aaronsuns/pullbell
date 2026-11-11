# Pullbell — functional specification

Pullbell is a macOS menu bar app that acts as a GitHub inbox for one person. It raises a banner for the
things that need you and keeps everything else in a quiet FYI list, so review requests and comments on your
pull requests stop getting lost in email.

This document describes **behaviour**. How to build it is up to the implementer.

## Platform and stack

- macOS 13 (Ventura) or newer, Apple silicon and Intel (universal build).
- Tauri 2: Rust backend, TypeScript frontend (React + Vite is fine), Vitest and `cargo test` for tests.
- Menu bar only: no Dock icon (`LSUIElement` / activation policy accessory).
- Bundle identifier `se.zexa.pullbell`. Product name `Pullbell`. Open source, MIT licence, public repo
  `github.com/aaronsuns/pullbell`, product page `https://pullbell.zexa.se` (placeholder for now).

## What counts as an event

Each event has a **kind**. Kinds are either *needs you* (banner + counted in **n**) or *FYI* (counted in
**m**, no banner). The user can turn the banner off per kind in Settings; a turned-off kind becomes FYI, it
is never dropped.

| Kind | Default | Emoji | Trigger |
|---|---|---|---|
| review requested | needs you | 👀 | someone requests your review on a PR (directly, or via a team you are in) |
| comment | needs you | 💬 | a human comments on your PR |
| reply | needs you | ↩️ | a human replies in a review thread you took part in |
| mention | needs you | 💬 | a human @-mentions you |
| approved | needs you | ✅ | your PR gets an approving review |
| changes requested | needs you | ❌ | your PR gets a changes-requested review |
| ready to merge | needs you | 🚀 | your open PR becomes approved, all required checks green, no conflict, not a draft |
| conflict | needs you | ⚠️ | your open PR becomes un-mergeable because of a conflict |
| default-branch failure | needs you | 🔥 | a workflow run on the default branch of a watched repo fails, and its workflow name looks like a deploy or release (configurable pattern, default `deploy|release|publish`) |
| digest | needs you | 📋 | once a day at a chosen hour: the review requests still waiting on you (one banner, one item) |
| bot / other | FYI | 🤖 | anything from a bot account (`[bot]` suffix or `type: Bot`), and every other notification reason |
| CI runs | ignored | — | notifications with reason `ci_activity` (GitHub sends one per workflow run you triggered). Ignored by default: left untouched on GitHub, just not added to the list. Settings → Alerts → **CI run notifications**: ignore (default) / FYI. Failures that matter are covered by the default-branch failure kind and the CI glyph in My open PRs |

Rules:
- Your own actions never create events (your comment, your review, your push).
- "Ready to merge" and "conflict" are **transitions**: they fire when the state changes, not on every poll.
  Keep the last-seen state of each of your open PRs so a restart does not re-fire them.
- An item that is already in the list and gets a newer event of the same thread updates in place (newest
  text, moves to the top) rather than duplicating.
- The very first sync after sign-in is silent: backlog goes into the list without banners.

## Where the data comes from

Use the public GitHub REST and GraphQL APIs with the signed-in user's token.

- `GET /notifications` (participating and not), honouring `Last-Modified` / `If-Modified-Since` and the
  `X-Poll-Interval` header. Notification reasons map to the kinds above; fetch the comment or review that the
  notification's `latest_comment_url` points at to know who acted and what they said (first ~120 characters
  for the banner).
- GraphQL search for **my open PRs** (`is:pr is:open author:@me archived:false`) with review decision,
  mergeable state and the status-check rollup, and for **waiting for my review**
  (`is:pr is:open review-requested:@me archived:false`).
- Actions workflow runs on default branches of watched repos for the failure kind.
- Back off on rate limits (`403`/`429` with `Retry-After` or `X-RateLimit-Reset`). Never poll faster than the
  user's interval or GitHub's `X-Poll-Interval`.

## Scope: which repos are watched

Settings → **Repos**:
- **Watch everything** (default for a new install), or
- an **allowlist** = repos of the GitHub teams the user picks (from `GET /user/teams`, resolved with
  `GET /orgs/{org}/teams/{team}/repos`) plus extra repos typed as `owner/name`;
- **muted repos** always win over the allowlist.

Matching is case-insensitive. If a team lookup fails, keep only the explicitly listed repos and show the
error in Settings; never fall back to "everything" silently.

## Menu bar

Title: `⏺ n+m` style text with a small bell; `n` = needs-you count, `m` = FYI count; just `0` when empty.

Menu, top to bottom:
1. Status line when something is wrong (one of): **Not signed in — Sign in…**, **GitHub is refusing the
   sign-in — retrying**, **Keychain access needed — Try again**, **Authorize for <org>…**,
   **Update available (x.y.z)**.
2. The newest needs-you items (up to 10): `emoji repo · actor action: text`. Clicking opens it in the browser
   and marks it done.
   When no item needs you, one disabled line says so, e.g. **Nothing needs you · 12 FYI** (or just
   **Nothing needs you**), and the FYI part tells the user where those are: **Open queue** lists them.
3. **Clear all (k)**, where k counts every item, labelled so the split is clear: **Clear all (3 + 12 FYI)**
   or **Clear all (12 FYI)**. Marks every item done.
4. **My open PRs (k) ›** submenu: one row per PR with CI state (🟢 green, 🔴 failing, 🟡 running) and review
   state (✅ approved, 👀 waiting, ❌ changes requested, 📝 draft, ⚠️ conflict), `repo#number title`.
5. **Waiting for my review (k) ›** submenu.
6. **Open queue** ⌃⌘G, **Open my PRs** ⌃⌘P, **Refresh now** ⌃⇧⌘G (shortcut shown at the right).
7. **Settings…**, **GitHub notifications** (opens github.com/notifications).
8. **About Pullbell**, **Quit Pullbell**.

## Queue window (⌃⌘G)

A small floating, searchable list of all items (needs-you first, then FYI, newest first within each).
Type to filter by repo, actor and text. ↑/↓ select, **⏎** opens the item and marks it done, **⇧⏎** marks it
done without opening, **Esc** closes. Marking done also marks the GitHub notification thread as read.

## My PRs window (⌃⌘P)

Searchable list: **Waiting for my review** first, then **My open PRs**, with the same status glyphs as the
menu. **⏎** opens the PR, **Esc** closes.

## Settings

- **Repos:** see Scope.
- **Alerts:** one checkbox per kind (banner on/off), digest hour (off, or 06–12), default-branch failure
  pattern.
- **General:** the three shortcuts (recorder, see below), poll interval (1, 3, 5, 10 min; default 3), start
  at login (default on, LaunchAgent via the Tauri autostart plugin), check for updates / install
  automatically, version, **Sign out**.
- **Browser:** system default, a specific browser app, or a Chrome profile (list read from Chrome's
  `Local State` file: profile dir, name, email).

Changes apply on the next poll.

### Shortcut recorder

Click a box, press the keys. **Clear** removes the shortcut. While a box is recording, all of Pullbell's
global shortcuts are paused, so the user can record a combination another row currently uses; they come back
on blur or Esc. Reject a shortcut without at least one of ⌃ ⌥ ⌘, one already used by another Pullbell
shortcut, or one macOS reserves (⌘Space, ⌘⇥, ⇧⌘3/4/5, ⌘Q, ⌃Space, …), with a short reason.

## Sign-in

- GitHub OAuth **device flow** with an OAuth App's client ID baked in at build time (env
  `PULLBELL_CLIENT_ID`). Scopes: `notifications repo read:org`. The welcome window shows the user code and a
  button that opens the verification page.
- Development fallback: if no client ID is built in, use the token of the GitHub CLI (`gh auth token`).
- Store the token, refresh token and expiry in the macOS Keychain (one generic-password item, service
  `Pullbell`). Never write a token to disk or to the log.
- Renew an expiring token with the refresh token before it expires.
- A `401` is not a sign-out. Keep the token, show "refusing the sign-in — retrying", retry with backoff, and
  only ask the user to sign in again after 30 minutes of continuous `401`s.
- SAML SSO: a `403` with an `X-GitHub-SSO: required; url=…` header means the org needs authorizing. Show
  **Authorize for <org>…** with that URL; other repos keep working.
- Keychain denied (user clicked Deny) → **Keychain access needed — Try again**.

## First run (welcome window)

A checklist that ticks itself off: **Sign in with GitHub**, **Choose what to watch** (everything / teams +
repos), **Allow notifications**, **Start at login** (on). **Finish** closes it. It reappears only if signed
out.

## Banners

macOS user notifications. Clicking a banner opens the item and marks it done. Group repeated events per PR so
a burst of comments is one banner, not ten. Respect the per-kind setting.

## Updates

Tauri updater plugin with a signed `latest.json` attached to the newest GitHub release of the public repo.
Check on start and every 6 hours. With **Install automatically** on (default), download and install in the
background, and restart only when no Pullbell window is open; afterwards a banner says **Pullbell updated to
x.y.z**. With it off, the menu shows **Update available (x.y.z)**; clicking installs. A failed attempt of a
given version is retried no sooner than 6 hours later; a newer version is tried at once.

## Persistence and logging

- Settings and state (items, done ids, last PR states, poll cursors) as JSON under
  `~/Library/Application Support/se.zexa.pullbell/`, written atomically. A corrupt file is moved aside and
  replaced by defaults, never a crash.
- Log to `~/Library/Logs/Pullbell/pullbell.log`, rotated at 1 MB, keep 3. One line per poll summary, auth
  change, error and update step. Never tokens, never comment bodies beyond the 120-char banner text.

## Quality bar

- All event classification, transition detection, scope matching, shortcut validation and settings
  migration are pure functions with unit tests, fed by recorded-style JSON fixtures that use only made-up
  names (`octo-org/web-app`, users `alex`, `sam`, `jordan`).
- HTTP is tested against a mock server (e.g. `wiremock`), including 401 streaks, SSO 403, rate limits and
  304 Not Modified.
- `scripts/check.sh` runs format, lint, typecheck and all tests; CI runs it on every push.
- README for end users: what it does, screenshot, install (download DMG, `xattr` step because it is not
  notarized), configure, shortcuts, updates, FAQ, uninstall. MAINTAINING.md for building and releasing.
