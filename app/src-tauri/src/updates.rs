//! Checks GitHub Releases for a newer version. Nothing but the request itself is
//! sent (GitHub sees an IP address and a "Yonder/<version>" user agent).

use serde::Serialize;
use std::time::Duration;

const LATEST_URL: &str = "https://api.github.com/repos/RMoura98/yonder/releases/latest";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UpdateInfo {
    /// e.g. "1.1.0"
    pub version: String,
    /// Release page with the installers and notes.
    pub url: String,
}

/// Parses "v1.2.3", "1.2.3" or "1.2" into comparable numbers; ignores pre-release suffixes.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>());
    let major = it.next()?.ok()?;
    let minor = it.next().unwrap_or(Ok(0)).ok()?;
    let patch = it.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Returns the latest release if it's newer than `current`.
pub fn fetch_latest(current: &str) -> Result<Option<UpdateInfo>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .user_agent(&format!("Yonder/{current}"))
        .build();
    let body = agent
        .get(LATEST_URL)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "No releases published yet.".to_string(),
            ureq::Error::Status(code, _) => format!("GitHub answered with HTTP {code}."),
            ureq::Error::Transport(_) => "Couldn't reach GitHub. Check your internet connection.".to_string(),
        })?
        .into_string()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return Ok(None);
    }
    let tag = json["tag_name"].as_str().unwrap_or_default();
    let url = json["html_url"].as_str().unwrap_or_default();
    if tag.is_empty() || !is_newer(tag, current) {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        url: url.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("v1.1.0", "1.0.0"));
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(is_newer("2.0", "1.99.99"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        assert!(!is_newer("v1.1.0-beta.1", "1.1.0"));
        assert!(!is_newer("garbage", "1.0.0"));
    }
}
