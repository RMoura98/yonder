// Break-screen backdrops: animated canvases, gradients, and split-screen textures.

export const ANIMATED = [
  { id: "aurora", name: "Aurora" },
  { id: "nebula", name: "Nebula" },
  { id: "ripple", name: "Ripple" },
];

export const GRADIENTS = {
  dusk: "linear-gradient(180deg,#0e3b43 0%,#1f7a7a 55%,#f4c26b 100%)",
  sunset: "linear-gradient(135deg,#ff9a6b 0%,#e8577a 55%,#6c3fa8 100%)",
  ocean: "linear-gradient(160deg,#0f2a4a 0%,#1f6f8b 50%,#5ee6c9 100%)",
  forest: "linear-gradient(150deg,#0e2a1f 0%,#2f6b4a 55%,#b8d98a 100%)",
  twilight: "linear-gradient(180deg,#141432 0%,#3b2a6b 50%,#d77a9c 100%)",
  peach: "linear-gradient(135deg,#ffe3c7 0%,#ffb59a 50%,#f2879a 100%)",
  graphite: "linear-gradient(160deg,#16161d 0%,#2b2b36 60%,#45455a 100%)",
};

export const TEXTURES = {
  linen: {
    name: "Linen",
    css: `background-color:#d9cfbf;background-image:
      repeating-linear-gradient(0deg,rgba(0,0,0,.05) 0 1px,transparent 1px 3px),
      repeating-linear-gradient(90deg,rgba(255,255,255,.12) 0 1px,transparent 1px 3px);`,
  },
  felt_green: {
    name: "Felt — green",
    css: `background-color:#2e5b45;background-image:radial-gradient(rgba(255,255,255,.05) 1px,transparent 1px);background-size:3px 3px;`,
  },
  felt_navy: {
    name: "Felt — navy",
    css: `background-color:#1f2b4a;background-image:radial-gradient(rgba(255,255,255,.05) 1px,transparent 1px);background-size:3px 3px;`,
  },
  cork: {
    name: "Cork",
    css: `background-color:#b88a5a;background-image:
      radial-gradient(rgba(60,30,10,.35) 1px,transparent 1.5px),
      radial-gradient(rgba(255,230,190,.25) 1px,transparent 1.5px);
      background-size:7px 7px,11px 11px;background-position:0 0,3px 5px;`,
  },
  mat_teal: {
    name: "Cutting mat — teal",
    css: `background-color:#1d6b66;background-image:
      linear-gradient(rgba(255,255,255,.18) 1px,transparent 1px),
      linear-gradient(90deg,rgba(255,255,255,.18) 1px,transparent 1px),
      linear-gradient(rgba(255,255,255,.07) 1px,transparent 1px),
      linear-gradient(90deg,rgba(255,255,255,.07) 1px,transparent 1px);
      background-size:100px 100px,100px 100px,20px 20px,20px 20px;`,
  },
  mat_coral: {
    name: "Cutting mat — coral",
    css: `background-color:#d9614c;background-image:
      linear-gradient(rgba(255,255,255,.2) 1px,transparent 1px),
      linear-gradient(90deg,rgba(255,255,255,.2) 1px,transparent 1px),
      linear-gradient(rgba(255,255,255,.08) 1px,transparent 1px),
      linear-gradient(90deg,rgba(255,255,255,.08) 1px,transparent 1px);
      background-size:100px 100px,100px 100px,20px 20px,20px 20px;`,
  },
  steel: {
    name: "Brushed steel",
    css: `background-color:#a7adb5;background-image:
      repeating-linear-gradient(90deg,rgba(255,255,255,.12) 0 1px,rgba(0,0,0,.04) 1px 2px,transparent 2px 4px),
      linear-gradient(180deg,#c4c9cf,#8f959d);`,
  },
  wood: {
    name: "Wood grain",
    css: `background-color:#a9744a;background-image:
      repeating-linear-gradient(90deg,rgba(60,30,10,.16) 0 2px,transparent 2px 38px),
      repeating-linear-gradient(90deg,rgba(255,225,180,.1) 0 5px,transparent 5px 23px),
      repeating-linear-gradient(0deg,rgba(70,35,10,.08) 0 1px,transparent 1px 4px),
      linear-gradient(90deg,#9c6a42,#b58052 35%,#a06d45 70%,#b07a4e);`,
  },
  slate: {
    name: "Slate",
    css: `background-color:#3b4350;background-image:
      radial-gradient(ellipse at 20% 15%,rgba(255,255,255,.1),transparent 55%),
      radial-gradient(ellipse at 80% 85%,rgba(0,0,0,.25),transparent 60%),
      radial-gradient(rgba(255,255,255,.04) 1px,transparent 1.5px),
      linear-gradient(160deg,#454e5c,#323943);
      background-size:auto,auto,5px 5px,auto;`,
  },
  terrazzo: {
    name: "Terrazzo",
    css: `background-color:#ece5da;background-image:
      radial-gradient(circle at 20px 25px,#d98c7a 0 6px,transparent 7px),
      radial-gradient(circle at 90px 70px,#6f8f86 0 8px,transparent 9px),
      radial-gradient(circle at 140px 20px,#e0b862 0 5px,transparent 6px),
      radial-gradient(circle at 60px 120px,#8c8fa6 0 7px,transparent 8px),
      radial-gradient(circle at 150px 140px,#d98c7a 0 4px,transparent 5px),
      radial-gradient(circle at 115px 105px,#4f5b66 0 3px,transparent 4px);
      background-size:180px 180px;`,
  },
  paper: {
    name: "Paper",
    css: `background-color:#f1ead9;background-image:
      radial-gradient(rgba(120,90,50,.07) 1px,transparent 1.5px),
      radial-gradient(rgba(255,255,255,.35) 1px,transparent 1.5px),
      linear-gradient(180deg,#f5efe0,#e8dfca);
      background-size:4px 4px,7px 7px,auto;background-position:0 0,2px 3px,0 0;`,
  },
  denim: {
    name: "Denim",
    css: `background-color:#2f4a73;background-image:
      repeating-linear-gradient(45deg,rgba(255,255,255,.07) 0 1px,transparent 1px 3px),
      repeating-linear-gradient(-45deg,rgba(0,0,20,.14) 0 1px,transparent 1px 3px),
      linear-gradient(160deg,#37557f,#27406a);`,
  },
};

