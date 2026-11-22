// Tiny i18n: dictionaries are keyed by the English source text, so a missing
// translation simply falls back to English. Keys may contain {placeholders}.
//
//   await initI18n(settings.language)   // "system" | "en" | "pt" | "es" | "fr" | "de"
//   t("Next break in {t}", { t: "4:12" })
//   applyI18n(document)                 // translates the static page
//
// Elements can opt in explicitly with data-i18n (text), data-i18n-html (trusted
// markup), data-i18n-placeholder / -title / -aria-label. Plain text nodes and
// those attributes are also translated automatically when they match a key.
// Add data-no-i18n to skip an element and its children.

export const LANGUAGES = [
  { code: "en", name: "English" },
  { code: "pt", name: "Português" },
  { code: "es", name: "Español" },
  { code: "fr", name: "Français" },
  { code: "de", name: "Deutsch" },
];

let lang = "en";
let dict = {};
let patterns = []; // [{ re, keys, value }] for keys with placeholders

export function resolveLanguage(setting) {
  const codes = LANGUAGES.map((l) => l.code);
  if (codes.includes(setting)) return setting;
  const nav = (navigator.languages?.[0] || navigator.language || "en").toLowerCase();
  return codes.find((c) => nav === c || nav.startsWith(c + "-")) || "en";
}

export const currentLanguage = () => lang;

export async function initI18n(setting = "system") {
  lang = resolveLanguage(setting);
  document.documentElement.lang = lang === "pt" ? "pt-PT" : lang;
  dict = {};
  patterns = [];
  if (lang !== "en") {
    try {
      dict = await (await fetch(`i18n/${lang}.json`)).json();
    } catch (e) {
      console.warn("i18n: could not load", lang, e);
    }
  }
  for (const [key, value] of Object.entries(dict)) {
    if (!key.includes("{")) continue;
    const names = [];
    const src = key
      .split(/(\{\w+\})/)
      .map((part) => {
        const m = part.match(/^\{(\w+)\}$/);
        if (m) {
          names.push(m[1]);
          return "(.+?)";
        }
        return part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      })
      .join("");
    patterns.push({ re: new RegExp("^" + src + "$"), names, value });
  }
  return lang;
}

const fill = (s, vars) => (vars ? s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : s);

/** Translates a key (English text), filling {placeholders}. */
export function t(key, vars) {
  return fill(dict[key] ?? key, vars);
}

/**
 * Translates arbitrary English text, including text that matches a pattern key
 * like "{n} minutes". Used for static markup and for stored default content
 * (only exact English defaults change; anything the user typed stays as-is).
 */
export function tr(text) {
  if (text == null) return text;
  const s = String(text);
  const trimmed = s.trim();
  if (!trimmed || lang === "en") return s;
  let out = dict[trimmed];
  if (out == null) {
    for (const p of patterns) {
      const m = trimmed.match(p.re);
      if (m) {
        out = fill(p.value, Object.fromEntries(p.names.map((n, i) => [n, m[i + 1]])));
        break;
      }
    }
  }
  if (out == null) return s;
  // keep the original surrounding whitespace
  return s.replace(trimmed, out);
}

/** n-dependent wording: plural(3, "1 minute", "{n} minutes"). */
export function plural(n, one, other) {
  const singular = lang === "fr" ? n < 2 : n === 1;
  return singular ? t(one, { n }) : t(other, { n });
}

/** 2 → "2nd" / "2.ª" / "2e" / "2." */
export function ordinal(n) {
  switch (lang) {
    case "pt":
    case "es":
      return `${n}.ª`;
    case "fr":
      return n === 1 ? "1re" : `${n}e`;
    case "de":
      return `${n}.`;
    default: {
      const s = ["th", "st", "nd", "rd"];
      const v = n % 100;
      return n + (s[(v - 20) % 10] || s[v] || s[0]);
    }
  }
}

/** Locale for Intl / toLocale* calls. */
export const locale = () => (lang === "pt" ? "pt-PT" : lang);

const ATTRS = [
  ["i18nPlaceholder", "placeholder"],
  ["i18nTitle", "title"],
  ["i18nAriaLabel", "aria-label"],
];

export function applyI18n(root = document) {
  const scope = root.body || root;
  // explicit markers (remember the English source the first time)
  scope.querySelectorAll("[data-i18n]").forEach((el) => {
    el.dataset.i18nSrc ??= el.dataset.i18n || el.textContent.trim();
    el.textContent = t(el.dataset.i18nSrc);
  });
  scope.querySelectorAll("[data-i18n-html]").forEach((el) => {
    el.dataset.i18nSrc ??= el.dataset.i18nHtml || el.innerHTML.trim();
    el.innerHTML = t(el.dataset.i18nSrc);
  });
  for (const [ds, attr] of ATTRS) {
    scope.querySelectorAll(`[data-${attr === "aria-label" ? "i18n-aria-label" : "i18n-" + attr}]`).forEach((el) => {
      el.dataset[ds + "Src"] ??= el.dataset[ds] || el.getAttribute(attr) || "";
      el.setAttribute(attr, t(el.dataset[ds + "Src"]));
    });
  }
  if (lang === "en") return;
  // automatic: text nodes and common attributes that match a key
  const skip = (el) => el.closest?.("[data-no-i18n],[data-i18n],[data-i18n-html],script,style,code,kbd");
  const walker = document.createTreeWalker(scope, NodeFilter.SHOW_TEXT);
  const nodes = [];
  while (walker.nextNode()) nodes.push(walker.currentNode);
  for (const node of nodes) {
    if (!node.nodeValue.trim() || skip(node.parentElement)) continue;
    node.__i18nSrc ??= node.nodeValue;
    node.nodeValue = tr(node.__i18nSrc);
  }
  scope.querySelectorAll("[placeholder],[title],[aria-label],[alt]").forEach((el) => {
    if (skip(el) && !el.matches("[data-i18n]")) return;
    for (const attr of ["placeholder", "title", "aria-label", "alt"]) {
      if (!el.hasAttribute(attr)) continue;
      const key = `__i18n_${attr}`;
      el[key] ??= el.getAttribute(attr);
      el.setAttribute(attr, tr(el[key]));
    }
  });
}
