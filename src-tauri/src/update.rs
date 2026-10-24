//! Update check: compare the running build against the latest GitHub
//! Release. Notification only — never auto-downloads or installs.

use serde::{Deserialize, Serialize};

/// Cloudflare Worker proxy (update-worker/) holds the GitHub PAT
/// server-side so the private repo's release feed needs no app-side auth.
pub const UPDATE_URL: &str = "https://streamy-update-proxy.renggap.workers.dev/latest";

#[derive(Serialize, Clone, Deserialize)]
pub struct UpdateInfo {
    /// Latest version tag, e.g. "v1.5.0" (leading v stripped).
    pub version: String,
    /// Human-readable release notes (first paragraph of the body).
    pub notes: Option<String>,
    /// Browser URL for the release page.
    pub url: String,
}

/// "v1.4.0" -> (1,4,0). Anything unparseable -> None.
pub fn parse_semver(tag: &str) -> Option<(u64, u64, u64)> {
    let mut it = tag.trim_start_matches('v').split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

/// True when `latest` is strictly newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Extract the release body's first paragraph as short notes.
/// Fetch the latest release info from the worker proxy.
pub async fn fetch_latest(client: &reqwest::Client) -> Result<UpdateInfo, String> {
    let info: UpdateInfo = client
        .get(UPDATE_URL)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("update check failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("update check failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("update check failed: {e}"))?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parses_plain_and_v_prefixed() {
        assert_eq!(parse_semver("1.4.0"), Some((1, 4, 0)));
        assert_eq!(parse_semver("v1.4.0"), Some((1, 4, 0)));
        assert_eq!(parse_semver("v2.0"), Some((2, 0, 0)));
        assert_eq!(parse_semver("junk"), None);
        assert_eq!(parse_semver("1.4.0.1"), None);
    }

    #[test]
    fn newer_compares_tuplewise() {
        assert!(is_newer("v1.5.0", "1.4.0"));
        assert!(is_newer("v1.4.1", "v1.4.0"));
        assert!(!is_newer("v1.4.0", "v1.4.0"));
        assert!(!is_newer("v1.3.9", "1.4.0"));
        // Unparseable never triggers a notification.
        assert!(!is_newer("latest", "1.4.0"));
    }
}
