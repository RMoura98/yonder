import { invoke, listen, action, applyTheme, fmtClock, fmtHuman } from "./api.js";
import { SOUNDS, playSound } from "./sounds.js";
import { ANIMATED, GRADIENTS, TEXTURES } from "./backgrounds.js";
import { icon as baseIcon, hydrateIcons, PICKABLE, ICON_PATHS } from "./icons.js";
import { adherence, streaks, weeklySummary, heatmap, toCsv, toMarkdown, dayKey } from "./stats-logic.js";
import { initI18n, applyI18n, t, tr, plural, ordinal, locale, LANGUAGES } from "./i18n.js";

// Prefer a filled glyph when asked for "<name>_fill", falling back to the outline.
const icon = (name, size, cls) =>
  baseIcon(ICON_PATHS[name] ? name : name.replace(/_fill$/, ""), size, cls);

const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];
// Monday-first weekday names in the UI language (2024-01-01 was a Monday).
const weekday = (i, style) => new Intl.DateTimeFormat(locale(), { weekday: style }).format(new Date(2024, 0, 1 + i));
const dayLetter = (i) => weekday(i, "narrow");
const dayName = (i) => weekday(i, "short");
const X = () => icon("close", 16);

// The built-in reminders' own glyphs (as drawn by reminders.html), shown at rest.
const BUILTIN_GLYPH = {
  posture: `<svg class="glyph" viewBox="0 0 50 50" aria-hidden="true"><path class="stroke" d="M16 24 L25 15 L34 24"/><path class="stroke" d="M25 16 L25 36"/></svg>`,
  blink: `<svg class="glyph" viewBox="0 0 50 50" aria-hidden="true"><g fill="#3b1d06"><ellipse cx="18" cy="22" rx="3.6" ry="4.6"/><ellipse cx="32" cy="22" rx="3.6" ry="4.6"/></g><path class="stroke" d="M19 32 Q25 36 31 32" style="stroke-width:3"/></svg>`,
};
const PRESETS = {
  balanced: { work_mins: 20, break_secs: 20, long_enabled: false, long_mins: 5, long_every: 3 },
  deep_focus: { work_mins: 45, break_secs: 30, long_enabled: true, long_mins: 5, long_every: 3 },
  eye_care: { work_mins: 15, break_secs: 15, long_enabled: true, long_mins: 3, long_every: 4 },
  wellness: { work_mins: 25, break_secs: 45, long_enabled: true, long_mins: 5, long_every: 2 },
};
const BREAK_LENGTHS = [10, 15, 20, 30, 45, 60, 90, 120, 180, 300];

let S; // settings
let platform = "windows";

// ------------------------------------------------------------------ helpers
const get = (path) => path.split(".").reduce((o, k) => o?.[k], S);
function set(path, value) {
  const keys = path.split(".");
  const last = keys.pop();
  keys.reduce((o, k) => o[k], S)[last] = value;
}
let saveTimer;
function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try {
      await invoke("save_settings", { settings: S });
    } catch (e) {
      toast(t("Couldn't save: {e}", { e }));
    }
  }, 250);
  refreshVisibility();
  refreshSummaries();
}
function toast(msg) {
  let t = $(".toast");
  if (!t) document.body.appendChild((t = Object.assign(document.createElement("div"), { className: "toast" })));
  t.textContent = msg;
  t.classList.add("on");
  clearTimeout(t._h);
  t._h = setTimeout(() => t.classList.remove("on"), 1800);
}
const el = (html) => {
  const t = document.createElement("template");
  t.innerHTML = html.trim();
  return t.content.firstElementChild;
};
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const uid = () => Math.random().toString(36).slice(2, 9);
const secsLabel = (s) =>
  s < 60 ? t("{n} seconds", { n: s }) : s % 60 ? t("{m} min {s} s", { m: Math.floor(s / 60), s: s % 60 }) : plural(s / 60, "1 minute", "{n} minutes");

/** Fills `[data-tile][data-ic]` elements with their glyph (nav items get a tile + label). */
function paintTiles() {
  for (const a of $$("#nav a")) {
    const label = a.textContent;
    a.innerHTML = `<span class="tile" data-tile="${a.dataset.tile}">${icon(a.dataset.ic + "_fill", 17)}</span><span>${esc(label)}</span>`;
  }
  for (const t of $$("span.tile[data-ic]")) t.innerHTML = icon(t.dataset.ic + "_fill", 17);
}

/** A button showing the current icon; opens a small grid of Material icons. */
function iconPicker(current, onPick) {
  const wrap = el(`<div class="icon-pick"><button type="button" class="icon-btn" title="${esc(t("Choose icon"))}">${icon(current, 18)}</button></div>`);
  const btn = wrap.firstChild;
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    const open = $(".icon-pop", wrap);
    $$(".icon-pop").forEach((p) => p.remove());
    if (open) return;
    const pop = el(`<div class="icon-pop" role="listbox"></div>`);
    for (const n of PICKABLE) {
      const b = el(`<button type="button" title="${n.replaceAll("_", " ")}" class="${n === current ? "on" : ""}">${icon(n, 20)}</button>`);
      b.onclick = (ev) => {
        ev.stopPropagation();
        current = n;
        btn.innerHTML = icon(n, 18);
        onPick(n);
        pop.remove();
      };
      pop.appendChild(b);
    }
    wrap.appendChild(pop);
  });
  return wrap;
}
document.addEventListener("click", () => $$(".icon-pop").forEach((p) => p.remove()));

// ------------------------------------------------------------------ focus
/** Fills the focus length pickers; must run before bindAll() selects their values. */
function initFocus() {
  for (const sel of $$("select[data-mins], select[data-periods]")) {
    const periods = !!sel.dataset.periods;
    const list = (sel.dataset.mins || sel.dataset.periods).split(",").map(Number);
    sel.innerHTML = "";
    for (const n of list) {
      const label = n === 0 ? t("Off") : periods ? t("{n} focus periods", { n }) : n === 1 ? t("1 minute") : t("{n} minutes", { n });
      sel.add(new Option(label, String(n)));
    }
  }
  const toggle = () => action(focusRunning ? "stop_focus" : "start_focus");
  $("#homeFocus").onclick = toggle;
  $("#focusToggle").onclick = toggle;
}

let focusRunning = false;
function renderFocus(f) {
  focusRunning = !!f;
  const label = t(f ? "Stop focus session" : "Start focus session");
  $("#homeFocus").textContent = label;
  $("#focusToggle").textContent = label;
  $("#focusToggle").classList.toggle("primary", !f);
  const line = $("#homeFocusLine");
  line.hidden = !f;
  let text = t("No focus session running");
  if (f) {
    const vars = { n: f.cycle, total: f.total, t: fmtClock(f.secs_left) };
    const key = f.phase === "break" ? (f.long ? "Long focus break · {t} left" : "Focus break {n}/{total} · {t} left") : "Focus {n}/{total} · {t} left";
    text = t(key, vars);
    line.textContent = text;
  }
  $("#focusStatus").textContent = text;
}

