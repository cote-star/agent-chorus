//! Generic relevance engine for context-pack.
//!
//! Provides configurable include/exclude glob matching against file paths,
//! mirroring the Node.js implementation in `scripts/context_pack/relevance.cjs`.

use globset::{Glob, GlobMatcher};
use std::fs;
use std::path::Path;

/// Default include patterns when no config is found.
const DEFAULT_INCLUDE: &[&str] = &["**"];

/// Default exclude patterns when no config is found.
const DEFAULT_EXCLUDE: &[&str] = &[
    ".agent-context/**",
    ".git/**",
    "node_modules/**",
    "target/**",
    "dist/**",
    "build/**",
    "vendor/**",
    "tmp/**",
];

/// Compiled relevance configuration.
pub struct RelevanceConfig {
    include: Vec<GlobMatcher>,
    exclude: Vec<GlobMatcher>,
}

/// Create a [`GlobMatcher`] from a pattern string, returning `None` on invalid patterns.
fn compile_glob(pattern: &str) -> Option<GlobMatcher> {
    Glob::new(pattern).ok().map(|g| g.compile_matcher())
}

/// Compile a list of pattern strings into [`GlobMatcher`]s, skipping any that fail to parse.
fn compile_patterns(patterns: &[String]) -> Vec<GlobMatcher> {
    patterns.iter().filter_map(|p| compile_glob(p)).collect()
}

fn default_include_matchers() -> Vec<GlobMatcher> {
    DEFAULT_INCLUDE
        .iter()
        .filter_map(|p| compile_glob(p))
        .collect()
}

fn default_exclude_matchers() -> Vec<GlobMatcher> {
    DEFAULT_EXCLUDE
        .iter()
        .filter_map(|p| compile_glob(p))
        .collect()
}

impl Default for RelevanceConfig {
    fn default() -> Self {
        Self {
            include: default_include_matchers(),
            exclude: default_exclude_matchers(),
        }
    }
}

/// Load relevance configuration from `.agent-context/relevance.json` under `pack_root`.
///
/// - Missing file → return defaults silently.
/// - Invalid JSON → warn to stderr, return defaults.
pub fn load_relevance_config(pack_root: &Path) -> RelevanceConfig {
    let config_path = pack_root.join(".agent-context").join("relevance.json");

    let raw = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return RelevanceConfig::default(),
    };

    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            eprintln!(
                "[relevance] WARNING: invalid JSON in {}, using defaults",
                config_path.display()
            );
            return RelevanceConfig::default();
        }
    };

    let include_strs: Vec<String> = parsed
        .get("include")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| DEFAULT_INCLUDE.iter().map(|s| s.to_string()).collect());

    let exclude_strs: Vec<String> = parsed
        .get("exclude")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect());

    RelevanceConfig {
        include: compile_patterns(&include_strs),
        exclude: compile_patterns(&exclude_strs),
    }
}

/// Determine whether a file path is relevant given a relevance config.
///
/// Evaluation order:
///   1. If `file_path` matches any exclude pattern → **not** relevant.
///   2. Else if `file_path` matches any include pattern → relevant.
///   3. Else → **not** relevant.
///
/// `file_path` should be repo-relative with forward slashes.
pub fn is_relevant(file_path: &str, config: &RelevanceConfig) -> bool {
    let normalized = file_path.replace('\\', "/");

    for m in &config.exclude {
        if m.is_match(&normalized) {
            return false;
        }
    }

    for m in &config.include {
        if m.is_match(&normalized) {
            return true;
        }
    }

    false
}

/// Convenience wrapper: filter a list of file paths to only those that are relevant.
pub fn filter_relevant_files(files: &[String], config: &RelevanceConfig) -> Vec<String> {
    files
        .iter()
        .filter(|f| is_relevant(f, config))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_includes_normal_files() {
        let config = RelevanceConfig::default();
        assert!(is_relevant("src/index.js", &config));
        assert!(is_relevant("README.md", &config));
    }

    #[test]
    fn default_config_excludes_standard_dirs() {
        let config = RelevanceConfig::default();
        assert!(!is_relevant("node_modules/foo/bar.js", &config));
        assert!(!is_relevant(".git/config", &config));
        assert!(!is_relevant(".agent-context/relevance.json", &config));
        assert!(!is_relevant("target/debug/main", &config));
        assert!(!is_relevant("dist/bundle.js", &config));
        assert!(!is_relevant("build/output.js", &config));
        assert!(!is_relevant("vendor/lib.js", &config));
        assert!(!is_relevant("tmp/scratch.txt", &config));
    }

    #[test]
    fn custom_config_works() {
        let config = RelevanceConfig {
            include: vec!["src/**", "lib/**", "*.md"]
                .into_iter()
                .filter_map(compile_glob)
                .collect(),
            exclude: vec!["src/deprecated/**", "**/*.test.js"]
                .into_iter()
                .filter_map(compile_glob)
                .collect(),
        };

        assert!(is_relevant("src/index.js", &config));
        assert!(!is_relevant("src/deprecated/old.js", &config));
        assert!(!is_relevant("src/utils.test.js", &config));
        assert!(is_relevant("lib/helper.js", &config));
        assert!(is_relevant("README.md", &config));
        assert!(!is_relevant("docs/guide.txt", &config));
        assert!(!is_relevant("lib/deep/thing.test.js", &config));
    }

    #[test]
    fn filter_relevant_files_works() {
        let config = RelevanceConfig::default();
        let files = vec![
            "src/main.rs".to_string(),
            "node_modules/foo.js".to_string(),
            "README.md".to_string(),
        ];
        let result = filter_relevant_files(&files, &config);
        assert_eq!(result, vec!["src/main.rs", "README.md"]);
    }
}
