// Guided exercises for the break screen: a routine of short steps, sized to the
// length of the break, each with a calm animated illustration (CSS/SVG only).
//
// Exercise ideas (the general set: eye movements, near/far focus, blinking,
// palming, neck and shoulder stretches) are drawn from the exercise lists of
// Workrave (GPL-3.0) and Safe Eyes (GPL-3.0), and from Stretchly's break ideas
// (BSD-2-Clause). Only the concepts are shared: all wording, illustrations and
// code here are Yonder's own.
import { t } from "./i18n.js";

// id -> [title, instruction]; the English text is the i18n key.
const COPY = {
  blink: ["Blink slowly", "Let your eyelids close gently, pause, then open. Repeat at an easy pace."],
  dotH: ["Left and right", "Keep your head still and let your eyes follow the dot from side to side."],
  dotV: ["Up and down", "Keep your head still and let your eyes follow the dot up and down."],
  eight: ["Draw a figure eight", "Follow the dot as it traces a slow figure eight on its side."],
  nearfar: ["Near and far", "Focus on something close, then on something far away. Switch with the dot."],
  neck: ["Neck stretch", "Tip one ear toward your shoulder, pause, then the other side. Keep your shoulders loose."],
  shoulders: ["Shoulder rolls", "Lift your shoulders, then roll them back and down in slow circles."],
  palming: ["Cover your eyes", "Rub your palms together to warm them, then rest them lightly over your closed eyes."],
};

// Eyes only for short stretches of time; stretches join once there is room.
const EYES = ["blink", "dotH", "nearfar", "dotV", "eight", "palming"];
const FULL = ["blink", "dotH", "dotV", "eight", "nearfar", "neck", "shoulders", "palming"];

/** The steps for a break of `total` seconds: [{ id, start, end, idx, count }]. */
export function plan(total) {
  const pool = total < 120 ? EYES : FULL;
  const minStep = total < 40 ? 10 : 20;
  const count = Math.max(1, Math.min(pool.length, Math.floor(total / minStep)));
  const round = pool.slice(0, count);
  // very long breaks repeat the routine instead of stretching each step out
  const rounds = Math.max(1, Math.round(total / count / 45));
  const n = count * rounds;
  return Array.from({ length: n }, (_, i) => ({
    id: round[i % count],
    start: Math.round((i * total) / n),
    end: Math.round(((i + 1) * total) / n),
    idx: i % count,
    count,
  }));
}

/** Does this break kind get the routine under the user's settings? */
export function wanted(settings, kind) {
  if (settings.break_style === "gentle" || settings.break_style === "notify") return false;
  return kind === "short" ? !!settings.exercises_short : !!settings.exercises;
}

// ------------------------------------------------------------------ visuals
const EIGHT_W = 130, EIGHT_H = 42, EIGHT_P = 12; // half-width, half-height, seconds per loop
const eightPath = (() => {
  const pts = [];
  for (let i = 0; i <= 72; i++) {
    const a = (i / 72) * Math.PI * 2;
    pts.push(`${(200 - EIGHT_W * Math.cos(a)).toFixed(1)},${(75 + EIGHT_H * Math.sin(2 * a)).toFixed(1)}`);
  }
  return pts.join(" ");
})();

const ARROW = (d) => `<path d="${d}" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>`;
const HEAD = (cls = "") => `<g class="head ${cls}"><circle cx="200" cy="52" r="25"/><path d="M200 77v22"/></g>`;
const TORSO = `<path class="torso" d="M118 150c4-30 28-44 82-44s78 14 82 44"/>`;

