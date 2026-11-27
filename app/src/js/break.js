import { invoke, listen, action, applyTheme } from "./api.js";
import { playSound } from "./sounds.js";
import { animate, GRADIENTS, TEXTURES } from "./backgrounds.js";
import { icon, hydrateIcons } from "./icons.js";
import { createExercises, wanted as exercisesWanted } from "./exercises.js";
import { initI18n, applyI18n, t, tr, locale } from "./i18n.js";

const $ = (id) => document.getElementById(id);
const primary = new URLSearchParams(location.search).get("primary") === "1";
let settings;
let stopAnim = () => {};
let ended = false;
let ready = false; // render only once the language is loaded

const HOLD_MS = 2000; // Firm mode: how long Skip must be held
let firm = false;

const SUBTITLES = {
  short: "Rest your eyes on something far away until the countdown ends.",
  long: "Stand up, stretch, refill your water. Your work will wait.",
  custom: "Take the time you need.",
  planned: "A break you planned for yourself. Enjoy it.",
  focus: "Stand up, stretch, and rest your eyes. Your focus will be back soon.",
};

const prettyAccel = (a) => {
  const mac = /mac/i.test(navigator.platform);
  return a.replace("CmdOrCtrl", mac ? "⌘" : "Ctrl").replace("Alt", mac ? "⌥" : "Alt").replace("Shift", mac ? "⇧" : "Shift").replaceAll("+", mac ? " " : "+");
};

// ------------------------------------------------------------ odometer clock
const timeEl = $("time");
function setTime(secs) {
  secs = Math.max(0, secs);
  const m = Math.floor(secs / 60), s = secs % 60;
  const text = `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  if (timeEl.childElementCount !== text.length) {
    timeEl.innerHTML = [...text].map((ch) =>
      ch === ":" ? `<span class="sep">:</span>` :
      `<span class="col"><span class="reel">${[...Array(10).keys()].map((n) => `<span>${n}</span>`).join("")}</span></span>`
    ).join("");
  }
  [...text].forEach((ch, i) => {
    if (ch === ":") return;
    timeEl.children[i].firstChild.style.transform = `translateY(${-Number(ch) * 1.15}em)`;
  });
}

function tickClock() {
  $("clock").innerHTML = icon("schedule", 14) + new Date().toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
}

// ------------------------------------------------------------------ setup
async function init() {
  settings = await invoke("get_settings");
  applyTheme(settings.theme);
  await initI18n(settings.language);
  applyI18n(document);
  ready = true;
  hydrateIcons();
  if (!primary) document.body.classList.add("secondary");
  await applyCustomCss();
  tickClock();
  setInterval(tickClock, 10000);

  const gentle = settings.break_style === "gentle";
  const split = settings.break_style === "split";
  const wantsShot = !gentle && (split || settings.background === "blur");
  const shot = wantsShot ? await invoke("get_screenshot") : null;

  if (gentle) {
    document.body.classList.add("gentle");
    $("veil").hidden = false;
  } else if (shot && split) {
    document.body.classList.add("split");
    $("split").hidden = false;
    $("texture").style.cssText = (TEXTURES[settings.texture] || TEXTURES.linen).css;
    document.querySelectorAll(".half").forEach((h) => (h.style.backgroundImage = `url(${shot})`));
  } else if (shot) {
    // the snapshot, over a stretched copy of itself that fills the off-screen margin (see break.css)
    $("blur").style.backgroundImage = `url(${shot}), url(${shot})`;
    $("blur").hidden = false;
  } else {
    $("backdrop").hidden = false;
    await setupBackdrop();
  }

  render(await invoke("get_state"));
  // wait for the screenshot to decode so the first frame matches the desktop exactly
  if (shot) await new Promise((r) => { const i = new Image(); i.onload = i.onerror = r; i.src = shot; });
  document.body.classList.remove("loading");
  requestAnimationFrame(() => requestAnimationFrame(() => document.body.classList.add("open")));
  if (primary && settings.sound_enabled) playSound(settings.sound, settings.volume * 0.8);
  if (userCss) cssWatchStart = setTimeout(watchCustomCss, 2500); // once the controls have faded in
}

// ------------------------------------------------------- custom CSS theme
// The user's stylesheet (Settings → Break Screen → Custom CSS) goes in last, followed by
// a guard that keeps the Skip / End / Lock buttons shown and clickable. Esc is handled in
// JS below, so no stylesheet can take it away. If a theme still hides or covers the
// buttons (higher-specificity !important rules, an overlay, opacity 0...), it is
// removed for the rest of this break.
// #foot stays fixed (its built-in position) on the top layer, so nothing in the page can be
// stacked over it; the built-in CSS doesn't use pseudo-elements on html/body/#content/#foot,
// so overlays drawn with them are switched off.
const G = "body:not(.secondary):not(.gentle)";
const GUARD_CSS = `
html:root, html:root > body { filter: none !important; clip-path: none !important; -webkit-mask: none !important; mask: none !important; }
html::before, html::after, body::before, body::after, #content::before, #content::after,
#foot::before, #foot::after, #foot .actions::before, #foot .actions::after,
#foot .pill::after, #foot .pill:not(.hold-fill)::before { content: none !important; display: none !important; }
${G} #foot { display: flex !important; position: fixed !important; z-index: 2147483647 !important; }
${G} #foot, ${G} #foot .actions, ${G} #foot .pill:not([hidden]) {
  visibility: visible !important; pointer-events: auto !important; content-visibility: visible !important;
  filter: none !important; clip-path: none !important; -webkit-mask: none !important; mask: none !important; mix-blend-mode: normal !important; }
