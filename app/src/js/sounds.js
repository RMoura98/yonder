// All sounds are synthesized with Web Audio — no audio files to ship.
export const SOUNDS = [
  { id: "chime", name: "Chime" },
  { id: "bowl", name: "Singing bowl" },
  { id: "bell", name: "Bell" },
  { id: "drop", name: "Water drop" },
  { id: "soft", name: "Soft pad" },
  { id: "marimba", name: "Marimba" },
  { id: "pluck", name: "Pluck" },
  { id: "breeze", name: "Breeze" },
  { id: "pop", name: "Pop" },
  { id: "twinkle", name: "Twinkle" },
  { id: "gong", name: "Soft gong" },
  { id: "rain", name: "Rain" },
  { id: "kalimba", name: "Kalimba" },
  { id: "windchimes", name: "Wind chimes" },
  { id: "harp", name: "Harp" },
];

let ctx;
function ac() {
  ctx ??= new (window.AudioContext || window.webkitAudioContext)();
  if (ctx.state === "suspended") ctx.resume();
  return ctx;
}

function tone(c, out, { freq, type = "sine", start = 0, dur = 1.2, gain = 0.3, attack = 0.01, detune = 0, slide = 0 }) {
  const t0 = c.currentTime + start;
  const o = c.createOscillator();
  const g = c.createGain();
  o.type = type;
  o.frequency.setValueAtTime(freq, t0);
  if (slide) o.frequency.exponentialRampToValueAtTime(freq * slide, t0 + dur);
  o.detune.value = detune;
  g.gain.setValueAtTime(0.0001, t0);
  g.gain.exponentialRampToValueAtTime(gain, t0 + attack);
  g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
  o.connect(g).connect(out);
  o.start(t0);
  o.stop(t0 + dur + 0.05);
}

function noise(c, out, { start = 0, dur = 1.5, gain = 0.15, freq = 800 }) {
  const t0 = c.currentTime + start;
  const buf = c.createBuffer(1, c.sampleRate * dur, c.sampleRate);
  const d = buf.getChannelData(0);
  for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  const src = c.createBufferSource();
  src.buffer = buf;
  const f = c.createBiquadFilter();
  f.type = "bandpass";
  f.frequency.setValueAtTime(freq, t0);
  f.frequency.linearRampToValueAtTime(freq * 2, t0 + dur / 2);
  f.frequency.linearRampToValueAtTime(freq, t0 + dur);
  const g = c.createGain();
  g.gain.setValueAtTime(0.0001, t0);
  g.gain.linearRampToValueAtTime(gain, t0 + dur / 2);
  g.gain.linearRampToValueAtTime(0.0001, t0 + dur);
  src.connect(f).connect(g).connect(out);
  src.start(t0);
}

