//! Resolve client-requested model ids (e.g. Anthropic official ids sent by the
//! Claude Code `/model` picker) to ids that GitHub Copilot actually accepts.
//!
//! Resolution order:
//! 1. Exact override from the optional user map file (`~/.copilot-gateway/model_map.json`).
//! 2. The requested id is already a valid Copilot id -> pass through unchanged.
//! 3. Family normalization (opus / sonnet / haiku) -> a sensible Copilot id,
//!    overridable per-family via the same map file.
//! 4. Unknown -> pass through unchanged.

use std::collections::HashMap;

use crate::config;

/// User-provided exact/family overrides loaded from disk. Empty when no file.
#[derive(Debug, Clone, Default)]
pub struct ModelMap {
    overrides: HashMap<String, String>,
}

impl ModelMap {
    /// Load `~/.copilot-gateway/model_map.json` if present. A missing or invalid
    /// file is non-fatal (logged) and yields an empty map.
    pub fn load() -> Self {
        let path = match config::model_map_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<HashMap<String, String>>(&text) {
                Ok(map) => {
                    tracing::info!("Loaded {} model map override(s) from {:?}", map.len(), path);
                    let overrides = map.into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect();
                    Self { overrides }
                }
                Err(err) => {
                    tracing::warn!("Ignoring invalid model_map.json ({err})");
                    Self::default()
                }
            },
            Err(err) => {
                tracing::warn!("Could not read model_map.json ({err})");
                Self::default()
            }
        }
    }

    /// Resolve `requested` to a Copilot model id, given the list of available ids.
    pub fn resolve(&self, requested: &str, available: &[String]) -> String {
        let key = requested.to_lowercase();

        // 1. Exact user override.
        if let Some(v) = self.overrides.get(&key) {
            return v.clone();
        }

        // 2. Already a valid Copilot id (preserves explicit version choices).
        if available.iter().any(|id| id.eq_ignore_ascii_case(requested)) {
            return requested.to_string();
        }

        // 3. Family normalization.
        if let Some(family) = detect_family(&key) {
            if let Some(v) = self.overrides.get(family) {
                return v.clone();
            }
            return family_default(family, available);
        }

        // 4. Unknown -> pass through unchanged.
        requested.to_string()
    }
}

/// Detect the Claude model family from a (lowercased) model id.
fn detect_family(id: &str) -> Option<&'static str> {
    if id.contains("haiku") {
        Some("haiku")
    } else if id.contains("sonnet") {
        Some("sonnet")
    } else if id.contains("opus") {
        Some("opus")
    } else {
        None
    }
}

/// Preferred Copilot id for a family, falling back to the best available match
/// if the preferred id is not in the account's model list.
fn family_default(family: &str, available: &[String]) -> String {
    let preferred = match family {
        "opus" => "claude-opus-4.8",
        "sonnet" => "claude-sonnet-4.6",
        "haiku" => "claude-haiku-4.5",
        _ => return family.to_string(),
    };

    if available.iter().any(|id| id == preferred) {
        return preferred.to_string();
    }

    // Fallback: among available ids of this family, prefer "plain" variants
    // (skip internal/experimental suffixes), then take the lexicographically
    // greatest (newest version) id.
    let best = available
        .iter()
        .filter(|id| id.contains(family))
        .filter(|id| {
            !id.contains("internal")
                && !id.contains("-1m")
                && !id.contains("-high")
                && !id.contains("-xhigh")
                && !id.contains("codex")
        })
        .max();

    match best {
        Some(id) => id.clone(),
        None => preferred.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available() -> Vec<String> {
        [
            "claude-opus-4.5",
            "claude-opus-4.7",
            "claude-opus-4.8",
            "claude-sonnet-4.5",
            "claude-sonnet-4.6",
            "claude-haiku-4.5",
            "gpt-5.5",
            "gpt-4o",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn passes_through_valid_copilot_id() {
        let m = ModelMap::default();
        // Explicit version choice must not be remapped to the family default.
        assert_eq!(m.resolve("claude-opus-4.7", &available()), "claude-opus-4.7");
        assert_eq!(m.resolve("gpt-4o", &available()), "gpt-4o");
    }

    #[test]
    fn maps_anthropic_official_ids_by_family() {
        let m = ModelMap::default();
        assert_eq!(
            m.resolve("claude-opus-4-1-20250805", &available()),
            "claude-opus-4.8"
        );
        assert_eq!(
            m.resolve("claude-sonnet-4-5-20250929", &available()),
            "claude-sonnet-4.6"
        );
        assert_eq!(
            m.resolve("claude-3-5-haiku-20241022", &available()),
            "claude-haiku-4.5"
        );
    }

    #[test]
    fn case_insensitive_and_short_aliases() {
        let m = ModelMap::default();
        assert_eq!(m.resolve("Opus", &available()), "claude-opus-4.8");
        assert_eq!(m.resolve("SONNET", &available()), "claude-sonnet-4.6");
        assert_eq!(m.resolve("haiku", &available()), "claude-haiku-4.5");
    }

    #[test]
    fn unknown_passes_through() {
        let m = ModelMap::default();
        assert_eq!(m.resolve("gpt-5.5", &available()), "gpt-5.5");
        assert_eq!(m.resolve("some-future-model", &available()), "some-future-model");
    }

    #[test]
    fn user_override_wins() {
        let mut overrides = HashMap::new();
        overrides.insert("opus".to_string(), "claude-opus-4.7".to_string());
        overrides.insert("claude-foo".to_string(), "gpt-5.5".to_string());
        let m = ModelMap { overrides };
        assert_eq!(m.resolve("claude-opus-4-1-20250805", &available()), "claude-opus-4.7");
        assert_eq!(m.resolve("claude-foo", &available()), "gpt-5.5");
    }

    #[test]
    fn family_default_falls_back_when_preferred_missing() {
        let avail: Vec<String> = ["claude-opus-4.5", "claude-opus-4.6"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Preferred 4.8 absent -> pick newest available plain opus.
        assert_eq!(family_default("opus", &avail), "claude-opus-4.6");
    }
}
