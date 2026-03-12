use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn expand_home(path_str: &str) -> Option<PathBuf> {
    if path_str == "~" {
        return dirs::home_dir();
    }
    if let Some(stripped) = path_str.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return Some(home.join(stripped));
        }
        return None;
    }
    Some(PathBuf::from(path_str))
}

pub fn normalize_path(path_str: &str) -> Result<PathBuf> {
    let expanded = expand_home(path_str).context("Could not expand home directory")?;
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .context("Could not resolve current directory")?
            .join(expanded)
    };

    absolute.canonicalize().or_else(|_| Ok(absolute))
}

pub fn hash_path(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Strip terminal escape sequences and C0 control characters from text.
/// Preserves \n (0x0A), \t (0x09), and \r (0x0D).
pub fn sanitize_for_terminal(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '\x1B' {
            // ESC sequence
            i += 1;
            if i < chars.len() {
                if chars[i] == '[' {
                    // CSI sequence: skip until letter
                    i += 1;
                    while i < chars.len() && !chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    if i < chars.len() { i += 1; } // skip the final letter
                } else if chars[i] == ']' {
                    // OSC sequence: skip until BEL (0x07) or ST (ESC\)
                    i += 1;
                    while i < chars.len() {
                        if chars[i] == '\x07' { i += 1; break; }
                        if chars[i] == '\x1B' && i + 1 < chars.len() && chars[i + 1] == '\\' {
                            i += 2; break;
                        }
                        i += 1;
                    }
                } else {
                    // Other ESC sequence: skip one char
                    i += 1;
                }
            }
            continue;
        }

        // C0 control characters (0x00-0x1F), except \t (0x09), \n (0x0A), \r (0x0D)
        let code = ch as u32;
        if code <= 0x1F && code != 0x09 && code != 0x0A && code != 0x0D {
            i += 1;
            continue;
        }

        output.push(ch);
        i += 1;
    }

    output
}
