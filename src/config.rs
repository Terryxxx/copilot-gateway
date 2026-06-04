use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::BaseDirs;

/// Copilot chat plugin version reported to the API.
pub const COPILOT_VERSION: &str = "0.26.7";
/// VS Code version reported to the API (kept configurable to stay current).
pub const VSCODE_VERSION: &str = "1.99.3";
pub const COPILOT_API_VERSION: &str = "2025-04-01";

/// GitHub OAuth app client id used by the official Copilot Chat extension.
pub const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
pub const GITHUB_APP_SCOPES: &str = "read:user";

pub const GITHUB_BASE_URL: &str = "https://github.com";
pub const GITHUB_API_BASE_URL: &str = "https://api.github.com";

/// Shown whenever the GitHub OAuth token appears to be expired/revoked (HTTP 401).
pub const REAUTH_HINT: &str =
    "GitHub authentication appears expired or revoked (HTTP 401). \
Re-authenticate by running: copilot-gateway auth";

pub fn editor_plugin_version() -> String {
    format!("copilot-chat/{COPILOT_VERSION}")
}

pub fn user_agent() -> String {
    format!("GitHubCopilotChat/{COPILOT_VERSION}")
}

pub fn editor_version() -> String {
    format!("vscode/{VSCODE_VERSION}")
}

/// Directory where the GitHub token is persisted.
pub fn config_dir() -> Result<PathBuf> {
    let base = BaseDirs::new().context("could not determine home directory")?;
    Ok(base.home_dir().join(".copilot-gateway"))
}

pub fn github_token_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("github_token"))
}

/// Optional user-editable model alias overrides.
pub fn model_map_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("model_map.json"))
}

/// Persist the GitHub token to disk with restrictive permissions where supported.
pub fn save_github_token(token: &str) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).context("failed to create config dir")?;
    let path = github_token_path()?;
    std::fs::write(&path, token).context("failed to write github token")?;
    set_file_permissions(&path)?;
    Ok(())
}

pub fn load_github_token() -> Result<Option<String>> {
    let path = github_token_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let token = std::fs::read_to_string(&path).context("failed to read github token")?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Ok(None);
    }
    Ok(Some(token))
}

#[cfg(unix)]
fn set_file_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms).context("failed to set token file permissions")?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}
