// Re-records Yonder's motion teasers from the real UI pages.
//
//   node tools/teasers/teasers.mjs              # all scenes
//   node tools/teasers/teasers.mjs hero split   # just these
//
// Needs Node 18+, Chrome/Chromium (set CHROME_PATH if it isn't found) and ffmpeg
// (on PATH, or set FFMPEG). Writes media/<scene>.mp4/.gif/-poster.png, copies the
// MP4s and posters to website/assets/media/, and saves a contact sheet per scene in
// tools/teasers/out/ so you can check the frames before committing.

import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const ROOT = join(HERE, "..", "..");
const MEDIA = join(ROOT, "media");
const SITE_MEDIA = join(ROOT, "website", "assets", "media");
const OUT = join(HERE, "out");
const FFMPEG = process.env.FFMPEG || "ffmpeg";

// scene -> poster time (s), GIF width and fps
const SCENES = {
  hero: { poster: 9.6, gifw: 900, fps: 15 },
  reminders: { poster: 2.9, gifw: 960, fps: 18 },
  settings: { poster: 3.4, gifw: 960, fps: 18 },
  split: { poster: 4.0, gifw: 960, fps: 18 },
};

const wanted = process.argv.slice(2).length ? process.argv.slice(2) : [...Object.keys(SCENES), "settingsLight"];
for (const s of wanted) if (!SCENES[s] && s !== "settingsLight") throw new Error(`unknown scene "${s}" (${Object.keys(SCENES).join(", ")}, settingsLight)`);

const run = (cmd, args, opts = {}) => {
  const r = spawnSync(cmd, args, { stdio: "inherit", ...opts });
  if (r.status !== 0) throw new Error(`${cmd} ${args.slice(0, 3).join(" ")}… failed`);
};
const ff = (args, cwd) => run(FFMPEG, ["-hide_banner", "-loglevel", "error", "-y", ...args], { cwd });

mkdirSync(MEDIA, { recursive: true });
mkdirSync(SITE_MEDIA, { recursive: true });
mkdirSync(OUT, { recursive: true });

// 1. static server for the stage + app pages
const server = spawn(process.execPath, [join(HERE, "serve.mjs")], { stdio: "ignore" });
await new Promise((r) => setTimeout(r, 800));

try {
  for (const scene of wanted) {
    const frames = join(OUT, "frames", scene);
    rmSync(frames, { recursive: true, force: true });
    console.log(`\n● recording ${scene}`);
    run(process.execPath, [join(HERE, "record.mjs"), scene, frames, "16", "4"]);
    run(process.execPath, [join(HERE, "retime.mjs"), frames]);

    if (scene === "settingsLight") {
      // a still of the light theme: the last frame
      const last = readdirSync(frames).filter((f) => f.endsWith(".jpg")).sort().at(-1);
      ff(["-i", join(frames, last), "-vf", "scale=1280:-2:flags=lanczos", join(MEDIA, "settings-light.png")]);
      continue;
    }

    const c = SCENES[scene];
    const mp4 = join(MEDIA, `${scene}.mp4`);
    const poster = join(MEDIA, `${scene}-poster.png`);
    ff(["-f", "concat", "-safe", "0", "-i", "concat.txt", "-vf", "fps=30,scale=1280:-2:flags=lanczos,format=yuv420p",
      "-c:v", "libx264", "-preset", "slow", "-crf", "20", "-movflags", "+faststart", "-an", mp4], frames);
    ff(["-f", "concat", "-safe", "0", "-i", "concat.txt", "-vf",
      `fps=${c.fps},scale=${c.gifw}:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff:max_colors=192[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle`,
      join(MEDIA, `${scene}.gif`)], frames);
    ff(["-ss", String(c.poster), "-i", mp4, "-frames:v", "1", poster]);
    ff(["-f", "concat", "-safe", "0", "-i", "concat.txt", "-vf", "fps=1.2,scale=480:-1,tile=4x4", "-frames:v", "1", join(OUT, `${scene}-sheet.jpg`)], frames);
    copyFileSync(mp4, join(SITE_MEDIA, `${scene}.mp4`));
    copyFileSync(poster, join(SITE_MEDIA, `${scene}-poster.png`));
    console.log(`✓ ${scene}: media/${scene}.mp4, .gif, -poster.png (contact sheet: tools/teasers/out/${scene}-sheet.jpg)`);
  }
} finally {
  server.kill();
}
console.log("\nDone. Check the contact sheets, then commit media/ and website/assets/media/.");
