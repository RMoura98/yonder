//! Scripting: turns command-line flags (`yonder --pause 30`) and `yonder://`
//! links (`yonder://pause/30`) into the same actions the tray menu and the
//! global shortcuts use. Pure parsing only; `lib.rs` runs the result.

/// One validated command, ready for `run_action`.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub action: &'static str,
    pub arg: Option<i64>,
}

/// Everything found in one launch's arguments.
#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub commands: Vec<Command>,
    /// Unknown or invalid commands, as typed, for the log.
    pub ignored: Vec<String>,
    /// `--background`: start without opening the settings window.
    pub background: bool,
    /// `--verbose`: debug-level logging for this run.
    pub verbose: bool,
}

impl Parsed {
    /// True when the arguments asked for something (even if it was invalid), so
    /// a script's typo doesn't pop the settings window open.
    pub fn is_request(&self) -> bool {
        !self.commands.is_empty() || !self.ignored.is_empty() || self.background
    }
}

struct Spec {
    name: &'static str,
    action: &'static str,
    /// Allowed range of the optional number, if the command takes one.
    range: Option<(i64, i64)>,
}

const SPECS: &[Spec] = &[
    Spec { name: "break", action: "take_break", range: Some((1, 120)) },
    Spec { name: "end-break", action: "end_break", range: None },
    Spec { name: "skip", action: "skip", range: None },
    Spec { name: "snooze", action: "snooze", range: Some((1, 120)) },
    Spec { name: "postpone", action: "snooze", range: Some((1, 120)) },
    Spec { name: "pause", action: "pause", range: Some((1, 1440)) },
    Spec { name: "pause-tomorrow", action: "pause_tomorrow", range: None },
    Spec { name: "resume", action: "resume", range: None },
    Spec { name: "toggle-pause", action: "toggle_pause", range: None },
    Spec { name: "focus", action: "start_focus", range: Some((1, 240)) },
    Spec { name: "focus-stop", action: "stop_focus", range: None },
    Spec { name: "focus-toggle", action: "toggle_focus", range: None },
    Spec { name: "reset", action: "reset_timer", range: None },
    Spec { name: "settings", action: "open_settings", range: None },
];

fn build(name: &str, number: Option<&str>) -> Result<Command, String> {
    let name = name.trim().to_lowercase().replace('_', "-");
    let spec = SPECS.iter().find(|s| s.name == name).ok_or_else(|| "unknown command".to_string())?;
    let arg = match (number, spec.range) {
        (None, _) => None,
        (Some(_), None) => return Err("takes no value".into()),
        (Some(n), Some((lo, hi))) => {
            let n: i64 = n.trim().parse().map_err(|_| "value must be a whole number of minutes".to_string())?;
            if n < lo || n > hi {
                return Err(format!("minutes must be between {lo} and {hi}"));
            }
            Some(n)
        }
    };
    Ok(Command { action: spec.action, arg })
}

/// `yonder://pause/30`, `yonder://pause?minutes=30`, `yonder://break`.
pub fn parse_url(url: &str) -> Result<Command, String> {
    let url = url.trim();
    let rest = match url.get(..8) {
        Some(p) if p.eq_ignore_ascii_case("yonder:/") => url[8..].trim_start_matches('/'),
        _ => return Err("not a yonder:// link".into()),
    };
    let rest = rest.split('#').next().unwrap_or("");
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut segs = path.split('/').filter(|s| !s.is_empty());
    let name = segs.next().ok_or_else(|| "no command".to_string())?;
    let mut number = segs.next();
    if segs.next().is_some() {
        return Err("too many parts".into());
    }
    if number.is_none() {
        number = query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| matches!(*k, "minutes" | "mins" | "m"))
            .map(|(_, v)| v);
    }
    build(name, number)
}