// ------------------------------------------------------------------ binding
function bindAll() {
  for (const input of $$("[data-bind]")) {
    const path = input.dataset.bind;
    if (input.classList.contains("seg")) {
      const paint = () => $$("button", input).forEach((b) => b.classList.toggle("on", b.dataset.v === String(get(path))));
      paint();
      $$("button", input).forEach((b) =>
        b.addEventListener("click", () => {
          set(path, b.dataset.v);
          paint();
          if (path === "theme") applyTheme(S.theme);
          save();
        })
      );
      continue;
    }
    const isNum = input.type === "number" || input.type === "range" || input.dataset.type === "number";
    if (input.type === "checkbox") input.checked = !!get(path);
    else {
      // make sure a select can show a value that isn't one of its presets
      if (input.tagName === "SELECT" && get(path) != null && ![...input.options].some((o) => o.value === String(get(path)))) {
        input.add(new Option(String(get(path)), String(get(path))));
      }
      input.value = get(path) ?? "";
    }
    input.addEventListener(input.type === "text" ? "input" : "change", () => {
      let v = input.type === "checkbox" ? input.checked : input.value;
      if (isNum) {
        v = Number(v);
        if (input.min !== "" && v < Number(input.min)) v = Number(input.min);
        if (input.max !== "" && v > Number(input.max)) v = Number(input.max);
        input.value = v;
      }
      set(path, v);
      if (input.hasAttribute("data-custom")) markCustomMode();
      save();
    });
  }
}

function refreshVisibility() {
  for (const n of $$("[data-show]")) {
    const [path, val] = n.dataset.show.split("=");
    const v = get(path);
    n.toggleAttribute("data-hidden", !(val === undefined ? !!v : String(v) === val));
  }
}

// -------------------------------------------------------------------- nav
let current = "today";
function go(page) {
  const sec = $("#page-" + page);
  if (!sec) return;
  current = page;
  const parent = sec.dataset.parent;
  const navKey = parent || page;
  $$("#nav a").forEach((a) => a.classList.toggle("active", a.dataset.page === navKey));
  $$(".page").forEach((p) => p.classList.toggle("active", p === sec));
  const navItem = $(`#nav a[data-page="${navKey}"]`);
  $("#barTitle").textContent = t(sec.dataset.title);
  $("#back").hidden = !parent;
  $("#barTile").hidden = !!parent;
  if (parent) $("#backLabel").textContent = t($("#page-" + parent).dataset.title);
  else {
    $("#barTile").dataset.tile = navItem.dataset.tile;
    $("#barTile").innerHTML = icon(navItem.dataset.ic + "_fill", 20);
  }
  $("#scroll").scrollTop = 0;
  if (page === "stats") renderStats();
}

function initNav() {
  $$("#nav a").forEach((a) => a.addEventListener("click", () => go(a.dataset.page)));
  $$("[data-go]").forEach((a) => a.addEventListener("click", () => go(a.dataset.go)));
  $("#back").addEventListener("click", () => go($("#page-" + current).dataset.parent));
  $$("[data-act]").forEach((b) => b.addEventListener("click", () => action(b.dataset.act)));
  $$("[data-break]").forEach((b) => b.addEventListener("click", () => action("take_break", Number(b.dataset.break))));
}

// external links open in the system browser (attached once, survives re-renders)
document.addEventListener("click", (e) => {
  const a = e.target.closest("a[href^='https://']");
  if (!a) return;
  e.preventDefault();
  invoke("open_url", { url: a.href });
});

// ------------------------------------------------------------ screen breaks
function markCustomMode() {
  const match = Object.entries(PRESETS).find(([, p]) => Object.entries(p).every(([k, v]) => S[k] === v));
  S.mode = match ? match[0] : "custom";
  paintBreaks();
}

function paintBreaks() {
  $("#modeSel").value = S.mode in PRESETS ? S.mode : "custom";
  $("#workH").value = Math.floor(S.work_mins / 60);
  $("#workM").value = S.work_mins % 60;
  const sel = $("#breakSel");
  const lengths = [...new Set([...BREAK_LENGTHS, S.break_secs])].sort((a, b) => a - b);
  sel.innerHTML = lengths.map((s) => `<option value="${s}">${secsLabel(s)}</option>`).join("");
  sel.value = S.break_secs;
  $$("#enforce button").forEach((b) => b.classList.toggle("on", b.dataset.v === S.skip_difficulty));
}

function refreshSummaries() {
  if (!S) return;
  const bgName = S.break_style === "split" ? "Split screen" : S.break_style === "gentle" ? "Gentle" : S.break_style === "notify" ? "Notification" : ({ blur: "Blurred desktop", image: "Your image", gradient: "Gradient" }[S.background] || (ANIMATED.find((a) => a.id === S.background)?.name ?? ""));
  $("#vScreen").textContent = t(bgName);
  $("#vLong").textContent = S.long_enabled ? t("Every {nth} break is a {mins} min long break", { nth: ordinal(S.long_every), mins: S.long_mins }) : t("Off");
  const oh = S.office_hours;
  const days = oh.days.every(Boolean)
    ? t("every day")
    : oh.days.slice(0, 5).every(Boolean) && !oh.days[5] && !oh.days[6]
      ? t("weekdays")
      : oh.days.map((d, i) => (d ? dayName(i) : null)).filter(Boolean).join(", ");
  $("#vHours").textContent = oh.enabled ? t("{from} to {to} {days}", { from: oh.start, to: oh.end, days }) : t("Always on");
  const n = S.planned.filter((p) => p.enabled).length;
  $("#vPlanned").textContent = n ? plural(n, "1 planned", "{n} planned") : t("None");
}

function initBreaks() {
  paintBreaks();
  $("#modeSel").addEventListener("change", (e) => {
    const m = e.target.value;
    if (PRESETS[m]) Object.assign(S, PRESETS[m]);
    S.mode = m;
    // refresh the controls the preset touched
    $$("[data-custom]").forEach((i) => (i.type === "checkbox" ? (i.checked = !!get(i.dataset.bind)) : (i.value = get(i.dataset.bind))));
    paintBreaks();
    save();
  });
  const setWork = () => {
    const h = Math.max(0, Math.min(4, Number($("#workH").value) || 0));
    const m = Math.max(0, Math.min(59, Number($("#workM").value) || 0));
    S.work_mins = Math.max(1, h * 60 + m);
    markCustomMode();
    save();
  };
  $("#workH").addEventListener("change", setWork);
  $("#workM").addEventListener("change", setWork);
  $("#breakSel").addEventListener("change", (e) => {
    S.break_secs = Number(e.target.value);
    markCustomMode();
    save();
  });
  $$("#enforce button").forEach((b) =>
    b.addEventListener("click", () => {
      S.skip_difficulty = b.dataset.v;
      paintBreaks();
      save();
    })
  );
}

