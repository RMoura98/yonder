import test from "node:test";
import assert from "node:assert/strict";
import { adherence, level, streaks, weeklySummary, heatmap, toCsv, toMarkdown, dayKey, addDays } from "./stats-logic.js";

const today = new Date(2026, 10, 25); // Wed 25 Nov 2026
const good = { screen_secs: 3600, breaks_taken: 3 }; // 3 due with 20 min work
const bad = { screen_secs: 3600, breaks_taken: 1, breaks_skipped: 2 };
const at = (n) => dayKey(addDays(today, -n));

test("adherence", () => {
  assert.equal(adherence(null, 20), null);
  assert.equal(adherence({ screen_secs: 0 }, 20), null);
  assert.equal(adherence(good, 20), 1);
  assert.equal(adherence({ screen_secs: 60, breaks_taken: 0 }, 20), 0); // at least 1 due
  assert.equal(adherence({ screen_secs: 3600, breaks_taken: 2, natural_breaks: 5 }, 20), 1);
  assert.equal(level(0.8), 3);
  assert.equal(level(null), 0);
});

test("streaks skip idle days and tolerate an unfinished today", () => {
  const days = { [at(0)]: bad, [at(1)]: good, [at(2)]: {}, [at(3)]: good, [at(4)]: bad, [at(5)]: good, [at(6)]: good, [at(7)]: good };
  assert.deepEqual(streaks(days, 20, today), { current: 2, best: 3 });
  days[at(1)] = bad; // a bad day that is over breaks it
  assert.equal(streaks(days, 20, today).current, 0);
  assert.deepEqual(streaks({}, 20, today), { current: 0, best: 0 });
});

test("weekly summary compares 7-day windows", () => {
  const days = { [at(0)]: good, [at(6)]: bad, [at(7)]: good, [at(13)]: good, [at(14)]: good };
  const { thisWeek, lastWeek } = weeklySummary(days, 20, today);
  assert.equal(thisWeek.breaks_taken, 4);
  assert.equal(thisWeek.breaks_skipped, 2);
  assert.equal(lastWeek.breaks_taken, 6);
  assert.equal(lastWeek.screen_secs, 7200);
  assert.ok(Math.abs(thisWeek.adherence - (1 + 1 / 3) / 2) < 1e-9);
});

test("heatmap covers 53 Monday-first weeks ending today", () => {
  const { weeks } = heatmap({ [at(0)]: good }, 20, today);
  assert.equal(weeks.length, 53);
  assert.equal(weeks[0][0].date.getDay(), 1);
  assert.equal(weeks[52][2].key, at(0)); // Wednesday
  assert.equal(weeks[52][2].level, 4);
  assert.equal(weeks[52][3], null);
});

test("exports", () => {
  const days = { "2026-11-24": { ...good, reminders_shown: 4, reminders_done: 2 }, "2026-11-23": bad };
  const csv = toCsv(days).split("\r\n");
  assert.equal(csv[0].split(",")[0], "date");
  assert.equal(csv[1].split(",")[0], "2026-11-23");
  assert.equal(csv[2], "2026-11-24,3600,3,0,0,4,2,0");
  const md = toMarkdown(days, { title: "T", summary: "S", headers: ["a", "b", "c", "d", "e", "f", "g"], fmtDate: (d) => dayKey(d), fmtDur: (s) => `${s}s` });
  assert.match(md, /^# T\n\nS\n\n\| a \| b/);
  assert.ok(md.indexOf("2026-11-24") < md.indexOf("2026-11-23"));
});