${G} #foot .actions { display: flex !important; }
body.open${G.slice(4)} #foot, body.open${G.slice(4)} #foot .actions { opacity: 1 !important; }
${G} #foot .pill:not([hidden]) { display: inline-flex !important; }
${G} #foot .pill .ic, ${G} #foot .pill .lbl { visibility: visible !important; opacity: 1 !important; filter: none !important; }
${G} #foot .pill.hold-fill::before { background: rgba(255, 255, 255, .42) !important; }`;
let userCss = null;
let cssWatch = null;
let cssWatchStart = null;

async function applyCustomCss() {
  if (!settings.break_css) return;
  let css = null;
  try { css = await invoke("get_break_css"); } catch {}
  if (!css) return;
  userCss = [Object.assign(document.createElement("style"), { id: "custom-css", textContent: css }),
    Object.assign(document.createElement("style"), { id: "custom-css-guard", textContent: GUARD_CSS })];
  document.head.append(...userCss);
}

/** Alpha of a computed CSS color ("rgb(...)", "rgba(...)", "transparent"...). */
function alphaOf(color) {
  if (!color || color === "transparent") return 0;
  const m = color.match(/rgba?\(([^)]+)\)/);
  if (!m) return 1; // a format we don't parse: assume visible
  const parts = m[1].split(/[\s,/]+/).filter(Boolean);
  return parts.length > 3 ? parseFloat(parts[3]) : 1;
}

/** True when every shown button can be seen and clicked. */
function controlsUsable() {
  for (const b of document.querySelectorAll("#foot .pill")) {
    if (b.hidden) continue;
    const cs = getComputedStyle(b);
    if (cs.display === "none" || cs.visibility !== "visible") return false;
    const r = b.getBoundingClientRect();
    if (r.width < 12 || r.height < 12) return false;
    const x = r.left + r.width / 2, y = r.top + r.height / 2;
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return false;
    // null means "can't tell" (e.g. mid-layout); overlap by the hint or exercise card is built in
    const hit = document.elementFromPoint(x, y);
    if (hit && !b.contains(hit) && !hit.closest("#foot, .ex")) return false;
    let opacity = 1;
    for (let n = b; n && n.nodeType === 1; n = n.parentElement) {
      const s = getComputedStyle(n);
      opacity *= parseFloat(s.opacity) || 0;
      // filter: opacity(...) fades just like opacity does (the guard removes it, but a
      // more specific !important rule could put it back)
      for (const m of s.filter.matchAll(/opacity\(\s*([\d.]+)(%?)\s*\)/g)) opacity *= parseFloat(m[1]) / (m[2] ? 100 : 1);
    }
    if (opacity < 0.2) return false;
    // invisible ink: transparent text (and icon, which uses currentColor) on a clear button
    const ink = Math.min(alphaOf(cs.color), alphaOf(cs.webkitTextFillColor || cs.color));
    if (ink < 0.3 && alphaOf(cs.backgroundColor) < 0.1 && (alphaOf(cs.borderTopColor) < 0.1 || parseFloat(cs.borderTopWidth) === 0)) return false;
  }
  return true;
}

function watchCustomCss() {
  // only the primary display has buttons; gentle breaks have none (skip from the tray)
  if (ended || !userCss || !primary || settings.break_style === "gentle") return;
  let failures = 0;
  const check = () => {
    if (ended || !document.body.classList.contains("open")) return;
    if (controlsUsable()) return void (failures = 0);
    if (++failures < 3) return; // a few in a row, so a passing transition doesn't count
    console.warn("custom CSS hid the break controls; using the built-in look");
    userCss.forEach((s) => s.remove());
    userCss = null;
    clearInterval(cssWatch);
  };
  check();
  cssWatch = setInterval(check, 1000);
}

async function setupBackdrop() {
  const bg = settings.background;
  if (bg === "image" && settings.has_image) {
    const url = await invoke("get_background");
    if (url) {
      $("canvas").style.display = "none";
      $("image").style.backgroundImage = `url(${url})`;
      $("image").classList.add("on");
      return;
    }
  }
  if (bg === "gradient") {
    $("canvas").style.display = "none";
    $("backdrop").style.background = GRADIENTS[settings.gradient] || GRADIENTS.sunset;
    return;
  }
  stopAnim = animate($("canvas"), ["aurora", "nebula", "ripple"].includes(bg) ? bg : "aurora");
}