// ----------------------------------------------------------- break screen
function soundOptions(sel, withNone) {
  const mine = S.custom_sounds || [];
  sel.classList.add("sound-pick");
  sel.dataset.none = withNone ? "1" : "0";
  sel.innerHTML =
    (withNone ? `<option value="none">${esc(t("None"))}</option>` : "") +
    SOUNDS.map((s) => `<option value="${s.id}">${esc(t(s.name))}</option>`).join("") +
    (mine.length
      ? `<optgroup label="${esc(t("Your sounds"))}">${mine.map((c) => `<option value="file:${esc(c.id)}">${esc(c.name)}</option>`).join("")}</optgroup>`
      : "") +
    `<option value="__add">${esc(t("Add a sound file…"))}</option>`;
}

/** Rebuilds every sound picker after the library changed, keeping each selection. */
function refreshSoundPickers() {
  $$("select.sound-pick").forEach((sel) => {
    const v = sel.value;
    soundOptions(sel, sel.dataset.none === "1");
    sel.value = [...sel.options].some((o) => o.value === v) ? v : sel.dataset.none === "1" ? "none" : "chime";
  });
}

/** Lets the user pick an audio file and adds it to their library. Resolves to the new sound or null. */
function pickSoundFile() {
  return new Promise((resolve) => {
    const input = $("#soundFile");
    input.value = "";
    input.onchange = async () => {
      const f = input.files[0];
      if (!f) return resolve(null);
      if (f.size > 8 * 1024 * 1024) {
        toast(t("That file is too large (max 8 MB)."));
        return resolve(null);
      }
      const dataUrl = await new Promise((r) => {
        const fr = new FileReader();
        fr.onload = () => r(fr.result);
        fr.readAsDataURL(f);
      });
      try {
        const sound = await invoke("add_custom_sound", { name: f.name.replace(/\.[^.]+$/, ""), dataUrl });
        S.custom_sounds = [...(S.custom_sounds || []), sound];
        refreshSoundPickers();
        renderSoundLibrary();
        toast(t("Sound added"));
        resolve(sound);
      } catch (e) {
        toast(t(String(e)));
        resolve(null);
      }
    };
    input.click();
  });
}

function renderSoundLibrary() {
  const list = $("#customSoundList");
  if (!list) return;
  const mine = S.custom_sounds || [];
  list.innerHTML = mine.length ? "" : `<div class="empty">${esc(t("No sounds added yet."))}</div>`;
  for (const c of mine) {
    const row = el(`<div class="item-row"><button class="btn ghost icon-only play" title="${esc(t("Preview"))}">${icon("play_arrow_fill", 18)}</button><div class="grow">${esc(c.name)}</div><button class="x" title="${esc(t("Remove"))}">${X()}</button></div>`);
    $(".play", row).onclick = () => playSound("file:" + c.id, S.volume);
    $(".x", row).onclick = async () => {
      await invoke("remove_custom_sound", { id: c.id });
      // mirror what the app did, so the next save doesn't bring it back
      const key = "file:" + c.id;
      S.custom_sounds = S.custom_sounds.filter((x) => x.id !== c.id);
      if (S.sound === key) S.sound = "chime";
      for (const r of [S.wellness.posture, S.wellness.blink, ...S.wellness.custom]) if (r.sound === key) r.sound = "none";
      refreshSoundPickers();
      renderSoundLibrary();
    };
    list.appendChild(row);
  }
}

// "Add a sound file…" in any picker: open the file dialog instead of selecting it.
// Runs in the capture phase so the picker's own change handler never sees "__add".
document.addEventListener("focusin", (e) => {
  if (e.target instanceof HTMLSelectElement) e.target.dataset.prev = e.target.value;
});
document.addEventListener("change", async (e) => {
  const sel = e.target;
  if (!(sel instanceof HTMLSelectElement) || sel.value !== "__add") return;
  e.stopImmediatePropagation();
  sel.value = sel.dataset.prev ?? (sel.dataset.none === "1" ? "none" : "chime");
  const sound = await pickSoundFile();
  if (sound) {
    sel.value = "file:" + sound.id;
    sel.dataset.prev = sel.value;
    sel.dispatchEvent(new Event("change", { bubbles: true }));
  }
}, true);

