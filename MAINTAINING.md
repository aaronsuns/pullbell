# Maintaining Pullbell

The behaviour is specified in [docs/SPEC.md](docs/SPEC.md). This file is about building, testing
and releasing.

## Layout

```
src-tauri/src/core/     pure logic, no I/O (classification, PR transitions, scope, digest,
                        deploy failures, shortcuts, settings migration, inbox, health, store)
src-tauri/src/github.rs REST + GraphQL client, HTTP errors → ApiError
src-tauri/src/auth.rs   device flow, refresh, Keychain, `gh auth token` fallback
src-tauri/src/sync.rs   one poll: GitHub → events → inbox state (no UI)
src-tauri/src/app/      Tauri glue: poll loop, tray, banners, shortcuts, windows, commands, updater
src-tauri/src/logging.rs rotating file log
src-tauri/tests/        mock-server tests (wiremock) + JSON fixtures with made-up names
src/                    React frontend; one bundle, the window label picks the view (#queue, …)
src/lib/                pure TS helpers with Vitest tests
```

## Prerequisites

- macOS 13+, Command Line Tools (Xcode not needed), Node 20, Rust stable.
- For universal builds: both Rust targets via rustup
  (`rustup target add aarch64-apple-darwin x86_64-apple-darwin`). If `cargo` on your PATH is
  Homebrew's (no cross targets), put the rustup toolchain first:
  `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`.

## Everyday commands

```sh
npm ci
./scripts/check.sh                       # fmt, clippy -D warnings, tsc, vitest, cargo test (CI runs this)
npm run tauri dev                        # run with hot reload
npm run tauri build -- --bundles app     # host-arch Pullbell.app
npm run tauri build -- --target universal-apple-darwin   # universal .app + .dmg
```

## Sign-in client id

Device flow needs the GitHub OAuth App's client id at **build time** in `PULLBELL_CLIENT_ID`
(read with `option_env!`, so changing it needs a rebuild). The app's id is
`Ov23li41v8r0R8koUOxg` (not a secret). It is stored as the repository variable
`PULLBELL_CLIENT_ID`, which both workflows pass in as `vars.PULLBELL_CLIENT_ID`.

```sh
PULLBELL_CLIENT_ID=Ov23li41v8r0R8koUOxg npm run tauri build -- --bundles app
```

Without it the build is a development build: sign-in uses the GitHub CLI's token
(`gh auth token`), and that token is not stored in the Keychain.

The OAuth App has device flow and expiring user tokens enabled and no client secret. Pullbell
refreshes ten minutes before expiry with `grant_type=refresh_token` and the client id only.
**Check this against a real expiring token.** If GitHub insists on a client secret for refresh,
the fallback is to run the device flow again when the token expires.

## Updater

`src-tauri/tauri.conf.json` → `plugins.updater`:

- `pubkey`: currently the placeholder `REPLACE_WITH_UPDATER_PUBLIC_KEY`. While it is the
  placeholder, the app skips update checks and logs that.
- `endpoints`: `https://github.com/aaronsuns/pullbell/releases/latest/download/latest.json`.

To turn updates on:

1. `npx tauri signer generate -w ~/.tauri/pullbell.key` (keep the private key and its password
   out of the repo).
2. Paste the public key into `plugins.updater.pubkey`.
3. Set `bundle.createUpdaterArtifacts` to `true`.
4. Add repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
5. Each release needs a `latest.json` asset next to the `.app.tar.gz` and `.sig`, for example:

   ```json
   {
     "version": "0.2.0",
     "notes": "…",
     "pub_date": "2026-12-01T12:00:00Z",
     "platforms": {
       "darwin-aarch64": { "signature": "<contents of .sig>", "url": "https://github.com/aaronsuns/pullbell/releases/download/v0.2.0/Pullbell_universal.app.tar.gz" },
       "darwin-x86_64":  { "signature": "<contents of .sig>", "url": "https://github.com/aaronsuns/pullbell/releases/download/v0.2.0/Pullbell_universal.app.tar.gz" }
     }
   }
   ```

## Releasing

1. Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.
2. `./scripts/check.sh`, commit, tag `vX.Y.Z`, push the tag.
3. `.github/workflows/release.yml` builds the universal app + DMG on macOS and attaches them to a
   draft release. Add `latest.json` (see above), write notes, publish.

The app is not notarized, which is why the README has the `xattr` step. Notarizing needs a paid
Apple Developer account; if one is added, sign + notarize in the release workflow and drop that step.

## Runtime files

- Settings and state: `~/Library/Application Support/se.zexa.pullbell/{settings,state}.json`,
  written atomically. An unreadable file is renamed `*.corrupt-<unix time>` and replaced by defaults.
- Log: `~/Library/Logs/Pullbell/pullbell.log`, rotated at 1 MB, three old files kept.
- Token: Keychain generic password, service `Pullbell`, account `github`.
- Login item: `~/Library/LaunchAgents/Pullbell.plist`, managed by the autostart plugin, only
  from an installed `.app` (never from `tauri dev`).

## Design notes and deviations from the spec

- **Banners** use `mac-notification-sys` (NSUserNotification) because Tauri's notification plugin
  cannot report clicks on desktop. Each clickable banner parks a thread until it is clicked or
  dismissed, capped at 16; beyond that banners are not clickable. Moving to
  UNUserNotificationCenter would need a signed app.
- **Allow notifications** in the welcome window ticks after sending a test banner; there is no
  permission API to read back with NSUserNotification.
- **"All required checks green"** uses the commit's `statusCheckRollup` state, which covers all
  checks rather than only the required ones.
- **Deploy-failure repos**: with "watch everything", the repos checked are those you have open PRs
  in or are asked to review in, at most 30.
- **401 back-off**: the next poll waits for the poll interval or 1, 2, 4, 8, … minutes (max 15),
  whichever is longer, so a 401 never makes Pullbell poll faster than your interval.
- **Ready to merge / conflict** keep the previous state while GitHub reports mergeability as
  `UNKNOWN`, so they do not flap.
- **Menu title** is `n+m`, `n` when there is no FYI, `0` when empty.

## TODO

- [ ] Verify refresh of expiring tokens with the real OAuth App (see above).
- [ ] Generate the updater key, set the pubkey and publish `latest.json` (automate it in release.yml).
- [ ] README screenshot.
- [ ] Product page at https://pullbell.zexa.se.
- [ ] Follow `Link` headers for more than 50 unread notifications.
- [ ] UI tests for the windows (currently only the pure TS helpers are tested).
- [ ] Watch the first CI run on ubuntu: the macOS-only code is compiled out there and has only
      been built on macOS so far.
