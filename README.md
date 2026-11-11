# Pullbell

<img src="docs/icon-1024.png" width="96" align="right" alt="Pullbell icon">

A GitHub inbox in your Mac's menu bar. Pullbell rings (a macOS banner) for the things that need
you — review requests, comments and reviews on your pull requests, replies, mentions, a PR that
just became ready to merge or got a conflict, a failed deploy — and keeps everything else in a
quiet FYI list. No more review requests lost in email.

The menu bar shows `n+m`: **n** things need you, **m** are FYI.

> Screenshot: _coming with the first release._

## What rings

| | Kind | When |
|---|---|---|
| 👀 | Review requested | someone asks for your review (directly or via one of your teams) |
| 💬 | Comment | a person comments on your PR |
| ↩️ | Reply | a person replies in a review thread you took part in |
| 💬 | Mention | a person @-mentions you |
| ✅ | Approved | your PR is approved |
| ❌ | Changes requested | a review on your PR requests changes |
| 🚀 | Ready to merge | your PR is approved, checks are green, no conflict, not a draft |
| ⚠️ | Conflict | your PR just got a merge conflict |
| 🔥 | Deploy failed | a deploy/release/publish workflow failed on a watched repo's default branch |
| 📋 | Daily digest | once a day, the review requests still waiting on you (off by default) |
| 🤖 | Bot / other | bots and everything else — FYI, no banner |

Your own comments, reviews and pushes never ring. A burst of comments on one PR is one banner.
Every kind can be switched to FYI in Settings → Alerts.

## Install

1. Download `Pullbell_x.y.z_universal.dmg` from the
   [latest release](https://github.com/aaronsuns/pullbell/releases/latest) and drag Pullbell to
   Applications. Needs macOS 13 or newer, Apple silicon or Intel.
2. Pullbell is not notarized by Apple, so macOS will refuse to open it the first time. Clear the
   quarantine flag once in Terminal:

   ```sh
   xattr -dr com.apple.quarantine /Applications/Pullbell.app
   ```

3. Open Pullbell. A bell appears in the menu bar and the welcome window opens.

## Set up

The welcome window walks you through it:

- **Sign in with GitHub.** Pullbell shows a short code (also copied to your clipboard) and opens
  GitHub's device page; paste the code there and approve. Pullbell asks for the `notifications`,
  `repo` and `read:org` scopes. The token is kept in your macOS Keychain, never on disk.
- **Choose what to watch.** Everything (default), or only the repos of the GitHub teams you pick
  plus extra repos you type as `owner/name`. Muted repos are always ignored.
- **Allow notifications.** Send a test banner; if macOS asks, click Allow.
- **Start at login.** On by default.

If your organization uses SAML single sign-on, the menu shows **Authorize for <org>…** — click it
to authorize Pullbell for that org. Other repos keep working meanwhile.

## Using it

| Shortcut | |
|---|---|
| ⌃⌘G | Open the queue: every item, needs-you first. Type to filter, ↑/↓, ⏎ opens and marks done, ⇧⏎ marks done, esc closes |
| ⌃⌘P | Open my PRs: PRs waiting for your review, then your own open PRs |
| ⌃⇧⌘G | Refresh now |

Shortcuts can be changed or cleared in Settings → General (click the box, press the keys).
Marking an item done also marks the GitHub notification as read.

Settings also has the poll interval (1, 3, 5 or 10 minutes; default 3), the daily digest hour,
the deploy-workflow name pattern (default `deploy|release|publish`), and which browser opens
links: the system default, a specific browser app, or a specific Chrome profile.

## Updates

Pullbell checks for updates at start and every 6 hours. With **Install automatically** on (the
default) it updates in the background and restarts when none of its windows is open, then says
**Pullbell updated to x.y.z**. With it off, the menu shows **Update available (x.y.z)**; click it
to install.

## FAQ

**The menu says "GitHub is refusing the sign-in — retrying".** GitHub answered 401. That is often
temporary, so Pullbell keeps your sign-in and retries. If it lasts 30 minutes you are asked to
sign in again.

**"Keychain access needed — Try again".** You clicked Deny on the Keychain prompt. Click the line
and choose Always Allow.

**I don't get banners.** Check System Settings → Notifications → Pullbell, and that the kind is
ticked in Settings → Alerts.

**Does Pullbell send my data anywhere?** No. It talks only to GitHub's API with your token. It
keeps its list in `~/Library/Application Support/se.zexa.pullbell/` and a small log (no tokens, no
full comment text) in `~/Library/Logs/Pullbell/`.

## Uninstall

1. Quit Pullbell from its menu and move `/Applications/Pullbell.app` to the Trash.
2. Remove its data and login item:

   ```sh
   rm -rf ~/Library/Application\ Support/se.zexa.pullbell ~/Library/Logs/Pullbell
   rm -f ~/Library/LaunchAgents/Pullbell.plist
   security delete-generic-password -s Pullbell
   ```

3. Optionally revoke access at GitHub → Settings → Applications → Authorized OAuth Apps.

## Licence

MIT. See [LICENSE](LICENSE). Product page: <https://pullbell.zexa.se>. Building and releasing:
[MAINTAINING.md](MAINTAINING.md).