function initScreen() {
  // style cards
  const paintStyle = () => $$("#styleCards button").forEach((b) => b.classList.toggle("on", b.dataset.v === S.break_style));
  $$("#styleCards button").forEach((b) =>
    b.addEventListener("click", () => {
      if (b.disabled) return;
      S.break_style = b.dataset.v;
      paintStyle();
      save();
    })
  );
  paintStyle();
  if (platform === "linux") {
    $("#splitBtn").disabled = true;
    $("#splitBtn span:last-child").textContent = t("Not available on Linux yet");
  }

  // backgrounds
  const box = $("#bgSwatches");
  const paint = () =>
    $$(".swatch", box).forEach((s) => {
      const on = s.dataset.bg === S.background && (S.background !== "gradient" || s.dataset.g === S.gradient);
      s.classList.toggle("on", on);
    });
  const preview = {
    aurora: "radial-gradient(circle at 25% 35%,#1f7a7a 0,transparent 55%),radial-gradient(circle at 75% 40%,#f4c26b 0,transparent 50%),radial-gradient(circle at 50% 90%,#2e62a6 0,transparent 60%),#0b1f24",
    nebula: "radial-gradient(circle at 35% 40%,#3d2a6b 0,transparent 50%),radial-gradient(circle at 65% 60%,#1d4e89 0,transparent 50%),#06060f",
    ripple: "repeating-radial-gradient(circle at 50% 50%,rgba(159,216,203,.28) 0 2px,transparent 2px 16px),linear-gradient(135deg,#0e3b43,#12253d)",
  };
  if (platform !== "linux") {
    box.appendChild(el(`<button class="swatch blurprev" data-bg="blur" title="${esc(t("Your desktop, frosted behind the countdown"))}"><span>${esc(t("Blurred desktop"))}</span></button>`));
  }
  for (const a of ANIMATED) box.appendChild(el(`<button class="swatch" data-bg="${a.id}" style="background:${preview[a.id]}"><span>${esc(t(a.name))}</span></button>`));
  for (const [id, g] of Object.entries(GRADIENTS)) {
    box.appendChild(el(`<button class="swatch" data-bg="gradient" data-g="${id}" style="background:${g}"><span>${esc(t(id[0].toUpperCase() + id.slice(1)))}</span></button>`));
  }
  const img = el(`<button class="swatch upload" data-bg="image" title="${esc(t("Click to choose, double-click to replace"))}"><span>${esc(t("Your image"))}</span><b class="plus">${icon("add", 26)}</b></button>`);
  box.appendChild(img);
  if (S.has_image) invoke("get_background").then((u) => { if (u) { img.style.backgroundImage = `url(${u})`; $(".plus", img)?.remove(); } });
  box.addEventListener("click", (e) => {
    const s = e.target.closest(".swatch");
    if (!s) return;
    if (s.dataset.bg === "image" && !S.has_image) return $("#imgInput").click();
    S.background = s.dataset.bg;
    if (s.dataset.g) S.gradient = s.dataset.g;
    paint();
    save();
  });
  img.addEventListener("dblclick", () => $("#imgInput").click());
  $("#imgInput").addEventListener("change", (e) => {
    const f = e.target.files[0];
    if (!f) return;
    const r = new FileReader();
    r.onload = async () => {
      try {
        await invoke("save_background", { dataUrl: r.result });
        S.has_image = true;
        S.background = "image";
        img.style.backgroundImage = `url(${r.result})`;
        $(".plus", img)?.remove();
        paint();
        save();
        toast(t("Background image saved"));
      } catch (err) {
        toast(String(err));
      }
    };
    r.readAsDataURL(f);
  });
  paint();

  // textures
  const tb = $("#texSwatches");
  for (const [id, tex] of Object.entries(TEXTURES)) {
    const s = el(`<button class="swatch" data-t="${id}"><span>${esc(tr(tex.name))}</span></button>`);
    s.style.cssText += tex.css;
    tb.appendChild(s);
  }
  const paintT = () => $$(".swatch", tb).forEach((s) => s.classList.toggle("on", s.dataset.t === S.texture));
  tb.addEventListener("click", (e) => {
    const s = e.target.closest(".swatch");
    if (!s) return;
    S.texture = s.dataset.t;
    paintT();
    save();
  });
  paintT();

  // messages
  const renderMsgs = () => {
    const list = $("#msgList");
    list.innerHTML = S.messages.length ? "" : `<div class="empty">${esc(t("No messages yet. A default one will be used."))}</div>`;
    S.messages.forEach((m, i) => {
      // untouched English defaults show translated; edits are stored as typed
      const row = el(`<div class="item-row"><div class="grow"><input type="text" maxlength="140" value="${esc(tr(m))}" /></div><button class="x" title="${esc(t("Remove"))}">${X()}</button></div>`);
      $("input", row).addEventListener("input", (e) => { S.messages[i] = e.target.value; save(); });
      $(".x", row).onclick = () => { S.messages.splice(i, 1); renderMsgs(); save(); };
      list.appendChild(row);
    });
  };
  const addMsg = () => {
    const v = $("#msgNew").value.trim();
    if (!v) return;
    S.messages.push(v);
    $("#msgNew").value = "";
    renderMsgs();
    save();
  };
  $("#msgAdd").onclick = addMsg;
  $("#msgNew").addEventListener("keydown", (e) => e.key === "Enter" && addMsg());
  renderMsgs();
}

function initSounds() {
  soundOptions($("#soundSel"), false);
  $("#soundSel").value = S.sound;
  $("#soundTest").onclick = () => playSound(S.sound, S.volume);
  $("#addSound").onclick = () => pickSoundFile();
  renderSoundLibrary();
}

// --------------------------------------------------------------- wellness
function reminderCard(r, { builtin, onRemove }) {
  const card = el(`<div class="rem">
    <div class="rem-head">
      ${builtin ? `<span class="icon-badge">${BUILTIN_GLYPH[r.id] || icon(r.icon, 18)}</span>` : `<span class="pick"></span>`}
      ${builtin ? `<div class="title">${esc(t(builtin))}</div>` : `<input type="text" class="title" maxlength="40" value="${esc(tr(r.title))}" placeholder="${esc(t("Title"))}" />`}
      <input type="checkbox" class="switch en" ${r.enabled ? "checked" : ""} />
      ${onRemove ? `<button class="x" title="${esc(t("Delete"))}">${X()}</button>` : ""}
    </div>
    <div class="rem-grid">
      <label>${esc(t("Every"))} <input type="number" class="iv" min="1" max="480" value="${r.interval_mins}" /> ${esc(t("min"))}</label>
      <label>${esc(t("Size"))} <span class="stepper"><select class="sz"><option value="small">${esc(t("Small"))}</option><option value="medium">${esc(t("Medium"))}</option><option value="large">${esc(t("Large"))}</option></select></span></label>
      <label>${esc(t("Sound"))} <span class="stepper"><select class="snd"></select></span></label>
      <label><input type="checkbox" class="switch sticky" ${r.sticky ? "checked" : ""} /> ${esc(t("Stay until done"))}</label>
    </div>
    ${builtin ? "" : `<div style="margin-top:8px"><input type="text" class="body" maxlength="80" style="width:100%" value="${esc(tr(r.body))}" placeholder="${esc(t("Optional description"))}" /></div>`}
  </div>`);
  $(".pick", card)?.replaceWith(iconPicker(r.icon, (n) => { r.icon = n; save(); }));
  soundOptions($(".snd", card), true);
  $(".snd", card).value = r.sound;
  $(".sz", card).value = r.size;
  const on = (sel, ev, fn) => $(sel, card)?.addEventListener(ev, (e) => { fn(e.target); save(); });
  on(".en", "change", (t) => (r.enabled = t.checked));
  on(".iv", "change", (t) => (r.interval_mins = Math.max(1, Number(t.value) || 1)));
  on(".sz", "change", (t) => (r.size = t.value));
  on(".snd", "change", (t) => { r.sound = t.value; playSound(r.sound, S.volume); });
  on(".sticky", "change", (t) => (r.sticky = t.checked));
  on("input.title", "input", (t) => (r.title = t.value));
  on(".body", "input", (t) => (r.body = t.value));
  if (onRemove) $(".x", card).onclick = onRemove;
  return card;
}

function initWellness() {
  const b = $("#builtinReminders");
  b.appendChild(reminderCard(S.wellness.posture, { builtin: "Posture" }));
  b.appendChild(reminderCard(S.wellness.blink, { builtin: "Blink" }));

  const renderCustom = () => {
    const box = $("#customReminders");
    box.innerHTML = "";
    S.wellness.custom.forEach((r, i) =>
      box.appendChild(reminderCard(r, { onRemove: () => { S.wellness.custom.splice(i, 1); renderCustom(); save(); } }))
    );
    $("#customCount").textContent = t("{n} of 5", { n: S.wellness.custom.length });
    $("#addReminder").disabled = S.wellness.custom.length >= 5;
  };
  $("#addReminder").onclick = () => {
    if (S.wellness.custom.length >= 5) return;
    S.wellness.custom.push({
      id: "c_" + uid(), enabled: true, title: "Drink water", body: "", icon: "water_drop",
      interval_mins: 45, size: "medium", sound: "drop", sticky: false,
    });
    renderCustom();
    save();
  };
  renderCustom();
}

