//! Translations for text the Rust core shows itself (tray menu, status line).
//! Uses the same dictionaries as the UI (`app/src/i18n/*.json`), keyed by the
//! English source text, so a missing entry falls back to English.

use std::collections::HashMap;
use std::sync::OnceLock;

type Dict = HashMap<String, String>;

fn dicts() -> &'static HashMap<&'static str, Dict> {
    static DICTS: OnceLock<HashMap<&'static str, Dict>> = OnceLock::new();
    DICTS.get_or_init(|| {
        let raw: [(&str, &str); 6] = [
            ("pt", include_str!("../../src/i18n/pt.json")),
            ("es", include_str!("../../src/i18n/es.json")),
            ("fr", include_str!("../../src/i18n/fr.json")),
            ("de", include_str!("../../src/i18n/de.json")),
            ("zh", include_str!("../../src/i18n/zh.json")),
            ("ja", include_str!("../../src/i18n/ja.json")),
        ];
        raw.into_iter()
            .map(|(code, json)| (code, serde_json::from_str(json).unwrap_or_default()))
            .collect()
    })
}

const SUPPORTED: [&str; 7] = ["en", "pt", "es", "fr", "de", "zh", "ja"];

/// Language from the OS (e.g. "pt-PT" → "pt"), falling back to English.
fn system_language() -> &'static str {
    static SYS: OnceLock<&'static str> = OnceLock::new();
    SYS.get_or_init(|| {
        let loc = sys_locale::get_locale().unwrap_or_default().to_lowercase();
        SUPPORTED
            .into_iter()
            .find(|c| loc == *c || loc.starts_with(&format!("{c}-")) || loc.starts_with(&format!("{c}_")))
            .unwrap_or("en")
    })
}

/// Maps the "language" setting ("system" or a code) to a supported code.
pub fn resolve(setting: &str) -> &'static str {
    SUPPORTED
        .into_iter()
        .find(|c| *c == setting)
        .unwrap_or_else(system_language)
}

/// Translates `key` (English text) into `lang` and fills `{name}` placeholders.
pub fn tr(lang: &str, key: &str, vars: &[(&str, &str)]) -> String {
    let mut s = dicts()
        .get(lang)
        .and_then(|d| d.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string());
    for (name, value) in vars {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::{resolve, tr};

    #[test]
    fn translates_and_falls_back() {
        assert_eq!(tr("pt", "Quit Yonder", &[]), "Sair do Yonder");
        assert_eq!(tr("de", "Next break in {t}", &[("t", "4:12")]), "Nächste Pause in 4:12");
        assert_eq!(tr("zh", "Next break in {t}", &[("t", "4:12")]), "距下次休息 4:12");
        assert_eq!(tr("ja", "Quit Yonder", &[]), "Yonder を終了");
        assert_eq!(tr("en", "Quit Yonder", &[]), "Quit Yonder");
        assert_eq!(tr("fr", "not a key", &[]), "not a key");
        assert_eq!(resolve("es"), "es");
        assert!(["en", "pt", "es", "fr", "de", "zh", "ja"].contains(&resolve("system")));
    }
}