const VISUAL = {
  blink: () => `
    <div class="eyes">
      <svg viewBox="0 0 140 76" class="eye">
        <defs><clipPath id="exEyeClip"><path d="M6 38Q70-8 134 38Q70 84 6 38Z"/></clipPath></defs>
        <path d="M6 38Q70-8 134 38Q70 84 6 38Z" fill="#EEF4F2"/>
        <g clip-path="url(#exEyeClip)">
          <circle cx="70" cy="38" r="19" fill="#1F7A7A"/><circle cx="70" cy="38" r="9" fill="#0E3B43"/><circle cx="64" cy="31" r="4" fill="#fff" opacity=".85"/>
          <rect class="lid" x="0" y="-4" width="140" height="84" fill="#2C8F8A"/>
        </g>
        <path d="M6 38Q70-8 134 38" fill="none" stroke="#9FD8CB" stroke-width="3" stroke-linecap="round"/>
        <path d="M6 38Q70 84 134 38" fill="none" stroke="#9FD8CB" stroke-width="3" stroke-linecap="round" opacity=".55"/>
      </svg>
      <svg viewBox="0 0 140 76" class="eye shut static-only"><path d="M12 28Q70 62 128 28" fill="none" stroke="#9FD8CB" stroke-width="4" stroke-linecap="round"/><path d="M34 46l-8 10M70 54v12M106 46l8 10" stroke="#9FD8CB" stroke-width="3" stroke-linecap="round" opacity=".7"/></svg>
    </div>`,
  dotH: () => `
    <div class="track h"></div><i class="cap l"></i><i class="cap r"></i>
    <svg class="static-only hint" viewBox="0 0 400 150">${ARROW("M70 75h-26m9-9l-9 9 9 9")}${ARROW("M330 75h26m-9-9l9 9-9 9")}</svg>
    <div class="move-x"><div class="dot"></div></div>`,
  dotV: () => `
    <div class="track v"></div><i class="cap t"></i><i class="cap b"></i>
    <svg class="static-only hint" viewBox="0 0 400 150">${ARROW("M200 30v-14m-8 8l8-8 8 8")}${ARROW("M200 120v14m-8-8l8 8 8-8")}</svg>
    <div class="move-y"><div class="dot"></div></div>`,
  eight: () => `
    <svg class="guide" viewBox="0 0 400 150"><polyline points="${eightPath}" fill="none" stroke="rgba(255,255,255,.28)" stroke-width="2" stroke-linejoin="round" stroke-dasharray="1 7" stroke-linecap="round"/></svg>
    <div class="eight-x"><div class="eight-y"><div class="dot"></div></div></div>`,
  nearfar: () => `
    <div class="horizon"></div>
    <div class="nf"><div class="ring"></div><div class="dot"></div></div>
    <div class="nf-label"></div>`,
  neck: () => `
    <svg viewBox="0 0 400 150" class="fig">${TORSO}
      <g class="ghosts static-only"><g transform="rotate(-20 200 100)"><circle cx="200" cy="52" r="25"/></g><g transform="rotate(20 200 100)"><circle cx="200" cy="52" r="25"/></g></g>
      ${HEAD("tilt")}</svg>`,
  shoulders: () => `
    <svg viewBox="0 0 400 150" class="fig">${TORSO}${HEAD()}
      <g class="roll l"><circle class="arc" cx="130" cy="124" r="17"/><g class="rx"><g class="ry"><circle class="ball" cx="130" cy="124" r="9"/></g></g></g>
      <g class="roll r"><circle class="arc" cx="270" cy="124" r="17"/><g class="rx"><g class="ry"><circle class="ball" cx="270" cy="124" r="9"/></g></g></g>
    </svg>`,
  palming: () => `
    <div class="shade"></div>
    <svg viewBox="0 0 400 150" class="palms">
      <path class="lidline" d="M156 78Q200 98 244 78" fill="none" stroke="#9FD8CB" stroke-width="3.5" stroke-linecap="round"/>
      <g class="palm pl"><ellipse cx="148" cy="76" rx="52" ry="38"/><path d="M110 62q16-6 26 4M112 80q18-4 30 6" fill="none"/></g>
      <g class="palm pr"><ellipse cx="252" cy="76" rx="52" ry="38"/><path d="M290 62q-16-6-26 4M288 80q-18-4-30 6" fill="none"/></g>
    </svg>`,
};

// -------------------------------------------------------------------- panel
/** Wires the routine into `root` (the #ex section). Call update() on every state tick. */
export function createExercises(root, { onCue } = {}) {
  root.innerHTML = `
    <div class="ex-stage" data-step=""></div>
    <div class="ex-copy"><div class="ex-count"></div><h2 class="ex-name"></h2><p class="ex-text"></p></div>
    <div class="ex-dots"></div>`;
  const stage = root.querySelector(".ex-stage");
  const [count, name, text, dots] = [".ex-count", ".ex-name", ".ex-text", ".ex-dots"].map((s) => root.querySelector(s));
  let steps = [], total = 0, cur = -1, lastId = null;

  function show(i) {
    const s = steps[i];
    const first = cur < 0;
    if (!first && lastId === "palming" && s.id !== "palming") onCue?.();
    cur = i; lastId = s.id;
    const [ttl, txt] = COPY[s.id];
    root.classList.remove("swap");
    void root.offsetWidth; // restart the cross-fade
    root.classList.add("swap");
    stage.dataset.step = s.id;
    stage.innerHTML = VISUAL[s.id]();
    count.textContent = t("{a} of {b}", { a: s.idx + 1, b: s.count });
    name.textContent = t(ttl);
    text.textContent = t(txt);
    if (s.count <= 12) {
      dots.hidden = false;
      dots.innerHTML = Array.from({ length: s.count }, (_, k) => `<i class="${k < s.idx ? "done" : k === s.idx ? "on" : ""}"></i>`).join("");
    } else dots.hidden = true;
  }

  function update(elapsed, totalSecs) {
    if (totalSecs !== total) { total = totalSecs; steps = plan(total); cur = -1; }
    let i = steps.findIndex((s) => elapsed >= s.start && elapsed < s.end);
    if (i < 0) i = elapsed >= total ? steps.length - 1 : 0;
    if (i !== cur) show(i);
    if (steps[i].id === "nearfar") {
      // alternate every 6 seconds: close, far, close...
      const far = Math.floor((elapsed - steps[i].start) / 6) % 2 === 1;
      stage.classList.toggle("far", far);
      const label = stage.querySelector(".nf-label");
      const want = t(far ? "Look far" : "Look near");
      if (label && label.textContent !== want) label.textContent = want;
    }
  }
  return { update };
}
