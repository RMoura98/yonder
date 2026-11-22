// Teaser stage: fake desktop + real Yonder pages in iframes, driven by a scripted timeline.
const W = 1600, H = 1000;
const scene = new URLSearchParams(location.search).get("scene") || "hero";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const days7 = [true, true, true, true, true, false, false];

const settings = {
  mode: "balanced", work_mins: 20, break_secs: 20, long_enabled: false, long_mins: 5, long_every: 3,
  skip_difficulty: "balanced", wait_while_typing: true, allow_end_early: true, lock_on_break: false,
  prebreak_enabled: true, prebreak_secs: 60, prebreak_visible_secs: 10,
  messages: [
    "Look at something at least 20 feet away.",
    "Let your eyes go soft. Breathe out slowly.",
    "Roll your shoulders back a few times.",
    "Stand up and stretch — your back will thank you.",
    "Sip some water.",
  ],
  break_style: "fullscreen", background: "blur", gradient: "dusk", has_image: false, texture: "linen",
  sound_enabled: true, sound: "chime", volume: 0.6, idle_reset_mins: 5,
  smart: { enabled: true, meetings: true, fullscreen: true, apps: ["obs64", "POWERPNT"], cooldown_secs: 60 },
  office_hours: { enabled: true, days: days7.slice(), start: "09:00", end: "18:00" },
  planned: [{ id: "lunch", enabled: true, title: "Lunch", icon: "restaurant", time: "12:30", duration_mins: 30, days: days7.slice() }],
  wellness: {
    posture: { id: "posture", enabled: true, title: "Sit tall", body: "Shoulders back, feet flat, screen at eye level.", icon: "accessibility_new", interval_mins: 30, size: "medium", sound: "soft", sticky: false },
    blink: { id: "blink", enabled: true, title: "Blink slowly", body: "Close your eyes for a moment and blink a few times.", icon: "visibility", interval_mins: 15, size: "small", sound: "none", sticky: false },
    custom: [{ id: "c_water", enabled: true, title: "Drink water", body: "", icon: "water_drop", interval_mins: 45, size: "medium", sound: "drop", sticky: false }],
    combine: true, reset_after_break: false, ripple: true,
  },
  focus: { work_mins: 25, break_mins: 5, long_break_mins: 15, long_every: 4, merge_window_mins: 3 },
  shortcuts: { take_break: "CmdOrCtrl+Alt+B", toggle_pause: "CmdOrCtrl+Alt+P", skip_break: "CmdOrCtrl+Alt+S", toggle_focus: "" },
  hooks: { on_break_start: "", on_break_end: "" },
  launch_at_login: true, theme: "system", onboarded: true, language: "en", check_updates: true, tray_countdown: true, skipped_update: "",
};
if (scene === "hero") Object.assign(settings, { prebreak_secs: 10, prebreak_visible_secs: 3 });
if (scene === "split") Object.assign(settings, { break_style: "split", texture: "mat_teal" });
if (scene === "gentle") Object.assign(settings, { break_style: "gentle" });
if (scene === "setup") Object.assign(settings, { onboarded: false, theme: "light" });
if (scene === "settings" || scene === "popover") settings.theme = "dark";
if (scene === "settingsLight") settings.theme = "light";
if (scene === "focus") settings.theme = "light";

// 14 days of plausible demo stats
const stats = { days: {} };
(() => {
  let seed = 7;
  const rnd = () => ((seed = (seed * 9301 + 49297) % 233280) / 233280);
  for (let i = 13; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    const weekend = d.getDay() === 0 || d.getDay() === 6;
    const screen = Math.round((weekend ? 1.2 : 5.5 + rnd() * 2.5) * 3600);
    const taken = Math.round(screen / 1500 * (0.7 + rnd() * 0.25));
    stats.days[key] = { screen_secs: i === 0 ? 4 * 3600 + 1260 : screen, breaks_taken: i === 0 ? 11 : taken, breaks_skipped: Math.round(rnd() * 3), natural_breaks: Math.round(rnd() * 4), reminders_shown: 20, reminders_done: 12, longest_stretch_secs: 1500 + Math.round(rnd() * 900) };
  }
})();

