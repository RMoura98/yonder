# Flatpak (Flathub)

App ID: `io.github.rmoura98.yonder` (Flathub requires `io.github.<user>.<app>` for
GitHub-hosted apps; ids ending in `.desktop` are rejected by its linter). The app's
internal Tauri identifier deliberately stays `app.yonder.desktop` so existing users'
settings aren't moved, which means inside the sandbox its data lives at
`~/.var/app/io.github.rmoura98.yonder/config/app.yonder.desktop` (settings) and
`~/.var/app/io.github.rmoura98.yonder/data/app.yonder.desktop` (data), because Tauri
resolves them from `XDG_CONFIG_HOME` / `XDG_DATA_HOME`. The manifest repackages the
released `.deb` instead of building from source, so it needs no vendored crates
or npm sources. `.github/workflows/flatpak.yml` bumps the `.deb` URL and sha256
and adds an AppStream release entry after every GitHub release, and test-builds
the Flatpak whenever this folder changes (`appstreamcli validate`,
`desktop-file-validate`, `flatpak-builder`).

Files:

- `io.github.rmoura98.yonder.yml`: the manifest (GNOME 49 runtime, which ships webkit2gtk-4.1; libappindicator is built from `flathub/shared-modules` for the tray icon)
- `io.github.rmoura98.yonder.metainfo.xml`: AppStream metadata (screenshots are raw GitHub URLs of `media/*-poster.png`)
- `io.github.rmoura98.yonder.desktop`: desktop entry

## Build and run locally

```sh
cd packaging/flatpak
git clone --depth 1 https://github.com/flathub/shared-modules.git
flatpak install flathub org.gnome.Platform//49 org.gnome.Sdk//49
flatpak-builder --user --install --force-clean build io.github.rmoura98.yonder.yml
flatpak run io.github.rmoura98.yonder
```

## Submitting to Flathub (maintainer, one time)

Needs your GitHub account; an agent can't do it for you.

1. Read <https://docs.flathub.org/docs/for-app-authors/submission>.
2. Fork <https://github.com/flathub/flathub> and check out a branch from `new-pr`
   (not `master`): `git checkout -b yonder new-pr`.
3. In the fork add `io.github.rmoura98.yonder.yml`, `io.github.rmoura98.yonder.desktop` and
   `io.github.rmoura98.yonder.metainfo.xml` from this folder, plus the shared module:
   `git submodule add https://github.com/flathub/shared-modules.git shared-modules`.
   Flathub builds the manifest in the repo root, so copy the files to the root.
4. Open a PR against `flathub/flathub` branch `new-pr`, titled `Add io.github.rmoura98.yonder`.
   Answer the checklist. Expect to justify: repackaging the official `.deb` (reviewers
   prefer building from source; offer to move to vendored sources if asked) and why
   each `--talk-name` is needed (tray, notifications, idle detection).
   Comment `bot, build` on the PR to trigger a test build.
5. Fix review feedback. After the merge Flathub creates `flathub/io.github.rmoura98.yonder`
   and you get an invite to it; accept it and enable 2FA.
6. Verification: `io.github.rmoura98.yonder` is verified through your GitHub account
   (Flathub's login-provider method); no domain is needed.
7. Updates: in the Flathub repo, `flatpak-external-data-checker` (configured by the
   `x-checker-data` block) opens PRs for each new release. Keep this folder in sync, or
   copy the updated files over after releases.

## Known limitations inside the sandbox

- Meeting detection (needs `pactl`) and pausing media (needs `playerctl`) rely on
  host tools that aren't in the GNOME runtime, so they are inactive in the Flatpak.
- "Start at login" writes to `~/.config/autostart`, which the sandbox can't reach.
- Idle detection uses `gdbus` from the runtime against GNOME (Mutter) and KDE
  Plasma (`org.freedesktop.ScreenSaver`).
