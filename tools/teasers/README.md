# Teaser recorder

The GIFs in the README and the videos on the website are recorded from Yonder's **real UI pages**, not mockups.
`stage.html` is a fake desktop that loads the app's actual `app/src/*.html` pages in iframes, with a stand-in for
the Tauri API (`stub.js`) driven by a scripted timeline (`stage.js`). Headless Chrome records it frame by frame.

## When to run it

Whenever a change alters something visible in a teaser, re-record the affected scenes **in the same change**:

| Scene | Shows |
|---|---|
| `hero` | heads-up card, cursor countdown, blurred-desktop break |
| `reminders` | posture and blink reminders |
| `settings` | settings window tour (dark) |
| `settingsLight` | settings window still (light), saved as `media/settings-light.png` |
| `split` | split-screen break |

## Usage

Requires Node 18+, Chrome or Chromium (set `CHROME_PATH` if it isn't found) and `ffmpeg` (on `PATH`, or set `FFMPEG`).

```sh
node tools/teasers/teasers.mjs            # every scene
node tools/teasers/teasers.mjs hero split # only these
```

It writes `media/<scene>.mp4`, `.gif` and `-poster.png`, copies the MP4s and posters to `website/assets/media/`, and
saves a contact sheet per scene in `tools/teasers/out/` (git-ignored). Look at the sheets, then commit `media/` and
`website/assets/media/` together with the UI change.

If the UI gains a new command the pages call, add it to the `invoke()` switch in `stage.js`.