const MESSAGE = settings.messages[0];
const snapWorking = (next) => ({
  phase: "working", label: next > 0 ? `Next break in ${Math.floor(next / 60)}:${String(next % 60).padStart(2, "0")}` : "Break starting soon",
  work_secs: Math.max(0, 1200 - next), interval_secs: 1200, next_break_in: next, next_is_long: false, prebreak_secs: settings.prebreak_secs,
  paused_until: null, reason: null, break: null, can_skip: true, skip_in: 0, can_end: false, skip_difficulty: settings.skip_difficulty,
});
const snapBreak = (el) => ({
  ...snapWorking(0), phase: "break", label: "On a break",
  break: { kind: "short", title: "Time for a break", icon: "", message: MESSAGE, total_secs: 20, elapsed_secs: el },
  can_skip: el >= 5, skip_in: Math.max(0, 5 - el), can_end: el >= 14,
});

let state = snapWorking(872);
const listeners = [];
const queue = [];
const overlays = document.getElementById("overlays");
const cursorEl = document.getElementById("cursor");
let cursor = { x: 700, y: 520 };

const TEASER = (window.TEASER = {
  settings, shot: null, done: false,
  on(ev, cb, win) { listeners.push({ ev, cb, win }); },
  emit(ev, payload) {
    for (const l of listeners.slice()) {
      if (l.ev !== ev) continue;
      if (!l.win.frameElement) { listeners.splice(listeners.indexOf(l), 1); continue; }
      try { l.cb({ payload }); } catch (e) { console.warn(e); }
    }
  },
  invoke(cmd, args, win) {
    switch (cmd) {
      case "get_settings": return JSON.parse(JSON.stringify(settings));
      case "get_state": return state;
      case "get_stats": return stats;
      case "get_screenshot": return TEASER.shot;
      case "get_background": return null;
      case "take_reminders": return queue.splice(0);
      case "platform_name": return "windows";
      case "app_version": return "1.0.0";
      case "get_update": return null;
      case "action": return stageAction(args.name);
      case "fit_window": return fit(win, args);
      default: return null;
    }
  },
  prepShot() { cursorEl.classList.add("hidden"); },
  afterShot() { cursorEl.classList.remove("hidden"); },
  run: () => SCENES[scene]().then(() => (TEASER.done = true)),
});

window.LOG = []; const T0 = performance.now();
function mark(m) { LOG.push(((performance.now() - T0) / 1000).toFixed(2) + " " + m); }
function setState(s) { state = s; mark(s.phase + " " + s.next_break_in + (s.break ? " el" + s.break.elapsed_secs : "")); TEASER.emit("state", s); }

function frameFor(win) { return [...overlays.querySelectorAll("iframe")].find((f) => f.contentWindow === win); }
function fit(win, { width, height }) {
  const f = frameFor(win);
  if (!f) return null;
  if (height <= 0) { f.remove(); return null; }
  f.style.width = width + "px";
  f.style.height = height + "px";
  if (f.dataset.role === "popover") {
    // anchored above the tray icon, like the real window (16px of the frame is shadow room)
    const tray = document.querySelector("#taskbar .yd").getBoundingClientRect();
    f.style.left = Math.round(Math.min(W - width - 4, tray.left + tray.width / 2 - width / 2)) + "px";
    f.style.top = Math.round(tray.top - 8 - height + 8) + "px";
  } else if (f.dataset.role !== "cursor") {
    f.style.left = Math.round((W - width) / 2) + "px";
    f.style.top = "48px";
  }
  return null;
}

function openFrame(role, src, css) {
  const f = document.createElement("iframe");
  f.dataset.role = role;
  f.setAttribute("allowtransparency", "true");
  Object.assign(f.style, css);
  f.src = src;
  overlays.appendChild(f);
  return f;
}