// ------------------------------------------------------------------ smart
function initSmart() {
  if (platform === "macos") {
    $("#dndNote").textContent = t("Reads macOS Focus when possible; may need Full Disk Access for Yonder.");
  } else if (platform === "linux") {
    $("#shareNote").textContent = t("Screen recorders like OBS, and PipeWire screen sharing.");
    $("#meetNote").textContent = t("When any app is recording from your microphone.");
    $("#fsNote").textContent = t("Not available on Linux yet. Add specific apps below instead.");
    $("#dndNote").textContent = t("Works with GNOME Do Not Disturb and desktops that inhibit notifications, such as KDE.");
  }
  const render = () => {
    const list = $("#appList");
    list.innerHTML = S.smart.apps.length ? "" : `<div class="empty">${esc(t("No focus apps yet."))}</div>`;
    S.smart.apps.forEach((a, i) => {
      const row = el(`<div class="item-row"><div class="grow">${esc(a)}</div><button class="x" title="${esc(t("Remove"))}">${X()}</button></div>`);
      $(".x", row).onclick = () => { S.smart.apps.splice(i, 1); render(); save(); };
      list.appendChild(row);
    });
  };
  const add = () => {
    const v = $("#appNew").value.trim();
    if (!v || S.smart.apps.includes(v)) return;
    S.smart.apps.push(v);
    $("#appNew").value = "";
    render();
    save();
  };
  $("#appAdd").onclick = add;
  $("#appNew").addEventListener("keydown", (e) => e.key === "Enter" && add());
  render();
}

// --------------------------------------------------------------- schedule
function daysPicker(arr) {
  const box = el(`<div class="days"></div>`);
  [0, 1, 2, 3, 4, 5, 6].forEach((i) => {
    const b = el(`<button class="${arr[i] ? "on" : ""}" title="${esc(dayName(i))}">${esc(dayLetter(i))}</button>`);
    b.onclick = () => { arr[i] = !arr[i]; b.classList.toggle("on", arr[i]); save(); };
    box.appendChild(b);
  });
  return box;
}

function initSchedule() {
  $("#ohDays").replaceWith(daysPicker(S.office_hours.days));
  const render = () => {
    const box = $("#plannedList");
    box.innerHTML = "";
    S.planned.forEach((p, i) => {
      const card = el(`<div class="rem">
        <div class="rem-head">
          <span class="pick"></span>
          <input type="text" class="title" maxlength="40" value="${esc(tr(p.title))}" />
          <input type="checkbox" class="switch en" ${p.enabled ? "checked" : ""} />
          <button class="x" title="${esc(t("Delete"))}">${X()}</button>
        </div>
        <div class="rem-grid">
          <label>${esc(t("At"))} <input type="time" class="tm" value="${esc(p.time)}" /></label>
          <label>${esc(t("For"))} <input type="number" class="du" min="1" max="240" value="${p.duration_mins}" /> ${esc(t("min"))}</label>
          <span class="dp"></span>
        </div></div>`);
      $(".dp", card).replaceWith(daysPicker(p.days));
      $(".pick", card).replaceWith(iconPicker(p.icon, (n) => { p.icon = n; save(); }));
      const on = (sel, ev, fn) => $(sel, card).addEventListener(ev, (e) => { fn(e.target); save(); });
      on(".title", "input", (t) => (p.title = t.value));
      on(".en", "change", (t) => (p.enabled = t.checked));
      on(".tm", "change", (t) => (p.time = t.value));
      on(".du", "change", (t) => (p.duration_mins = Math.max(1, Number(t.value) || 1)));
      $(".x", card).onclick = () => { S.planned.splice(i, 1); render(); save(); };
      box.appendChild(card);
    });
  };
  $("#addPlanned").onclick = () => {
    const first = !S.planned.length;
    S.planned.push({
      id: "p_" + uid(), enabled: true,
      title: first ? "Lunch" : "Afternoon walk", icon: first ? "restaurant" : "directions_walk",
      time: first ? "12:30" : "16:00", duration_mins: first ? 30 : 10,
      days: [true, true, true, true, true, false, false],
    });
    render();
    save();
  };
  render();
}

