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
};

export function playSound(id, volume = 0.6) {
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