// --------------------------------------------------------------- cursor
function placeCursor() {
  cursorEl.style.transform = `translate(${cursor.x}px, ${cursor.y}px)`;
  const pill = overlays.querySelector('iframe[data-role="cursor"]');
  if (pill) { pill.style.left = cursor.x + 18 + "px"; pill.style.top = cursor.y + 22 + "px"; }
}
function moveCursor(x, y, ms = 700) {
  const from = { ...cursor };
  const t0 = performance.now();
  return new Promise((res) => {
    const step = () => {
      const now = performance.now();
      const p = Math.min(1, (now - t0) / ms);
      const e = p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2; // easeInOutCubic
      cursor.x = from.x + (x - from.x) * e;
      cursor.y = from.y + (y - from.y) * e;
      placeCursor();
      p < 1 ? setTimeout(step, 16) : res();
    };
    requestAnimationFrame(step);
  });
}
function clickRipple() {
  const r = document.getElementById("ripple");
  r.style.left = cursor.x + 3 + "px";
  r.style.top = cursor.y + 3 + "px";
  r.animate([{ opacity: 0.9, transform: "scale(.4)" }, { opacity: 0, transform: "scale(1.3)" }], { duration: 450, easing: "ease-out" });
  cursorEl.animate([{ transform: `translate(${cursor.x}px, ${cursor.y}px) scale(1)` }, { transform: `translate(${cursor.x}px, ${cursor.y}px) scale(.86)` }, { transform: `translate(${cursor.x}px, ${cursor.y}px) scale(1)` }], { duration: 220 });
}
/** Moves the fake cursor onto an element inside an iframe and clicks it. */
async function clickIn(frame, selector, ms = 650) {
  const el = frame.contentDocument.querySelector(selector);
  if (!el) { console.warn("missing", selector); return; }
  const fr = frame.getBoundingClientRect();
  const r = el.getBoundingClientRect();
  await moveCursor(fr.left + r.left + Math.min(r.width / 2, 60), fr.top + r.top + r.height / 2, ms);
  await sleep(120);
  clickRipple();
  el.click();
  await sleep(200);
}
placeCursor();

// Focus session as the Rust core would report it (25 min periods, 4 per session).
let focusTicker = null;
function stageAction(name) {
  if (name === "start_focus") {
    let left = 25 * 60 - 12 * 60 - 30; // demo: already 12:30 into the second period
    const push = () => setState({ ...state, focus: { cycle: 2, total: 4, phase: "work", secs_left: left--, long: false } });
    push();
    focusTicker = setInterval(push, 1000);
  } else if (name === "stop_focus") {
    clearInterval(focusTicker);
    setState({ ...state, focus: null });
  }
  return null;
}

// --------------------------------------------------------------- scenes
async function runBreak(stepMs) {
  setState(snapBreak(0));
  const f = openFrame("break", "/app/break.html?primary=1", { left: "0", top: "0", width: W + "px", height: H + "px" });
  await sleep(900);
  for (let el = 1; el <= 20; el++) {
    await sleep(stepMs);
    setState(snapBreak(el));
  }
  setState(snapWorking(1200));
  TEASER.emit("break-ending", 1500);
  await sleep(1550);
  f.remove();
}

