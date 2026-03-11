use anyhow::{Result};
use std::fs;
use std::path::{PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use std::io::IsTerminal;

const REGISTRY_URL: &str = "https://registry.npmjs.org/agent-bridge/latest";
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

#[derive(Debug)]
struct Cache {
    latest: String,
    checked_at: u64,
    last_notified_version: Option<String>,
}

pub struct UpdateStatus {
    pub current: String,
    pub latest: Option<String>,
    pub up_to_date: bool,
    pub error: Option<String>,
}

pub fn maybe_notify_update(is_json: bool, command: &str) {
    // 1. Guards
    if is_json
        || !std::io::stderr().is_terminal()
        || std::env::var("CI").is_ok()
        || std::env::var("BRIDGE_SKIP_UPDATE_CHECK").unwrap_or_default() == "1"
        || command == "context-pack"
    {
        return;
    }

    let cache_dir = match dirs::cache_dir() {
        Some(d) => d.join("agent-bridge"),
        None => return,
    };
    let cache_file = cache_dir.join("update-check.json");
    let lock_file = cache_dir.join("update-check.lock");

    // 2. Check Cache
    if let Ok(cache) = read_cache(&cache_file) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if (now.saturating_sub(cache.checked_at)) < CHECK_INTERVAL_SECS {
            let current = env!("CARGO_PKG_VERSION");
            if compare_versions(current, &cache.latest) == 1
                && cache.last_notified_version.as_deref() != Some(&cache.latest)
                && !cache.latest.contains('-')
            {
                eprintln!(
                    "\nUpdate available: {} → {} — run `npm update -g agent-bridge`\n",
                    current, cache.latest
                );
                
                // Update last_notified_version
                let new_cache = Cache {
                    last_notified_version: Some(cache.latest.clone()),
                    ..cache
                };
                let _ = write_cache(&cache_file, &new_cache);
            }
            return;
        }
    }

    // 3. Cache Stale/Missing -> Spawn Background Fetch
    // Check lock
    if is_locked(&lock_file) {
        return;
    }

    // Spawn hidden worker
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe)
            .arg("update-worker") // Hidden subcommand
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

pub fn check_now_for_doctor() -> UpdateStatus {
    let current = env!("CARGO_PKG_VERSION").to_string();
    
    match fetch_latest_version(std::time::Duration::from_secs(5)) {
        Ok(latest) => {
            let up_to_date = compare_versions(&current, &latest) < 1;
            
            // Update cache
            let cache_dir = dirs::cache_dir().map(|d| d.join("agent-bridge"));
            if let Some(dir) = cache_dir {
                let cache_file = dir.join("update-check.json");
                let last_notified = read_cache(&cache_file).ok().and_then(|c| c.last_notified_version);
                
                let cache = Cache {
                    latest: latest.clone(),
                    checked_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                    last_notified_version: last_notified,
                };
                let _ = write_cache(&cache_file, &cache);
            }

            UpdateStatus {
                current,
                latest: Some(latest),
                up_to_date,
                error: None,
            }
        }
        Err(e) => UpdateStatus {
            current,
            latest: None,
            up_to_date: true,
            error: Some(e.to_string()),
        },
    }
}

pub fn run_worker() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(10));
        std::process::exit(0);
    });

    let cache_dir = match dirs::cache_dir() {
        Some(d) => d.join("agent-bridge"),
        None => return,
    };
    let lock_file = cache_dir.join("update-check.lock");
    let cache_file = cache_dir.join("update-check.json");

    if is_locked(&lock_file) {
        return;
    }

    let _ = fs::create_dir_all(&cache_dir);
    let _ = fs::write(&lock_file, std::process::id().to_string());

    if let Ok(latest) = fetch_latest_version(std::time::Duration::from_secs(5)) {
        let last_notified = read_cache(&cache_file).ok().and_then(|c| c.last_notified_version);
        let cache = Cache {
            latest,
            checked_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            last_notified_version: last_notified,
        };
        let _ = write_cache(&cache_file, &cache);
    }

    let _ = fs::remove_file(lock_file);
}

fn fetch_latest_version(timeout: std::time::Duration) -> Result<String> {
    let resp = ureq::get(REGISTRY_URL)
        .timeout(timeout)
        .call()?;
    
    let json: serde_json::Value = resp.into_json()?;
    json.get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("No version field"))
}

fn read_cache(path: &PathBuf) -> Result<Cache> {
    let content = fs::read_to_string(path)?;
    let v: serde_json::Value = serde_json::from_str(&content)?;
    Ok(Cache {
        latest: v["latest"].as_str().unwrap_or("").to_string(),
        checked_at: v["checked_at"].as_u64().unwrap_or(0),
        last_notified_version: v["last_notified_version"].as_str().map(|s| s.to_string()),
    })
}

fn write_cache(path: &PathBuf, cache: &Cache) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    let json = serde_json::json!({
        "latest": cache.latest,
        "checked_at": cache.checked_at,
        "last_notified_version": cache.last_notified_version
    });
    fs::write(&temp, serde_json::to_string(&json)?)?;
    fs::rename(temp, path)?;
    Ok(())
}

fn is_locked(path: &PathBuf) -> bool {
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            // Check if process exists via kill -0
            if Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

fn compare_versions(current: &str, latest: &str) -> i32 {
    let parse = |v: &str| {
        v.split('-')
            .next()
            .unwrap_or(v)
            .split('.')
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    
    let v1 = parse(current);
    let v2 = parse(latest);
    
    for i in 0..3 {
        let n1 = v1.get(i).copied().unwrap_or(0);
        let n2 = v2.get(i).copied().unwrap_or(0);
        if n2 > n1 { return 1; }
        if n2 < n1 { return -1; }
    }
    0
}