const RECIPES = {
  chime: (c, o) => {
    [659.25, 987.77, 1318.5].forEach((f, i) => tone(c, o, { freq: f, start: i * 0.12, dur: 1.8, gain: 0.22 }));
  },
  bowl: (c, o) => {
    tone(c, o, { freq: 220, dur: 4, gain: 0.3, attack: 0.05 });
    tone(c, o, { freq: 221.5, dur: 4, gain: 0.2, attack: 0.05 });
    tone(c, o, { freq: 594, dur: 3, gain: 0.08, attack: 0.05 });
  },
  bell: (c, o) => {
    tone(c, o, { freq: 880, dur: 2.2, gain: 0.25 });
    tone(c, o, { freq: 2200, dur: 1.2, gain: 0.06 });
    tone(c, o, { freq: 1320, start: 0.4, dur: 2, gain: 0.18 });
  },
  drop: (c, o) => {
    tone(c, o, { freq: 1400, dur: 0.25, gain: 0.3, slide: 0.45 });
    tone(c, o, { freq: 900, start: 0.18, dur: 0.3, gain: 0.15, slide: 0.5 });
  },
  soft: (c, o) => {
    [261.6, 329.6, 392].forEach((f) => tone(c, o, { freq: f, dur: 2.2, gain: 0.12, attack: 0.4, type: "triangle" }));
  },
  marimba: (c, o) => {
    [523.25, 659.25, 783.99, 1046.5].forEach((f, i) => tone(c, o, { freq: f, start: i * 0.09, dur: 0.5, gain: 0.25 }));
  },
  pluck: (c, o) => {
    tone(c, o, { freq: 392, dur: 0.8, gain: 0.3, type: "triangle" });
    tone(c, o, { freq: 587.3, start: 0.15, dur: 0.9, gain: 0.25, type: "triangle" });
  },
  breeze: (c, o) => noise(c, o, { dur: 2.2, gain: 0.25, freq: 600 }),
  pop: (c, o) => tone(c, o, { freq: 600, dur: 0.12, gain: 0.35, slide: 1.8 }),
  twinkle: (c, o) => {
    [1568, 2093, 1760, 2349].forEach((f, i) => tone(c, o, { freq: f, start: i * 0.08, dur: 0.6, gain: 0.12 }));
  },
  gong: (c, o) => {
    tone(c, o, { freq: 110, dur: 2.4, gain: 0.28, attack: 0.04 });
    tone(c, o, { freq: 164, dur: 2.2, gain: 0.14, attack: 0.05 });
    tone(c, o, { freq: 277, dur: 1.8, gain: 0.08, attack: 0.05, detune: 6 });
    tone(c, o, { freq: 410, dur: 1.2, gain: 0.04, attack: 0.05 });
  },
  rain: (c, o) => {
    [[1250, 0, 0.9], [1750, 0.11, 0.65], [980, 0.23, 1], [1500, 0.34, 0.6], [2000, 0.47, 0.5], [1100, 0.58, 0.8],
      [1650, 0.72, 0.55], [880, 0.85, 0.9], [1400, 1.0, 0.5], [1850, 1.15, 0.4]].forEach(([f, t, v]) =>
      tone(c, o, { freq: f, start: t, dur: 0.22, gain: 0.14 * v, attack: 0.006, slide: 0.6 }));
  },
  kalimba: (c, o) => {
    [[523.25, 0], [659.25, 0.16], [783.99, 0.32], [1046.5, 0.5]].forEach(([f, t]) => {
      tone(c, o, { freq: f, start: t, dur: 0.9, gain: 0.22, attack: 0.008 });
      tone(c, o, { freq: f * 4.1, start: t, dur: 0.15, gain: 0.04, attack: 0.008 });
    });
  },
  windchimes: (c, o) => {
    [[1318.5, 0], [1760, 0.2], [1567.98, 0.38], [2093, 0.6], [1396.9, 0.85], [1975.5, 1.05]].forEach(([f, t]) =>
      tone(c, o, { freq: f, start: t, dur: 1.3, gain: 0.1, attack: 0.01 }));
  },
  harp: (c, o) => {
    [261.63, 329.63, 392, 523.25, 659.25, 783.99].forEach((f, i) => {
      tone(c, o, { freq: f, start: i * 0.1, dur: 1.4, gain: 0.17, attack: 0.012, type: "triangle" });
      tone(c, o, { freq: f * 2, start: i * 0.1, dur: 0.6, gain: 0.04, attack: 0.012 });
    });
  },
};

// The user's own sounds ("file:<id>") are stored by the app as data URLs.
const customCache = new Map();
let current; // the sound file currently playing, so previews don't pile up
async function playFile(id, volume) {
  try {
    if (!customCache.has(id)) {
      const url = await window.__TAURI__.core.invoke("get_custom_sound", { id });
      if (!url) return;
      customCache.set(id, url);
    }
    current?.pause();
    current = new Audio(customCache.get(id));
    current.volume = Math.max(0, Math.min(1, volume));
    await current.play();
  } catch (e) {
    console.warn("custom sound failed", e);
  }
}

export function playSound(id, volume = 0.6) {
  if (id?.startsWith("file:")) return void playFile(id.slice(5), volume);
  if (!id || id === "none" || !RECIPES[id]) return;
  try {
    const c = ac();
    const master = c.createGain();
    master.gain.value = Math.max(0, Math.min(1, volume));
    master.connect(c.destination);
    RECIPES[id](c, master);
  } catch (e) {
    console.warn("sound failed", e);
  }
}