const SCENES = {
  async hero() {
    await sleep(500);
    const wander = moveCursor(760, 470, 2600);
    let next = 10;
    setState(snapWorking(next));
    openFrame("prebreak", "/app/prebreak.html", { left: (W - 400) / 2 + "px", top: "48px", width: "400px", height: "140px" });
    await sleep(450);
    while (next > 4) {
      await sleep(430);
      setState(snapWorking(--next));
    }
    await wander;
    // final seconds: the countdown pill trails the cursor
    openFrame("cursor", "/app/cursor.html", { left: cursor.x + 18 + "px", top: cursor.y + 22 + "px", width: "210px", height: "48px" });
    const drift = moveCursor(980, 600, 2300);
    while (next > 1) {
      await sleep(650);
      setState(snapWorking(--next));
    }
    await sleep(650);
    await drift;
    overlays.querySelectorAll('iframe[data-role="prebreak"], iframe[data-role="cursor"]').forEach((f) => f.remove());
    await runBreak(170);
    await moveCursor(700, 520, 700);
    await sleep(300);
  },

  async reminders() {
    await sleep(400);
    queue.push(settings.wellness.posture);
    openFrame("reminders", "/app/reminders.html", { left: (W - 380) / 2 + "px", top: "48px", width: "380px", height: "120px" });
    await sleep(4300);
    overlays.querySelectorAll('iframe[data-role="reminders"]').forEach((f) => f.remove());
    queue.push(settings.wellness.blink);
    openFrame("reminders", "/app/reminders.html", { left: (W - 380) / 2 + "px", top: "48px", width: "380px", height: "120px" });
    await sleep(4300);
    overlays.querySelectorAll('iframe[data-role="reminders"]').forEach((f) => f.remove());
    await sleep(300);
  },

  async popover() {
    // click the tray icon: the popover opens above it with the live countdown and the score of today
    let next = 872;
    const ticker = setInterval(() => setState(snapWorking(--next)), 1000);
    setState(snapWorking(next));
    await sleep(500);
    const tray = document.querySelector("#taskbar .yd").getBoundingClientRect();
    await moveCursor(tray.left + tray.width / 2 - 4, tray.top + tray.height / 2 - 4, 900);
    await sleep(150);
    clickRipple();
    const f = openFrame("popover", "/app/popover.html", { left: "1300px", top: "600px", width: "332px", height: "420px", colorScheme: "dark" });
    await sleep(2600);
    await clickIn(f, "#pause", 700);
    await sleep(1800);
    // click elsewhere: it closes, like a blur
    await moveCursor(700, 420, 700);
    await sleep(150);
    clickRipple();
    f.remove();
    await sleep(600);
    clearInterval(ticker);
  },

  async gentle() {
    // the screen dims, and the cursor keeps working underneath (clicks pass through)
    await sleep(500);
    const work = (async () => {
      await sleep(1400);
      const spots = [[520, 330], [880, 300], [640, 640], [1060, 560], [760, 420]];
      for (const [x, y] of spots) {
        await moveCursor(x, y, 650);
        await sleep(120);
        clickRipple();
        await sleep(380);
      }
    })();
    await runBreak(230);
    await work;
    await moveCursor(700, 520, 600);
    await sleep(400);
  },

  async setup() {
    const f = settingsFrame;
    await sleep(1400);
    await clickIn(f, "#setupNext", 650);
    await sleep(900);
    await clickIn(f, '#setupModes button[data-v="eye_care"]', 600);
    await sleep(500);
    await clickIn(f, '#setupModes button[data-v="balanced"]', 500);
    await sleep(600);
    await clickIn(f, "#setupNext", 550);
    await sleep(900);
    await clickIn(f, '#setupStyles button[data-v="gentle"]', 600);
    await sleep(700);
    await clickIn(f, '#setupStyles button[data-v="fullscreen"]', 500);
    await sleep(600);
    await clickIn(f, "#setupNext", 600);
    await sleep(1100);
    await moveCursor(700, 520, 600);
    await sleep(400);
  },

  async split() {
    await sleep(500);
    await runBreak(230);
    await sleep(500);
  },

  async settings() {
    const f = settingsFrame;
    let next = 872;
    const ticker = setInterval(() => setState(snapWorking(--next)), 1000);
    await sleep(1200);
    await clickIn(f, '#nav a[data-page="breaks"]', 600);
    await sleep(700);
    await clickIn(f, '#enforce button[data-v="casual"]', 550);
    await sleep(350);
    await clickIn(f, '#enforce button[data-v="balanced"]', 450);
    await sleep(350);
    await clickIn(f, '[data-go="long"]', 550);
    await sleep(550);
    await clickIn(f, '#page-long input[data-bind="long_enabled"]', 500);
    await sleep(700);
    await clickIn(f, "#back", 500);
    await sleep(350);
    await clickIn(f, '#nav a[data-page="screen"]', 550);
    await sleep(650);
    await clickIn(f, '#bgSwatches [data-bg="aurora"]', 500);
    await sleep(550);
    await clickIn(f, '#bgSwatches [data-bg="blur"]', 400);
    await sleep(450);
    await clickIn(f, '#styleCards button[data-v="gentle"]', 500);
    await sleep(600);
    await clickIn(f, '#styleCards button[data-v="fullscreen"]', 450);
    await sleep(450);
    await clickIn(f, '#nav a[data-page="today"]', 600);
    await moveCursor(700, 520, 650);
    await sleep(500);
    clearInterval(ticker);
  },

  // Today page: start a focus session, then show the Focus page with the lengths.
  async focus() {
    const f = settingsFrame;
    await sleep(900);
    await clickIn(f, "#homeFocus", 800);
    await sleep(1200);
    await clickIn(f, '#nav a[data-page="focus"]', 700);
    await sleep(1800);
    await clickIn(f, '#nav a[data-page="today"]', 700);
    await moveCursor(900, 560, 600);
    await sleep(1200);
  },

  async settingsLight() {
    const f = settingsFrame;
    cursorEl.classList.add("hidden");
    await sleep(300);
    f.contentDocument.querySelector('#nav a[data-page="breaks"]').click();
    await sleep(700);
  },
};

// Settings scenes: open the window before recording so frame 0 is fully rendered.
let settingsFrame = null;
if (scene === "settings" || scene === "settingsLight" || scene === "setup" || scene === "focus") {
  if (scene !== "settingsLight") document.body.classList.add("dim");
  settingsFrame = openFrame("settings", "/app/index.html", { left: "310px", top: "130px", width: "980px", height: "690px" });
  settingsFrame.classList.add("settings");
}
window.STAGE_READY = true;
