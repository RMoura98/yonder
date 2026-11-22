// Usage: node retime.mjs <framesDir> [fps] -> writes concat.txt for ffmpeg (variable durations).
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
const [, , dir] = process.argv;
const t = readFileSync(join(dir, "timing.txt"), "utf8").trim().split("\n").map(Number);
const lines = ["ffconcat version 1.0"];
for (let i = 0; i < t.length; i++) {
  const dur = i < t.length - 1 ? (t[i + 1] - t[i]) / 1000 : 1 / 30;
  lines.push(`file 'f${String(i).padStart(4, "0")}.jpg'`, `duration ${Math.max(dur, 0.001).toFixed(4)}`);
}
lines.push(`file 'f${String(t.length - 1).padStart(4, "0")}.jpg'`);
writeFileSync(join(dir, "concat.txt"), lines.join("\n"));
console.log("frames", t.length, "duration", (t.at(-1) / 1000).toFixed(2));
