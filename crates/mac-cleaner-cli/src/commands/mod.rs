pub mod apply;
pub mod clean;
pub mod config;
pub mod history;
pub mod plan;
pub mod progress;
pub mod restore;
pub mod scan;

use std::path::PathBuf;

/// Returns standard state runs directory: `~/.local/state/mac-cleaner/runs`.
pub fn runs_dir() -> PathBuf {
    let base = if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(xdg).join("mac-cleaner").join("runs")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("mac-cleaner")
            .join("runs")
    } else {
        PathBuf::from(".mac-cleaner").join("runs")
    };

    std::fs::create_dir_all(&base).ok();
    base
}

/// Parses a duration string (e.g. "30d", "14d", "2w", "30") into days.
pub fn parse_duration_to_days(input: &str) -> Option<u32> {
    let trimmed = input.trim().to_lowercase();
    if trimmed.ends_with("days") || trimmed.ends_with('d') {
        let num_part = trimmed
            .trim_end_matches("days")
            .trim_end_matches('d')
            .trim();
        num_part.parse::<u32>().ok()
    } else if trimmed.ends_with("weeks") || trimmed.ends_with('w') {
        let num_part = trimmed
            .trim_end_matches("weeks")
            .trim_end_matches('w')
            .trim();
        num_part.parse::<u32>().ok().map(|w| w * 7)
    } else if trimmed.ends_with("months") || trimmed.ends_with('m') {
        let num_part = trimmed
            .trim_end_matches("months")
            .trim_end_matches('m')
            .trim();
        num_part.parse::<u32>().ok().map(|m| m * 30)
    } else if trimmed.ends_with("years") || trimmed.ends_with('y') {
        let num_part = trimmed
            .trim_end_matches("years")
            .trim_end_matches('y')
            .trim();
        num_part.parse::<u32>().ok().map(|y| y * 365)
    } else {
        trimmed.parse::<u32>().ok()
    }
}

/// Checks if a scanner name matches a user-provided category.
pub fn matches_category(scanner_name: &str, category: &str) -> bool {
    let cat = category.to_lowercase();
    let name = scanner_name.to_lowercase();

    if name == cat || name.contains(&cat) {
        return true;
    }

    match cat.as_str() {
        "rust" => name == "rust_target" || name == "cargo_cache",
        "cargo" => name == "cargo_cache" || name == "rust_target",
        "node" | "js" | "javascript" | "ts" | "typescript" => {
            name == "node_modules" || name == "npm_cache"
        }
        "npm" => name == "npm_cache" || name == "node_modules",
        "xcode" | "ios" | "swift" => {
            name == "xcode_derived_data" || name == "unavailable_simulators"
        }
        "simulator" | "simulators" | "simctl" => name == "unavailable_simulators",
        "brew" | "homebrew" => name == "brew_cache",
        "cache" | "caches" => {
            name == "brew_cache"
                || name == "cargo_cache"
                || name == "npm_cache"
                || name == "user_caches"
        }
        "user" | "user_cache" | "apps" => name == "user_caches",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_to_days() {
        assert_eq!(parse_duration_to_days("30d"), Some(30));
        assert_eq!(parse_duration_to_days("30"), Some(30));
        assert_eq!(parse_duration_to_days("2w"), Some(14));
        assert_eq!(parse_duration_to_days("1m"), Some(30));
        assert_eq!(parse_duration_to_days("1y"), Some(365));
        assert_eq!(parse_duration_to_days("invalid"), None);
    }

    #[test]
    fn test_matches_category() {
        assert!(matches_category("rust_target", "rust"));
        assert!(matches_category("cargo_cache", "rust"));
        assert!(matches_category("node_modules", "node"));
        assert!(matches_category("npm_cache", "node"));
        assert!(matches_category("xcode_derived_data", "xcode"));
        assert!(matches_category("unavailable_simulators", "simulator"));
        assert!(matches_category("brew_cache", "brew"));
        assert!(matches_category("user_caches", "cache"));
    }
}