/// Parses the arguments after the executable name.
pub fn parse_args<S: AsRef<str>>(args: &[S]) -> Parsed {
    let mut out = Parsed::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_ref();
        i += 1;
        if a.len() >= 8 && a.get(..8).map_or(false, |p| p.eq_ignore_ascii_case("yonder:/")) {
            match parse_url(a) {
                Ok(c) => out.commands.push(c),
                Err(e) => out.ignored.push(format!("{a} ({e})")),
            }
            continue;
        }
        let Some(flag) = a.strip_prefix("--") else { continue }; // exe path, stray values
        let (name, inline) = match flag.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (flag, None),
        };
        match name {
            "background" => {
                out.background = true;
                continue;
            }
            "verbose" => {
                out.verbose = true;
                continue;
            }
            _ => {}
        }
        // `--pause 30`: a following plain number belongs to this flag.
        let mut number = inline;
        if number.is_none() {
            if let Some(next) = args.get(i).map(|s| s.as_ref()) {
                if !next.starts_with("--") && next.parse::<i64>().is_ok() {
                    number = Some(next);
                    i += 1;
                }
            }
        }
        match build(name, number) {
            Ok(c) => out.commands.push(c),
            Err(e) => out.ignored.push(format!("{a} ({e})")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(action: &'static str, arg: Option<i64>) -> Command {
        Command { action, arg }
    }

    #[test]
    fn flags_map_to_actions() {
        let p = parse_args(&["--break"]);
        assert_eq!(p.commands, vec![cmd("take_break", None)]);
        let p = parse_args(&["--break", "5"]);
        assert_eq!(p.commands, vec![cmd("take_break", Some(5))]);
        let p = parse_args(&["--pause=30"]);
        assert_eq!(p.commands, vec![cmd("pause", Some(30))]);
        let p = parse_args(&["--pause", "30", "--resume", "--focus", "25", "--skip", "--settings"]);
        assert_eq!(
            p.commands,
            vec![
                cmd("pause", Some(30)),
                cmd("resume", None),
                cmd("start_focus", Some(25)),
                cmd("skip", None),
                cmd("open_settings", None)
            ]
        );
        assert_eq!(parse_args(&["--postpone", "10"]).commands, vec![cmd("snooze", Some(10))]);
        assert_eq!(parse_args(&["--pause-tomorrow"]).commands, vec![cmd("pause_tomorrow", None)]);
        assert_eq!(parse_args(&["--focus-toggle"]).commands, vec![cmd("toggle_focus", None)]);
    }

    #[test]
    fn plain_pause_is_indefinite() {
        let p = parse_args(&["--pause", "--resume"]);
        assert_eq!(p.commands, vec![cmd("pause", None), cmd("resume", None)]);
    }

    #[test]
    fn urls_map_to_actions() {
        assert_eq!(parse_url("yonder://break"), Ok(cmd("take_break", None)));
        assert_eq!(parse_url("yonder://break/5"), Ok(cmd("take_break", Some(5))));
        assert_eq!(parse_url("yonder://pause/30/"), Ok(cmd("pause", Some(30))));
        assert_eq!(parse_url("yonder://pause?minutes=45"), Ok(cmd("pause", Some(45))));
        assert_eq!(parse_url("YONDER://Pause-Tomorrow"), Ok(cmd("pause_tomorrow", None)));
        assert_eq!(parse_url("yonder:///resume"), Ok(cmd("resume", None)));
        assert_eq!(parse_url("yonder://toggle_pause#x"), Ok(cmd("toggle_pause", None)));
        assert_eq!(parse_url("yonder://focus/25"), Ok(cmd("start_focus", Some(25))));
    }

    #[test]
    fn rejects_bad_input() {
        for bad in [
            "yonder://",
            "yonder://nope",
            "yonder://pause/abc",
            "yonder://pause/0",
            "yonder://pause/-5",
            "yonder://pause/99999",
            "yonder://skip/5",
            "yonder://pause/1/2",
            "https://example.com",
            "yonder",
        ] {
            assert!(parse_url(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn invalid_and_unknown_are_ignored_not_fatal() {
        let p = parse_args(&["--bogus", "--pause", "0", "--skip"]);
        assert_eq!(p.commands, vec![cmd("skip", None)]);
        assert_eq!(p.ignored.len(), 2);
        assert!(p.is_request());
        let p = parse_args(&["--resume=5"]);
        assert!(p.commands.is_empty());
        assert_eq!(p.ignored.len(), 1);
    }

    #[test]
    fn mixes_urls_and_flags_and_keeps_background() {
        let p = parse_args(&["C:\\Yonder\\yonder.exe", "--background", "--verbose", "yonder://break/2"]);
        assert!(p.background && p.verbose);
        assert_eq!(p.commands, vec![cmd("take_break", Some(2))]);
        assert!(p.ignored.is_empty());
    }

    #[test]
    fn plain_launch_is_not_a_request() {
        assert!(!parse_args::<&str>(&[]).is_request());
        assert!(!parse_args(&["--verbose"]).is_request());
        assert!(parse_args(&["--background"]).is_request());
    }
}
