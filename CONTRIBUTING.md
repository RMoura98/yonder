# Contributing to Yonder

Thanks for wanting to help! Yonder is a small project and every bug report, idea and pull request makes a
real difference. This guide gets you from zero to a running dev build.

## Ways to help

- **Report a bug** or **suggest a feature** with the [issue templates](https://github.com/RMoura98/yonder/issues/new/choose).
- **Pick up an issue** labelled [`good first issue`](https://github.com/RMoura98/yonder/labels/good%20first%20issue)
  or [`help wanted`](https://github.com/RMoura98/yonder/labels/help%20wanted). Comment on it first so nobody duplicates work.
- **Test on your platform.** macOS and Linux reports are especially valuable, as are multi-monitor setups.
- **Ask or share ideas** in [Discussions](https://github.com/RMoura98/yonder/discussions).

## Development setup

You need [Rust](https://rustup.rs) (stable), Node.js 20+ and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS (WebView2 on Windows, Xcode command line
tools on macOS, `libwebkit2gtk-4.1-dev` and friends on Linux).

```sh
git clone https://github.com/RMoura98/yonder
cd yonder/app
npm install
npm run tauri dev
```

`tauri dev` watches the UI files and reloads them. Rust changes trigger a rebuild automatically.

Run the tests with:

```sh
cd app/src-tauri
cargo test
```

## Where things live

```
app/src/                 The UI: plain HTML, CSS and JavaScript (no framework, no bundler)
  index.html, js/settings.js   Settings window
  break.html, js/break.js      Break screen (blurred desktop, split screen, backdrops)
  prebreak.html, cursor.html   Heads-up card and the cursor countdown
  reminders.html               Posture, blink and custom reminders
  js/sounds.js                 Synthesized sounds (Web Audio)
  js/backgrounds.js            Animated backdrops, gradients, split-screen textures
app/src-tauri/src/
  engine.rs                The break scheduler: a 1-second state machine that returns effects
  lib.rs                   Tray, windows, commands, global shortcuts, the tick loop
  platform.rs              OS integrations: idle time, smart pause, lock screen, screenshots, tray glyph
  settings.rs, stats.rs    Persisted settings and daily stats
website/                   The project website (static, deployed to GitHub Pages)
```

A good mental model: `engine.rs` decides **what** should happen, `lib.rs` makes it happen on screen,
and `platform.rs` talks to the operating system.

## Adding a translation

All text lives in `app/src/i18n/<code>.json`. The files are keyed by the English text, so anything missing
simply shows in English.

1. Copy `app/src/i18n/en.json` to `app/src/i18n/<code>.json` (e.g. `it.json`) and translate the **values**.
   Keep `{placeholders}` and `<code>`/`<kbd>` tags as they are.
2. Add the language to `LANGUAGES` in `app/src/js/i18n.js` (with its native name) and, if it needs special
   ordinals or plurals, to `ordinal()` / `plural()` there.
3. Add the file to `dicts()` and the code to `SUPPORTED` in `app/src-tauri/src/i18n.rs`. The tray menu and
   status line read the same file.
4. Run the app, pick the language in Settings → General, and check that long strings still fit.

## Pull requests

- Keep each PR focused on one change, and describe what you tested and on which OS.
- Use clear commit messages in the [Conventional Commits](https://www.conventionalcommits.org/) style
  (`feat(ui): …`, `fix(core): …`, `docs: …`).
- Match the existing style: `cargo fmt` for Rust; for the UI, plain modern JavaScript and the CSS tokens in
  `app/src/css/base.css` (both light and dark themes).
- UI changes: please attach a screenshot or short clip.
- Respect privacy: Yonder never sends data anywhere. New features must keep it that way.

## Code of conduct

Be kind, assume good intent, and keep discussions constructive. Harassment of any kind isn't tolerated.
