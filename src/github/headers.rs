use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use uuid::Uuid;

use crate::config::{
    editor_plugin_version, editor_version, user_agent, COPILOT_API_VERSION,
};

fn insert(map: &mut HeaderMap, key: &'static str, value: String) {
    if let Ok(v) = HeaderValue::from_str(&value) {
        map.insert(HeaderName::from_static(key), v);
    }
}

/// Headers used when calling api.github.com with the GitHub OAuth token.
pub fn github_headers(github_token: &str) -> HeaderMap {
    let mut map = HeaderMap::new();
    insert(&mut map, "accept", "application/json".into());
    insert(&mut map, "content-type", "application/json".into());
    insert(&mut map, "authorization", format!("token {github_token}"));
    insert(&mut map, "editor-version", editor_version());
    insert(&mut map, "editor-plugin-version", editor_plugin_version());
    insert(&mut map, "user-agent", user_agent());
    insert(&mut map, "x-github-api-version", COPILOT_API_VERSION.into());
    insert(
        &mut map,
        "x-vscode-user-agent-library-version",
        "electron-fetch".into(),
    );
    map
}

/// Headers used when calling the Copilot API with the short-lived Copilot token.
pub fn copilot_headers(copilot_token: &str, vision: bool, is_agent_call: bool) -> HeaderMap {
    let mut map = HeaderMap::new();
    insert(&mut map, "authorization", format!("Bearer {copilot_token}"));
    insert(&mut map, "content-type", "application/json".into());
    insert(&mut map, "copilot-integration-id", "vscode-chat".into());
    insert(&mut map, "editor-version", editor_version());
    insert(&mut map, "editor-plugin-version", editor_plugin_version());
    insert(&mut map, "user-agent", user_agent());
    insert(&mut map, "openai-intent", "conversation-panel".into());
    insert(&mut map, "x-github-api-version", COPILOT_API_VERSION.into());
    insert(&mut map, "x-request-id", Uuid::new_v4().to_string());
    insert(
        &mut map,
        "x-vscode-user-agent-library-version",
        "electron-fetch".into(),
    );
    insert(
        &mut map,
        "x-initiator",
        if is_agent_call { "agent" } else { "user" }.into(),
    );
    if vision {
        insert(&mut map, "copilot-vision-request", "true".into());
    }
    map
}
