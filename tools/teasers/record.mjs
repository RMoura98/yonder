// Usage: node record.mjs <scene> <framesDir> [maxSeconds] [slow]
// Slow-motion capture: all page clocks, timers and animations run <slow>x slower,
// frames are grabbed as fast as possible with real timestamps, and timing.txt
// records each frame's position on the *scene* clock for later retiming.
import { spawn } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const [, , scene, outDir, maxSec = "16", slowArg = "4"] = process.argv;
const K = Number(slowArg);
import { existsSync } from "node:fs";
// Chrome/Chromium: $CHROME_PATH, or the usual install location for this OS.
const CHROME = process.env.CHROME_PATH || [
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
].find((p) => existsSync(p));
if (!CHROME) throw new Error("Chrome not found; set CHROME_PATH");
const PORT = 9333;
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
const profile = join(tmpdir(), "yonder-teaser-" + scene);
rmSync(profile, { recursive: true, force: true });

const chrome = spawn(CHROME, [
  "--headless=new", `--remote-debugging-port=${PORT}`, "--window-size=1600,1000", "--hide-scrollbars",
  "--force-device-scale-factor=1", `--user-data-dir=${profile}`, "--autoplay-policy=no-user-gesture-required",
  "--force-color-profile=srgb", "--lang=en-GB", "about:blank",
], { stdio: "ignore" });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let target;
for (let i = 0; i < 50 && !target; i++) {
  await sleep(200);
  try { target = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === "page"); } catch {}
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
const waiters = [];
ws.addEventListener("message", (ev) => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  if (m.method === "Runtime.exceptionThrown") console.log("[exception]", m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text);
  if (m.method === "Runtime.consoleAPICalled" && m.params.type === "error") console.log("[console]", m.params.args.map((a) => a.value ?? a.description).join(" "));
  for (const w of waiters.slice()) if (w.method === m.method) { waiters.splice(waiters.indexOf(w), 1); w.res(m.params); }
});
const send = (method, params = {}) => new Promise((res) => { const i = ++id; pending.set(i, (m) => res(m.result ?? m)); ws.send(JSON.stringify({ id: i, method, params })); });
const once = (method) => new Promise((res) => waiters.push({ method, res }));
const evalJs = async (expression) => (await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }))?.result?.value;
await new Promise((r) => ws.addEventListener("open", r));

// Slow every frame's notion of time by K (timers, rAF timestamps, performance.now, Date).
const TIME_SCALE = `(() => {
  if (window.__slowed) return; window.__slowed = true;
  const K = ${K};
  const st = window.setTimeout, si = window.setInterval;
  window.setTimeout = (f, d = 0, ...a) => st(f, d * K, ...a);
  window.setInterval = (f, d = 0, ...a) => si(f, d * K, ...a);
  const pn = performance.now.bind(performance), t0 = pn();
  performance.now = () => t0 + (pn() - t0) / K;
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => raf((ts) => cb(t0 + (ts - t0) / K));
  const RD = Date, real0 = RD.now(), d0 = new RD(real0).setHours(14, 32, 5, 0);
  const now = () => d0 + (RD.now() - real0) / K;
  class D extends RD { constructor(...a) { a.length ? super(...a) : super(now()); } static now() { return now(); } }
  window.Date = D;
})();`;

await send("Page.enable");
await send("Runtime.enable");
await send("Animation.enable");
await send("Page.addScriptToEvaluateOnNewDocument", { source: TIME_SCALE });
await send("Emulation.setDeviceMetricsOverride", { width: 1600, height: 1000, deviceScaleFactor: 1, mobile: false });
const loaded = once("Page.loadEventFired");
await send("Page.navigate", { url: `http://127.0.0.1:8765/stage.html?scene=${scene}` });
await loaded;
await sleep(1500);

await evalJs("TEASER.prepShot()");
await sleep(300);
const shot = await send("Page.captureScreenshot", { format: "jpeg", quality: 92 });
await evalJs(`TEASER.shot = "data:image/jpeg;base64,${shot.data}"`);
await evalJs("TEASER.afterShot()");
await sleep(300);

await send("Animation.setPlaybackRate", { playbackRate: 1 / K });
await send("Runtime.evaluate", { expression: "TEASER.run()" });
const start = Date.now();
const times = [];
let n = 0;
while ((Date.now() - start) / K < Number(maxSec) * 1000) {
  const f = await send("Page.captureScreenshot", { format: "jpeg", quality: 93 });
  const t = (Date.now() - start) / K; // scene-clock ms (approximate capture moment)
  writeFileSync(join(outDir, `f${String(n).padStart(4, "0")}.jpg`), Buffer.from(f.data, "base64"));
  times.push(t);
  n++;
  if (n % 15 === 0 && (await evalJs("TEASER.done"))) break;
}
writeFileSync(join(outDir, "timing.txt"), times.map((t) => t.toFixed(1)).join("\n"));
console.log((await evalJs("LOG.join(\"\\n\")")) || "");
console.log(`${scene}: ${n} frames, ${(times.at(-1) / 1000).toFixed(2)}s scene time, ${(n / (times.at(-1) / 1000)).toFixed(1)} fps effective`);
ws.close();
chrome.kill();
process.exit(0);