// -------------------------------------------------------------- shortcuts
function accelFrom(e) {
  const mods = [];
  if (e.ctrlKey || e.metaKey) mods.push("CmdOrCtrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  let k = e.key;
  if (["Control", "Meta", "Alt", "Shift"].includes(k)) return null;
  if (e.code.startsWith("Key")) k = e.code.slice(3);
  else if (e.code.startsWith("Digit")) k = e.code.slice(5);
  else if (k === " ") k = "Space";
  else if (k.length === 1) k = k.toUpperCase();
  if (!mods.length && !/^F\d+$/.test(k)) return null;
  return [...mods, k].join("+");
}
const pretty = (a) => {
  if (!a) return t("Not set");
  const mac = platform === "macos";
  return a.replace("CmdOrCtrl", mac ? "⌘" : "Ctrl").replace("Alt", mac ? "⌥" : "Alt").replace("Shift", mac ? "⇧" : "Shift").replaceAll("+", mac ? " " : " + ");
};

function initShortcuts() {
  for (const input of $$("[data-keys]")) {
    const path = input.dataset.keys;
    input.value = pretty(get(path));
    input.addEventListener("focus", () => (input.value = t("Press keys…")));
    input.addEventListener("blur", () => (input.value = pretty(get(path))));
    input.addEventListener("keydown", (e) => {
      e.preventDefault();
      if (e.key === "Backspace" || e.key === "Delete") { set(path, ""); input.blur(); save(); return; }
      if (e.key === "Escape") return input.blur();
      const a = accelFrom(e);
      if (!a) return;
      set(path, a);
      input.blur();
      save();
      toast(t("Shortcut saved"));
    });
  }
}

// ------------------------------------------------------------------ stats
function score(d) {
  if (!d || !d.screen_secs) return "—";
  const expected = Math.max(1, Math.floor(d.screen_secs / (S.work_mins * 60)));
  const ratio = Math.min(1, ((d.breaks_taken || 0) + (d.natural_breaks || 0)) / expected);
  const skipPenalty = Math.min(0.3, (d.breaks_skipped || 0) * 0.05);
  return Math.round(Math.max(0, ratio - skipPenalty) * 100);
}

async function renderStats() {
  const stats = await invoke("get_stats");
  const days = [];
  for (let i = 13; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    days.push({ date: d, ...(stats.days[dayKey(d)] || {}) });
  }
  const last7 = days.slice(-7);
  const sum = (arr, k) => arr.reduce((a, d) => a + (d[k] || 0), 0);
  const activeDays = last7.filter((d) => d.screen_secs > 0).length || 1;
  $("#sScore").textContent = score(days[13]);
  $("#sAvg").textContent = fmtHuman(sum(last7, "screen_secs") / activeDays);
  $("#sTaken").textContent = sum(last7, "breaks_taken");
  $("#sNatural").textContent = sum(last7, "natural_breaks");

  renderWeek(stats.days);
  renderHeat(stats.days);

  const max = Math.max(3600, ...days.map((d) => d.screen_secs || 0));
  const maxB = Math.max(1, ...days.map((d) => d.breaks_taken || 0));
  const chart = $("#chart");
  chart.innerHTML = "";
  for (const d of days) {
    const tip = [
      d.date.toLocaleDateString(locale(), { weekday: "long", day: "numeric", month: "long" }),
      t("Screen time: {t}", { t: fmtHuman(d.screen_secs || 0) }),
      t("Breaks taken: {n} · skipped: {m}", { n: d.breaks_taken || 0, m: d.breaks_skipped || 0 }),
    ].join("\n");
    chart.appendChild(el(`<div class="bar" title="${esc(tip)}">
      <div class="a" style="height:${((d.screen_secs || 0) / max) * 82}%"></div>
      <div class="b" style="opacity:${0.25 + 0.75 * ((d.breaks_taken || 0) / maxB)}"></div>
      <div class="d">${esc(d.date.toLocaleDateString(locale(), { weekday: "short" }).slice(0, 3))}</div></div>`));
  }
}

const pct = (a) => Math.round((a ?? 0) * 100);
const signed = (n, fmt = String) => (n > 0 ? "+" : n < 0 ? "−" : "") + fmt(Math.abs(n));
const daysLabel = (n) => plural(n, "1 day", "{n} days");

function renderWeek(days) {
  const { thisWeek: a, lastWeek: b } = weeklySummary(days, S.work_mins);
  const hasLast = b.screen_secs > 0;
  const tile = (label, cur, prev, fmt) => {
    const diff = cur - prev;
    const delta = !hasLast ? t("No data last week") : diff === 0 ? t("Same as last week") : t("{d} vs last week", { d: signed(diff, fmt) });
    return `<div class="stat"><div class="l">${esc(label)}</div><div class="v">${esc(fmt(cur))}</div><div class="delta">${esc(delta)}</div></div>`;
  };
  $("#weekTiles").innerHTML =
    tile(t("Breaks taken"), a.breaks_taken, b.breaks_taken, String) +
    tile(t("Breaks skipped"), a.breaks_skipped, b.breaks_skipped, String) +
    tile(t("Screen time"), a.screen_secs, b.screen_secs, fmtHuman) +
    (a.adherence == null
      ? `<div class="stat"><div class="l">${esc(t("Adherence"))}</div><div class="v">—</div><div class="delta"> </div></div>`
      : tile(t("Adherence"), pct(a.adherence), pct(b.adherence), (n) => n + "%"));
  const { current, best } = streaks(days, S.work_mins);
  $("#sStreak").textContent = daysLabel(current);
  $("#sBest").textContent = daysLabel(best);
}

function renderHeat(days) {
  const { weeks } = heatmap(days, S.work_mins);
  const fmt = (o) => new Intl.DateTimeFormat(locale(), o);
  const month = fmt({ month: "short" });
  const full = fmt({ weekday: "long", day: "numeric", month: "long", year: "numeric" });
  const todayKey = dayKey(new Date());
  let html = "";
  let lastMonth = -1;
  let lastCol = -9;
  weeks.forEach((col, w) => {
    const m = col[0].date.getMonth();
    if (m === lastMonth) return;
    lastMonth = m;
    // a month that starts in the first column's last days would collide with the next label
    if (w - lastCol >= 3 && w <= 50 && !(w === 0 && weeks[1][0].date.getMonth() !== m)) {
      html += `<span class="m" style="grid-column:${w + 2}">${esc(month.format(col[0].date))}</span>`;
      lastCol = w;
    }
  });
  // Mon, Wed, Fri labels (2024-01-01 was a Monday)
  const wd = fmt({ weekday: "short" });
  for (const r of [0, 2, 4]) html += `<span class="wd" style="grid-row:${r + 2}" aria-hidden="true">${esc(wd.format(new Date(2024, 0, 1 + r)))}</span>`;
  weeks.forEach((col, w) =>
    col.forEach((c, r) => {
      if (!c) return;
      const d = c.day || {};
      const a = adherence(c.day, S.work_mins);
      const info =
        a == null
          ? t("No activity")
          : [t("Breaks taken: {n} · skipped: {m}", { n: d.breaks_taken || 0, m: d.breaks_skipped || 0 }), t("Adherence: {p}%", { p: pct(a) })].join(" · ");
      const label = full.format(c.date) + ": " + info;
      html += `<i class="cell l${c.level}${c.key === todayKey ? " today" : ""}" role="listitem" style="grid-column:${w + 2};grid-row:${r + 2}" title="${esc(label)}" aria-label="${esc(label)}"></i>`;
    })
  );
  $("#heat").innerHTML = html;
}

function download(name, text, type) {
  const blob = new Blob([text], { type });
  Object.assign(document.createElement("a"), { href: URL.createObjectURL(blob), download: name }).click();
}

async function exportHistory(kind) {
  const stats = await invoke("get_stats");
  const keys = Object.keys(stats.days);
  if (!keys.length) return toast(t("Nothing to export yet"));
  const stamp = dayKey(new Date());
  if (kind === "csv") download(`yonder-history-${stamp}.csv`, toCsv(stats.days), "text/csv");
  else {
    const dfmt = new Intl.DateTimeFormat(locale(), { weekday: "short", year: "numeric", month: "short", day: "numeric" });
    const md = toMarkdown(stats.days, {
      title: t("Yonder break journal"),
      summary: t("Exported on {date}. {n} days of history, newest first.", { date: dfmt.format(new Date()), n: keys.length }),
      headers: [t("Date"), t("Screen time"), t("Breaks taken"), t("Skipped"), t("Natural breaks"), t("Reminders done"), t("Longest stretch")],
      fmtDate: (d) => dfmt.format(d),
      fmtDur: fmtHuman,
    });
    download(`yonder-history-${stamp}.md`, md, "text/markdown");
  }
  toast(t("History exported"));
}

let confirmReset = 0;
function initStats() {
  $("#exportCsv").onclick = () => exportHistory("csv");
  $("#exportMd").onclick = () => exportHistory("md");
  $("#exportStats").onclick = async () => {
    const stats = await invoke("get_stats");
    const blob = new Blob([JSON.stringify(stats, null, 2)], { type: "application/json" });
    Object.assign(document.createElement("a"), { href: URL.createObjectURL(blob), download: "yonder-stats.json" }).click();
  };
  $("#resetStats").onclick = async () => {
    // two clicks within 3 s, instead of a modal dialog
    if (Date.now() - confirmReset > 3000) {
      confirmReset = Date.now();
      $("#resetStats").textContent = t("Click again to delete all stats");
      setTimeout(() => ($("#resetStats").textContent = t("Reset stats")), 3000);
      return;
    }
    await invoke("reset_stats");
    $("#resetStats").textContent = t("Reset stats");
    renderStats();
    refreshToday();
  };
}

// ----------------------------------------------------------------- backup
let pendingImport = null;
let importTimer;
function resetImportButton() {
  pendingImport = null;
  clearTimeout(importTimer);
  const b = $("#importSettings");
  if (b) b.textContent = t("Import settings…");
}

function initBackup() {
  resetImportButton();
  $("#exportSettings").onclick = async () => {
    try {
      const text = await invoke("export_settings");
      const blob = new Blob([text], { type: "application/json" });
      const name = `yonder-settings-${dayKey(new Date())}.yonder.json`;
      Object.assign(document.createElement("a"), { href: URL.createObjectURL(blob), download: name }).click();
      toast(t("Settings exported"));
    } catch (e) {
      toast(tr(String(e)));
    }
  };
  const importBtn = $("#importSettings");
  importBtn.onclick = async () => {
    if (!pendingImport) return $("#importFile").click();
    // second click within 10 s confirms (no modal dialog)
    const text = pendingImport;
    resetImportButton();
    try {
      S = await invoke("import_settings", { text });
      applyTheme(S.theme);
      await relocalize(); // rebuild every page from the imported settings
      toast(t("Settings imported"));
    } catch (e) {
      toast(tr(String(e)));
    }
  };
  $("#importFile").onchange = async (e) => {
    const f = e.target.files[0];
    e.target.value = "";
    if (!f) return;
    try {
      const text = await f.text();
      await invoke("check_backup", { text });
      pendingImport = text;
      importBtn.textContent = t("Click again to replace your settings");
      toast(t("Backup file looks good. Click Import again within 10 seconds to replace your current settings."));
      clearTimeout(importTimer);
      importTimer = setTimeout(resetImportButton, 10000);
    } catch (err) {
      toast(tr(String(err)));
    }
  };
}

// ------------------------------------------------------------------ today
async function refreshToday() {
  const stats = await invoke("get_stats");
  const d = stats.days[dayKey(new Date())] || {};
  $("#tScreen").textContent = fmtHuman(d.screen_secs || 0);
  $("#tTaken").textContent = d.breaks_taken || 0;
  $("#tSkipped").textContent = d.breaks_skipped || 0;
  $("#tStretch").textContent = fmtHuman(d.longest_stretch_secs || 0);
}

const PHASES = { working: "Working", break: "On a break", paused: "Paused", smart: "Holding breaks", offhours: "Off the clock", away: "Away" };
function renderState(s) {
  const left = Math.max(0, s.next_break_in);
  const ring = $("#homeRing");
  if (s.phase === "break" && s.break) {
    $("#homeCount").textContent = fmtClock(s.break.total_secs - s.break.elapsed_secs);
    $("#homeSub").textContent = t("of break left");
    ring.style.strokeDashoffset = String(326.7 * (s.break.elapsed_secs / s.break.total_secs));
  } else {
    $("#homeCount").textContent = fmtClock(left);
    $("#homeSub").textContent = t(s.next_is_long ? "until long break" : "until next break");
    ring.style.strokeDashoffset = String(326.7 * Math.min(1, s.work_secs / s.interval_secs));
  }
  renderFocus(s.focus);
  $("#homePhase").textContent = t(PHASES[s.phase] || "Working");
  $("#homeLabel").textContent = s.label;
  $("#homeSkip").disabled = s.skip_difficulty === "hardcore";
  const pause = $("#homePause");
  pause.textContent = t(s.phase === "paused" ? "Resume" : "Pause 30 min");
  pause.onclick = () => (s.phase === "paused" ? action("resume") : action("pause", 30));
  const side = $("#sideStatus");
  side.textContent = s.label;
  side.className = "status " + (s.phase === "paused" || s.phase === "offhours" ? "paused" : s.phase === "break" ? "break" : "");
}

// ---------------------------------------------------------------- updates
function renderUpdate(u) {
  $("#updateBanner").hidden = !u;
  $("#sideUpdate").hidden = !u;
  if (!u) return;
  $("#updTitle").textContent = t("Yonder {v} is available", { v: "v" + u.version });
  // our own trusted markup (contains a <code> tag)
  $("#updHint").innerHTML =
    platform === "macos"
      ? t("Installed with Homebrew? Run <code>brew upgrade --cask yonder</code>. Otherwise download the new .dmg.")
      : esc(t("Download the new installer from the release page."));
  $("#updGet").onclick = () => invoke("open_url", { url: u.url });
  $("#updSkip").onclick = async () => {
    await invoke("skip_update", { version: u.version });
    $("#updStatus").textContent = t("Skipped v{v}. You'll hear about the next one.", { v: u.version });
  };
  $("#updStatus").textContent = t("Version {v} is available.", { v: u.version });
}

let updateListener; // attached once; renderUpdate always works on the current markup
async function initUpdates() {
  const version = await invoke("app_version");
  $("#aboutVersion").textContent = `${t("Version {v}", { v: version })} · ${t("Look up. Look far. Get back to it.")}`;
  renderUpdate(await invoke("get_update"));
  if (!updateListener) updateListener = listen("update-status", (u) => renderUpdate(u));
  $("#openLogs").onclick = () => invoke("open_logs_folder");
  $("#updCheck").onclick = async () => {
    const btn = $("#updCheck");
    btn.disabled = true;
    $("#updStatus").textContent = t("Checking…");
    try {
      const u = await invoke("check_updates_now");
      renderUpdate(u);
      const time = new Date().toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
      if (!u) $("#updStatus").textContent = t("You're on the latest version (checked {time}).", { time });
    } catch (e) {
      $("#updStatus").textContent = tr(String(e));
    } finally {
      btn.disabled = false;
    }
  };
}

// --------------------------------------------------------- window controls
function initWindowControls() {
  if (platform === "macos") {
    document.body.classList.add("mac"); // native traffic lights float over the sidebar
    return;
  }
  const win = window.__TAURI__.window.getCurrentWindow();
  $("#winctl").hidden = false;
  $("#wMin").onclick = () => win.minimize();
  $("#wMax").onclick = () => win.toggleMaximize();
  $("#wClose").onclick = () => win.close();
}

// --------------------------------------------------------------- language
function initLanguage() {
  const sel = $("#langSel");
  sel.innerHTML =
    `<option value="system">${esc(t("System default"))}</option>` +
    LANGUAGES.map((l) => `<option value="${l.code}">${esc(l.name)}</option>`).join("");
  sel.value = S.language || "system";
  sel.addEventListener("change", async () => {
    S.language = sel.value;
    clearTimeout(saveTimer);
    await invoke("save_settings", { settings: S }); // also re-labels the tray
    await relocalize();
  });
  // ordinal option labels for "Every Nth break"
  $$("select[data-bind='long_every'] option").forEach((o) => (o.textContent = t("{nth} break", { nth: ordinal(Number(o.value)) })));
}

// ------------------------------------------------------------ first-run setup
let setupStep = 1;
const SETUP_MODES = [
  ["balanced", "Balanced", "The classic 20-20-20 rhythm."],
  ["deep_focus", "Deep focus", "Longer stretches, fewer interruptions."],
  ["eye_care", "Eye care", "Short, frequent rests for tired eyes."],
  ["wellness", "Wellness", "Longer breaks and more long breaks."],
];

function initSetup() {
  const root = $("#setup");
  root.hidden = S.onboarded;
  if (S.onboarded) return;

  // language
  const sel = $("#setupLang");
  sel.innerHTML =
    `<option value="system">${esc(t("System default"))}</option>` + LANGUAGES.map((l) => `<option value="${l.code}">${esc(l.name)}</option>`).join("");
  sel.value = S.language || "system";
  sel.addEventListener("change", async () => {
    S.language = sel.value;
    clearTimeout(saveTimer);
    await invoke("save_settings", { settings: S });
    await relocalize(); // rebuilds this overlay and stays on the same step
  });

  // timing modes
  const modes = $("#setupModes");
  modes.innerHTML = SETUP_MODES.map(([id, name, why]) => {
    const p = PRESETS[id];
    return `<button data-v="${id}"><b>${esc(t(name))}</b><span class="when">${esc(t("{mins} min · {secs} s break", { mins: p.work_mins, secs: p.break_secs }))}</span><span class="why">${esc(t(why))}</span></button>`;
  }).join("");
  const paintModes = () => $$("button", modes).forEach((b) => b.classList.toggle("on", b.dataset.v === S.mode));
  $$("button", modes).forEach((b) =>
    b.addEventListener("click", () => {
      Object.assign(S, PRESETS[b.dataset.v]);
      S.mode = b.dataset.v;
      paintModes();
      save();
    })
  );
  paintModes();

  // skip difficulty and break style reuse the real pages' cards (previews included)
  const enforce = $("#setupEnforce");
  enforce.innerHTML = $("#enforce").innerHTML;
  enforce.style.gridTemplateColumns = `repeat(${enforce.children.length}, 1fr)`;
  const paintEnf = () => $$("button", enforce).forEach((b) => b.classList.toggle("on", b.dataset.v === S.skip_difficulty));
  $$("button", enforce).forEach((b) => b.addEventListener("click", () => { S.skip_difficulty = b.dataset.v; paintEnf(); save(); }));
  paintEnf();
  const styles = $("#setupStyles");
  styles.innerHTML = $("#styleCards").innerHTML.replace(/ id="[^"]*"/g, "");
  styles.style.gridTemplateColumns = `repeat(${styles.children.length}, 1fr)`;
  const paintStyles = () => $$("button", styles).forEach((b) => b.classList.toggle("on", b.dataset.v === S.break_style));
  $$("button", styles).forEach((b) =>
    b.addEventListener("click", () => {
      if (b.disabled) return;
      S.break_style = b.dataset.v;
      paintStyles();
      save();
    })
  );
  paintStyles();

  // navigation
  const finish = () => {
    S.onboarded = true;
    save();
    root.hidden = true;
    go("today");
  };
  const show = (n) => {
    setupStep = n;
    $$(".setup-step", root).forEach((s) => (s.hidden = Number(s.dataset.step) !== n));
    $$("#setupDots i").forEach((d, i) => d.classList.toggle("on", i === n - 1));
    $("#setupBack").hidden = n === 1;
    $("#setupNext").textContent = t(n === 3 ? "Get started" : "Next");
    root.scrollTop = 0;
  };
  $("#setupBack").onclick = () => show(setupStep - 1);
  $("#setupNext").onclick = () => (setupStep === 3 ? finish() : show(setupStep + 1));
  $("#setupSkip").onclick = finish;
  show(setupStep);
}

// ------------------------------------------------------------------- init
// An untouched copy of the window's markup, taken before anything is rendered,
// so a language change can rebuild every page in place instead of reloading.
const PRISTINE = document.querySelector(".app").cloneNode(true);

/** Re-renders the whole window in the current language, staying on the same page. */
async function relocalize() {
  const page = current;
  const scrollTop = $("#scroll").scrollTop;
  await initI18n(S.language);
  const app = document.querySelector(".app");
  const fresh = PRISTINE.cloneNode(true);
  fresh.style.opacity = "0.55";
  app.replaceWith(fresh);
  renderAll();
  go(page);
  $("#scroll").scrollTop = scrollTop;
  renderState(await invoke("get_state"));
  refreshToday();
  fresh.animate([{ opacity: 0.55 }, { opacity: 1 }], { duration: 180, easing: "ease-out" }).finished.then(() => (fresh.style.opacity = ""));
}

/** Everything that builds or wires up the window's DOM (safe to run again on fresh markup). */
function renderAll() {
  // Translate the static markup before anything dynamic is rendered
  // (dynamic content is built with t() directly).
  applyI18n(document);
  initLanguage();
  initWindowControls();
  paintTiles();
  hydrateIcons();
  initNav();
  initFocus();
  bindAll();
  initBreaks();
  initScreen();
  initSounds();
  initWellness();
  initSmart();
  initSchedule();
  initShortcuts();
  initStats();
  initBackup();
  initUpdates();
  initSetup();
  refreshVisibility();
  refreshSummaries();
}

/** One-time startup: load settings, then render; global listeners are attached only here. */
async function init() {
  [S, platform] = await Promise.all([invoke("get_settings"), invoke("platform_name")]);
  applyTheme(S.theme);
  await initI18n(S.language);
  renderAll();
  if (platform === "linux") {
    const note = document.getElementById("pauseMediaNote");
    if (note) note.textContent = t("Needs playerctl. Resumes afterwards, only what Yonder paused.");
  }
  // Windows can't draw text beside a tray icon, so it gets the countdown drawn into the icon instead.
  if (platform === "windows") {
    $("#trayTitleRow").hidden = true;
    $("#trayIconRow").hidden = false;
  }
  go("today");
  renderState(await invoke("get_state"));
  refreshToday();
  setInterval(refreshToday, 30000);
  listen("state", (s) => renderState(s));
}
init();