/** Starts an animated background on `canvas`; returns a stop() function. */
export function animate(canvas, kind) {
  const c = canvas.getContext("2d");
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let w, h, raf;
  const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
  const resize = () => {
    w = canvas.width = innerWidth * dpr;
    h = canvas.height = innerHeight * dpr;
  };
  resize();
  addEventListener("resize", resize);

  const stars = Array.from({ length: 220 }, () => ({
    x: Math.random(), y: Math.random(), z: Math.random(), tw: Math.random() * 6,
  }));
  const blobs = [
    { hue: 178, x: 0.2, y: 0.3, r: 0.55, sx: 0.00013, sy: 0.00009 },
    { hue: 38, x: 0.75, y: 0.4, r: 0.5, sx: -0.0001, sy: 0.00012 },
    { hue: 205, x: 0.5, y: 0.8, r: 0.6, sx: 0.00008, sy: -0.0001 },
    { hue: 160, x: 0.3, y: 0.7, r: 0.4, sx: -0.00012, sy: -0.00007 },
  ];

  const frame = (t) => {
    if (kind === "aurora") {
      c.fillStyle = "#071a1e";
      c.fillRect(0, 0, w, h);
      c.globalCompositeOperation = "lighter";
      for (const b of blobs) {
        const x = (b.x + Math.sin(t * b.sx * 6) * 0.15) * w;
        const y = (b.y + Math.cos(t * b.sy * 6) * 0.12) * h;
        const r = b.r * Math.max(w, h);
        const g = c.createRadialGradient(x, y, 0, x, y, r);
        g.addColorStop(0, `hsla(${b.hue},85%,60%,.55)`);
        g.addColorStop(1, `hsla(${b.hue},85%,40%,0)`);
        c.fillStyle = g;
        c.fillRect(0, 0, w, h);
      }
      c.globalCompositeOperation = "source-over";
    } else if (kind === "nebula") {
      c.fillStyle = "#06060f";
      c.fillRect(0, 0, w, h);
      c.globalCompositeOperation = "lighter";
      [[280, 0.35, 0.4], [200, 0.65, 0.6], [330, 0.5, 0.25]].forEach(([hue, bx, by], i) => {
        const x = (bx + Math.sin(t / 9000 + i) * 0.05) * w;
        const y = (by + Math.cos(t / 11000 + i) * 0.05) * h;
        const r = 0.45 * Math.max(w, h);
        const g = c.createRadialGradient(x, y, 0, x, y, r);
        g.addColorStop(0, `hsla(${hue},70%,45%,.35)`);
        g.addColorStop(1, "hsla(0,0%,0%,0)");
        c.fillStyle = g;
        c.fillRect(0, 0, w, h);
      });
      c.globalCompositeOperation = "source-over";
      for (const s of stars) {
        s.y += (0.00002 + s.z * 0.00006) * (reduce ? 0 : 16);
        if (s.y > 1) s.y = 0;
        const a = 0.4 + 0.6 * Math.abs(Math.sin(t / 900 + s.tw));
        c.fillStyle = `rgba(255,255,255,${a * (0.3 + s.z * 0.7)})`;
        const size = (0.6 + s.z * 1.6) * dpr;
        c.fillRect(s.x * w, s.y * h, size, size);
      }
    } else {
      // ripple
      const g = c.createLinearGradient(0, 0, w, h);
      g.addColorStop(0, "#0d3b4a");
      g.addColorStop(1, "#1a1f3d");
      c.fillStyle = g;
      c.fillRect(0, 0, w, h);
      const cx = w / 2, cy = h / 2, max = Math.hypot(cx, cy);
      for (let i = 0; i < 9; i++) {
        const r = ((t / 40 + i * (max / 9)) % max);
        c.strokeStyle = `rgba(140,230,220,${0.18 * (1 - r / max)})`;
        c.lineWidth = 2 * dpr;
        c.beginPath();
        c.arc(cx, cy, r, 0, Math.PI * 2);
        c.stroke();
      }
    }
    if (!reduce) raf = requestAnimationFrame(frame);
  };
  raf = requestAnimationFrame(frame);
  return () => {
    cancelAnimationFrame(raf);
    removeEventListener("resize", resize);
  };
}
