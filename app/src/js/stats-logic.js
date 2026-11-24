// Pure helpers for the Stats page: adherence, streaks, weekly summary, heatmap
// levels and CSV / Markdown export. No DOM access, so they can be unit-tested
// with `node --test app/src/js/stats-logic.test.mjs`.

/** A day is "good" when at least this share of the breaks that were due got taken. */
export const GOOD_DAY = 0.8;

export const dayKey = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

/** Local-date arithmetic that is safe across DST changes. */
export const addDays = (d, n) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);

/** Parses "YYYY-MM-DD" as a local date. */
export const parseKey = (k) => {
  const [y, m, d] = k.split("-").map(Number);
  return new Date(y, m - 1, d);
};

/**
 * Breaks taken (including natural ones) / breaks that were due, capped at 1.
 * "Due" is estimated from screen time and the work interval, the same way the
 * screen score is. null = no activity recorded that day.
 */
export function adherence(day, workMins) {
  if (!day || !day.screen_secs) return null;
  const expected = Math.max(1, Math.floor(day.screen_secs / (Math.max(1, workMins) * 60)));
  return Math.min(1, ((day.breaks_taken || 0) + (day.natural_breaks || 0)) / expected);
}

/** Heatmap intensity 0 (no activity) to 4 (every break taken). */
export function level(a) {
  if (a == null) return 0;
  if (a >= 1) return 4;
  if (a >= GOOD_DAY) return 3;
  if (a >= 0.4) return 2;
  return 1;
}

/**
 * Day streaks over days with activity. Days without recorded screen time (a
 * weekend, a holiday, the app not running) neither extend nor break a streak,
 * and today does not break it until it is over.
 * `days` maps "YYYY-MM-DD" to DayStats; `today` is a Date.
 */
export function streaks(days, workMins, today = new Date()) {
  const keys = Object.keys(days).sort();
  const todayKey = dayKey(today);
  let best = 0;
  let run = 0;
  for (const k of keys) {
    const a = adherence(days[k], workMins);
    if (a == null) continue;
    run = a >= GOOD_DAY ? run + 1 : 0;
    best = Math.max(best, run);
  }
  let current = 0;
  for (let i = keys.length - 1; i >= 0; i--) {
    const a = adherence(days[keys[i]], workMins);
    if (a == null) continue;
    if (a >= GOOD_DAY) current++;
    else if (keys[i] !== todayKey) break;
  }
  return { current, best };
}

/** Totals for the 7 days ending at `end` (inclusive). */
export function weekTotals(days, workMins, end) {
  const t = { screen_secs: 0, breaks_taken: 0, breaks_skipped: 0, natural_breaks: 0, adherence: null };
  let sum = 0;
  let n = 0;
  for (let i = 0; i < 7; i++) {
    const d = days[dayKey(addDays(end, -i))];
    if (!d) continue;
    for (const k of ["screen_secs", "breaks_taken", "breaks_skipped", "natural_breaks"]) t[k] += d[k] || 0;
    const a = adherence(d, workMins);
    if (a != null) {
      sum += a;
      n++;
    }
  }
  if (n) t.adherence = sum / n;
  return t;
}

/** Last 7 days against the 7 before them. */
export function weeklySummary(days, workMins, today = new Date()) {
  return { thisWeek: weekTotals(days, workMins, today), lastWeek: weekTotals(days, workMins, addDays(today, -7)) };
}

/**
 * Cells for a 53-column, Monday-first heatmap ending with the week of `today`.
 * Returns { weeks: [[cell|null x7] x53] } where cell = { key, date, level, day }.
 * Days after today are null.
 */
export function heatmap(days, workMins, today = new Date()) {
  const dow = (today.getDay() + 6) % 7; // Monday = 0
  const start = addDays(today, -dow - 52 * 7);
  const weeks = [];
  for (let w = 0; w < 53; w++) {
    const col = [];
    for (let r = 0; r < 7; r++) {
      const date = addDays(start, w * 7 + r);
      if (date > today) {
        col.push(null);
        continue;
      }
      const key = dayKey(date);
      col.push({ key, date, day: days[key] || null, level: level(adherence(days[key], workMins)) });
    }
    weeks.push(col);
  }
  return { weeks };
}

const CSV_COLS = ["screen_secs", "breaks_taken", "breaks_skipped", "natural_breaks", "reminders_shown", "reminders_done", "longest_stretch_secs"];

/** One row per recorded day, oldest first. Machine-friendly: ISO dates, seconds. */
export function toCsv(days) {
  const rows = Object.keys(days).sort().map((k) => [k, ...CSV_COLS.map((c) => days[k][c] || 0)].join(","));
  return ["date," + CSV_COLS.join(","), ...rows].join("\r\n") + "\r\n";
}

/**
 * Human-friendly journal, newest first. `L` carries the translated labels and
 * formatters: { title, headers: [7 strings], summary, fmtDate(d), fmtDur(s) }.
 */
export function toMarkdown(days, L) {
  const keys = Object.keys(days).sort().reverse();
  const head = L.headers;
  const lines = [`# ${L.title}`, "", L.summary, "", `| ${head.join(" | ")} |`, `|${head.map((_, i) => (i ? " ---: " : " --- ")).join("|")}|`];
  for (const k of keys) {
    const d = days[k];
    lines.push(`| ${[L.fmtDate(parseKey(k)), L.fmtDur(d.screen_secs || 0), d.breaks_taken || 0, d.breaks_skipped || 0, d.natural_breaks || 0, `${d.reminders_done || 0}/${d.reminders_shown || 0}`, L.fmtDur(d.longest_stretch_secs || 0)].join(" | ")} |`);
  }
  return lines.join("\n") + "\n";
}
