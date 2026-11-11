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

`src-tauri/tauri.conf.json` → `plugins.updater` holds the updater's public key (`pubkey`) and the
endpoint `https://github.com/aaronsuns/pullbell/releases/latest/download/latest.json`, and
`bundle.createUpdaterArtifacts` is `true`, so a build signed with the matching private key also
produces `Pullbell.app.tar.gz` + `.sig`. The release workflow publishes those as
`Pullbell_X.Y.Z_universal.app.tar.gz(.sig)` together with a `latest.json` that points all three
macOS platform keys (`darwin-aarch64`, `darwin-x86_64`, `darwin-universal`) at that one universal
bundle. The endpoint only answers once the repository is public.

If the key is ever lost, generate a new pair (`npx tauri signer generate`), replace `pubkey`, and
update the two `TAURI_SIGNING_*` secrets. Installed copies signed for the old key cannot verify
new updates, so their users have to download the next DMG by hand once.

## Signing and secrets

The app is code-signed with a **self-signed** certificate, "Pullbell Self-Signed". There is no
Apple Developer ID and no notarization, which is why the README has the `xattr` step. A stable
signature still matters: macOS keeps Keychain and notification permissions across updates only
while the signing identity stays the same.

All signing material lives in the GitHub repository secrets, with an encrypted backup kept by the
owner:

| Secret | What |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | updater (minisign) key |
| `APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERTIFICATE_PASSWORD` | the self-signed code-signing certificate |
| `APPLE_SIGNING_IDENTITY` | its name, `Pullbell Self-Signed` |

Plus the repository variable `PULLBELL_CLIENT_ID` (see above). The release workflow imports the
certificate into a temporary keychain, marks it trusted for code signing on the runner, and
checks that `codesign -dv` shows `Authority=Pullbell Self-Signed` before publishing.

## Releasing

1. Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
   (and run `cargo check` in `src-tauri` so `Cargo.lock` follows). They must match; the workflow
   refuses a tag that differs.
2. `./scripts/check.sh`, commit, push to `main`, and wait for CI to go green.
3. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. `.github/workflows/release.yml` builds the universal app on macOS, signs it, verifies the
   signature, and publishes the release (not a draft) with the DMG, the updater bundle + `.sig`
   and `latest.json`, with install steps and generated notes. Edit the notes on GitHub if needed.
5. A failed run: fix on `main`, then delete the release and tag and tag again:
   `gh release delete vX.Y.Z --cleanup-tag -y && git tag -d vX.Y.Z`, then step 3.

Installed copies pick the new version up through `latest.json` within six hours.

## Website

<https://pullbell.zexa.se> is the static page in `site/public/`, served by the Cloudflare Worker
`pullbell-site` (static assets only, custom domain set in `site/wrangler.toml`). Deploy with
Node 22: `cd site && npx -y wrangler@4 deploy` (logged in to the Cloudflare account that holds
the zexa.se zone). Keep `sitemap.xml`'s `lastmod` and the JSON-LD `softwareVersion` current.

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
- [ ] README screenshot.
- [ ] Follow `Link` headers for more than 50 unread notifications.
- [ ] UI tests for the windows (currently only the pure TS helpers are tested).
