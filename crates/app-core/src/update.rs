//! Update checks against the project's GitHub releases, the native
//! equivalent of legacy `needUpdate` (`getGithubVersion` + version compare).

use serde::Deserialize;

/// Repository whose latest release defines the newest published version.
pub const REPOSITORY: &str = "PaulTony7/casemove";

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateStatus {
    pub current: String,
    pub latest: Option<String>,
    /// Whether the latest published version is newer than this build.
    pub available: bool,
    /// Release page to download from, when one was published.
    pub download_url: Option<String>,
}

impl UpdateStatus {
    pub fn unchecked(current: &str) -> Self {
        Self {
            current: current.to_owned(),
            latest: None,
            available: false,
            download_url: None,
        }
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

/// Fetches the newest published release and compares it with `current`.
/// Any transport or shape error is reported as a message; the caller keeps
/// the unchecked status in that case.
pub async fn check(current: &str) -> Result<UpdateStatus, String> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let client = reqwest::Client::builder()
        .user_agent(format!("casemove/{current}"))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    // A repository with no published release answers 404.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(UpdateStatus::unchecked(current));
    }
    if !response.status().is_success() {
        return Err(format!("GitHub answered {}", response.status()));
    }
    let release: Release = response.json().await.map_err(|error| error.to_string())?;
    let latest = release.tag_name.trim_start_matches('v').to_owned();
    Ok(UpdateStatus {
        current: current.to_owned(),
        available: is_newer(&latest, current),
        latest: Some(latest),
        download_url: Some(release.html_url),
    })
}

/// Dotted version comparison; non-numeric segments count as zero, so
/// `"1.0.0-beta"` compares like `1.0.0`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |version: &str| -> Vec<u32> {
        version
            .split('.')
            .map(|part| {
                part.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let mut candidate = parse(candidate);
    let mut current = parse(current);
    let length = candidate.len().max(current.len());
    candidate.resize(length, 0);
    current.resize(length, 0);
    candidate > current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_dotted_versions_left_to_right() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.12"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1"));
        assert!(is_newer("0.2.0-beta", "0.1.0"));
        assert!(!is_newer("0.1.0-beta", "0.1.0"));
    }

    #[test]
    fn unchecked_status_reports_the_build_version() {
        let status = UpdateStatus::unchecked("0.1.0");
        assert_eq!(status.current, "0.1.0");
        assert!(!status.available);
        assert_eq!(status.latest, None);
    }
}