// ----------------------------------------------------------------- render
let lastKind, exercises = null;
function render(s) {
  const b = s.break;
  if (!b || ended || !ready) return;
  setTime(b.total_secs - b.elapsed_secs);
  $("bar").style.transform = `scaleX(${Math.min(1, (b.elapsed_secs + 1) / b.total_secs)})`;

  if (lastKind !== b.kind + b.message) {
    lastKind = b.kind + b.message;
    const kindLabel = { long: "Long break", planned: b.title, focus: b.title, custom: "Break time", short: "Short break" }[b.kind] || "Break";
    $("kind").innerHTML = (b.icon ? icon(b.icon, 15) : "") + `<span></span>`;
    $("kind").lastChild.textContent = b.kind === "planned" ? tr(kindLabel) : t(kindLabel);
    $("title").textContent = tr(b.message);
    $("sub").textContent = t(SUBTITLES[b.kind] || SUBTITLES.short);
    exercises = exercisesWanted(settings, b.kind)
      ? createExercises($("ex"), { onCue: () => { if (primary && settings.sound_enabled) playSound("soft", settings.volume * 0.5); } })
      : null;
    $("ex").hidden = !exercises;
    document.body.classList.toggle("has-ex", !!exercises);
  }
  exercises?.update(b.elapsed_secs, b.total_secs);

  const skip = $("skip"), end = $("end"), hint = $("hint");
  end.hidden = !s.can_end;
  skip.hidden = s.can_end || s.skip_difficulty === "hardcore";
  if (skip.hidden) firm = false;
  if (!skip.hidden) {
    skip.disabled = !s.can_skip;
    firm = s.skip_difficulty === "firm" && s.can_skip;
    skip.classList.toggle("hold-fill", firm);
    if (!firm) stopHold();
    skip.querySelector(".lbl").textContent = firm ? t("Hold to skip") : s.can_skip ? t("Skip break") : t("Skip in {n}s", { n: s.skip_in });
  }
  if (settings.break_style === "gentle") {
    const acc = settings.shortcuts?.skip_break;
    if (acc) hint.innerHTML = t("Skip from the tray or press {keys}", { keys: `<kbd>${prettyAccel(acc)}</kbd>` });
    else hint.textContent = t("Skip from the tray menu");
  } else if (s.can_end) hint.innerHTML = t("Press <kbd>Esc</kbd> to end the break");
  else if (firm) hint.innerHTML = t("Hold <kbd>Esc</kbd> to skip");
  else if (s.can_skip) hint.innerHTML = t("Press <kbd>Esc</kbd> twice to skip");
  else if (s.skip_difficulty === "hardcore") hint.textContent = t("Hardcore mode. No skipping — you've got this.");
  else hint.textContent = "";
}

function finish(delay) {
  if (ended) return;
  ended = true;
  clearTimeout(cssWatchStart);
  clearInterval(cssWatch);
  document.body.classList.remove("open");
  document.body.classList.add("closing");
  if (primary && settings?.sound_enabled && delay > 1000) playSound("soft", settings.volume * 0.7);
  // After the desktop has sharpened back (blur/split) or the backdrop faded,
  // cross-fade the whole window into the real desktop: no visible jump.
  setTimeout(() => document.body.classList.add("gone"), Math.max(0, delay - 420));
  setTimeout(stopAnim, delay);
}

// ---------------------------------------------------------------- actions
// Firm mode: the button (or Esc) must be held for HOLD_MS; a fill shows the progress.
let holdTimer = null;
function startHold() {
  if (holdTimer || !firm || ended) return;
  const skip = $("skip");
  skip.style.setProperty("--hold-ms", HOLD_MS + "ms");
  skip.classList.add("holding");
  holdTimer = setTimeout(() => {
    holdTimer = null;
    $("skip").classList.remove("holding");
    action("skip_held");
  }, HOLD_MS);
}
function stopHold() {
  clearTimeout(holdTimer);
  holdTimer = null;
  $("skip").classList.remove("holding");
}
$("skip").onclick = () => { if (!firm) action("end_break"); };
$("skip").addEventListener("pointerdown", (e) => { if (e.button === 0) startHold(); });
for (const ev of ["pointerup", "pointercancel", "pointerleave", "blur"]) $("skip").addEventListener(ev, stopHold);
$("skip").addEventListener("keydown", (e) => { if (e.key === " " || e.key === "Enter") { e.preventDefault(); startHold(); } });
$("skip").addEventListener("keyup", stopHold);
$("end").onclick = () => action("end_break");
$("lock").onclick = () => action("lock");
let lastEsc = 0;
addEventListener("keyup", (e) => { if (e.key === "Escape") stopHold(); });
addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (firm && $("end").hidden) return startHold(); // hold Esc to skip
  const now = Date.now();
  // End early: one press. Skip: two presses within 1.5 s, so it's never an accident.
  if (!$("end").hidden || now - lastEsc < 1500) action("end_break");
  lastEsc = now;
});

listen("state", render);
listen("break-ending", finish);
init();
