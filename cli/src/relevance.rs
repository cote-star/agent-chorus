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

// ---------------------------------------------------------------------------
// Introspection API (Phase 2 innovation)
// ---------------------------------------------------------------------------

/// Result of `list_patterns`: current include/exclude patterns and their source.
#[derive(Debug, serde::Serialize)]
pub struct PatternsInfo {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Either the absolute path to relevance.json or "defaults".
    pub source: String,
}

/// List current include/exclude patterns for a given repo root.
pub fn list_patterns(cwd: &Path) -> PatternsInfo {
    let config_path = cwd.join(".agent-context").join("relevance.json");

    if let Ok(raw) = fs::read_to_string(&config_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
            let include = parsed
                .get("include")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| DEFAULT_INCLUDE.iter().map(|s| s.to_string()).collect());
            let exclude = parsed
                .get("exclude")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect());
            return PatternsInfo {
                include,
                exclude,
                source: config_path.to_string_lossy().to_string(),
            };
        }
    }

    PatternsInfo {
        include: DEFAULT_INCLUDE.iter().map(|s| s.to_string()).collect(),
        exclude: DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect(),
        source: "defaults".to_string(),
    }
}

/// Result of `test_file`: whether a path is relevant and which pattern matched.
#[derive(Debug, serde::Serialize)]
pub struct TestFileResult {
    pub path: String,
    pub relevant: bool,
    pub matched_by: Option<String>,
}

/// Test whether a specific file path is relevant, returning the matching pattern.
pub fn test_file(cwd: &Path, file_path: &str) -> TestFileResult {
    let info = list_patterns(cwd);
    let normalized = file_path.replace('\\', "/");

    for pattern in &info.exclude {
        if let Some(matcher) = compile_glob(pattern) {
            if matcher.is_match(&normalized) {
                return TestFileResult {
                    path: file_path.to_string(),
                    relevant: false,
                    matched_by: Some(format!("exclude: {}", pattern)),
                };
            }
        }
    }

    for pattern in &info.include {
        if let Some(matcher) = compile_glob(pattern) {
            if matcher.is_match(&normalized) {
                return TestFileResult {
                    path: file_path.to_string(),
                    relevant: true,
                    matched_by: Some(format!("include: {}", pattern)),
                };
            }
        }
    }

    TestFileResult {
        path: file_path.to_string(),
        relevant: false,
        matched_by: None,
    }
}

/// A suggested pattern with reason and type.
#[derive(Debug, serde::Serialize)]
pub struct PatternSuggestion {
    pub pattern: String,
    pub reason: String,
    #[serde(rename = "type")]
    pub suggestion_type: String,
}

/// Suggest patterns based on common project conventions detected in cwd.
pub fn suggest_patterns(cwd: &Path) -> Vec<PatternSuggestion> {
    let checks: Vec<(&str, Option<&str>, &str, &str, &str)> = vec![
        // (dir_check, file_check, pattern, reason, type)
        ("coverage", None, "coverage/**", "Test coverage output", "exclude"),
        (".next", None, ".next/**", "Next.js build output", "exclude"),
        (".nuxt", None, ".nuxt/**", "Nuxt build output", "exclude"),
        ("__pycache__", None, "__pycache__/**", "Python cache", "exclude"),
        (".pytest_cache", None, ".pytest_cache/**", "Pytest cache", "exclude"),
        (".venv", None, ".venv/**", "Python virtualenv", "exclude"),
        ("venv", None, "venv/**", "Python virtualenv", "exclude"),
        (".turbo", None, ".turbo/**", "Turborepo cache", "exclude"),
        (".cargo", None, ".cargo/**", "Cargo local config", "exclude"),
    ];

    let file_checks: Vec<(Option<&str>, &str, &str, &str, &str)> = vec![
        (None, "Dockerfile", "Dockerfile*", "Docker config (include for infra context)", "include"),
        (None, "docker-compose.yml", "docker-compose*.yml", "Docker Compose config", "include"),
    ];

    let mut suggestions = Vec::new();

    for (dir, _, pattern, reason, stype) in &checks {
        if cwd.join(dir).exists() {
            suggestions.push(PatternSuggestion {
                pattern: pattern.to_string(),
                reason: reason.to_string(),
                suggestion_type: stype.to_string(),
            });
        }
    }

    for (_, file, pattern, reason, stype) in &file_checks {
        if cwd.join(file).exists() {
            suggestions.push(PatternSuggestion {
                pattern: pattern.to_string(),
                reason: reason.to_string(),
                suggestion_type: stype.to_string(),
            });
        }
    }

    suggestions
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
