import { invoke, listen, action, applyTheme } from "./api.js";
import { playSound } from "./sounds.js";
import { animate, GRADIENTS, TEXTURES } from "./backgrounds.js";
import { icon, hydrateIcons } from "./icons.js";
import { initI18n, applyI18n, t, tr, locale } from "./i18n.js";

const $ = (id) => document.getElementById(id);
const primary = new URLSearchParams(location.search).get("primary") === "1";
let settings;
let stopAnim = () => {};
let ended = false;
let ready = false; // render only once the language is loaded

const SUBTITLES = {
  short: "Rest your eyes on something far away until the countdown ends.",
  long: "Stand up, stretch, refill your water. Your work will wait.",
  custom: "Take the time you need.",
  planned: "A break you planned for yourself. Enjoy it.",
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
  tickClock();
  setInterval(tickClock, 10000);

  const split = settings.break_style === "split";
  const wantsShot = split || settings.background === "blur";
  const shot = wantsShot ? await invoke("get_screenshot") : null;

  if (shot && split) {
    document.body.classList.add("split");
    $("split").hidden = false;
    $("texture").style.cssText = (TEXTURES[settings.texture] || TEXTURES.linen).css;
    document.querySelectorAll(".half").forEach((h) => (h.style.backgroundImage = `url(${shot})`));
  } else if (shot) {
    $("blur").style.backgroundImage = `url(${shot})`;
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
let lastKind;
function render(s) {
  const b = s.break;
  if (!b || ended || !ready) return;
  setTime(b.total_secs - b.elapsed_secs);
  $("bar").style.transform = `scaleX(${Math.min(1, (b.elapsed_secs + 1) / b.total_secs)})`;

  if (lastKind !== b.kind + b.message) {
    lastKind = b.kind + b.message;
    const kindLabel = { long: "Long break", planned: b.title, custom: "Break time", short: "Short break" }[b.kind] || "Break";
    $("kind").innerHTML = (b.icon ? icon(b.icon, 15) : "") + `<span></span>`;
    $("kind").lastChild.textContent = b.kind === "planned" ? tr(kindLabel) : t(kindLabel);
    $("title").textContent = tr(b.message);
    $("sub").textContent = t(SUBTITLES[b.kind] || SUBTITLES.short);
  }

  const skip = $("skip"), end = $("end"), hint = $("hint");
  end.hidden = !s.can_end;
  skip.hidden = s.can_end || s.skip_difficulty === "hardcore";
  if (!skip.hidden) {
    skip.disabled = !s.can_skip;
    skip.querySelector(".lbl").textContent = s.can_skip ? t("Skip break") : t("Skip in {n}s", { n: s.skip_in });
  }
  if (s.can_end) hint.innerHTML = t("Press <kbd>Esc</kbd> to end the break");
  else if (s.can_skip) hint.innerHTML = t("Press <kbd>Esc</kbd> twice to skip");
  else if (s.skip_difficulty === "hardcore") hint.textContent = t("Hardcore mode. No skipping — you've got this.");
  else hint.textContent = "";
}

function finish(delay) {
  if (ended) return;
  ended = true;
  document.body.classList.remove("open");
  document.body.classList.add("closing");
  if (primary && settings?.sound_enabled && delay > 1000) playSound("soft", settings.volume * 0.7);
  // After the desktop has sharpened back (blur/split) or the backdrop faded,
  // cross-fade the whole window into the real desktop: no visible jump.
  setTimeout(() => document.body.classList.add("gone"), Math.max(0, delay - 420));
  setTimeout(stopAnim, delay);
}

// ---------------------------------------------------------------- actions
$("skip").onclick = () => action("end_break");
$("end").onclick = () => action("end_break");
$("lock").onclick = () => action("lock");
let lastEsc = 0;
addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  const now = Date.now();
  // End early: one press. Skip: two presses within 1.5 s, so it's never an accident.
  if (!$("end").hidden || now - lastEsc < 1500) action("end_break");
  lastEsc = now;
});

listen("state", render);
listen("break-ending", finish);
init();
