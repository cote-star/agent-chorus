const fs = require('fs');
const path = require('path');
const os = require('os');
const https = require('https');
const { spawn } = require('child_process');

const CACHE_DIR = path.join(os.homedir(), '.cache', 'agent-bridge');
const CACHE_FILE = path.join(CACHE_DIR, 'update-check.json');
const LOCK_FILE = path.join(CACHE_DIR, 'update-check.lock');
const REGISTRY_URL = 'https://registry.npmjs.org/agent-bridge/latest';
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000; // 24 hours

// Helper: Ensure cache dir exists
function ensureCacheDir() {
    try {
        fs.mkdirSync(CACHE_DIR, { recursive: true });
    } catch (_) { }
}

// Helper: Read cache safely
function readCache() {
    try {
        const data = fs.readFileSync(CACHE_FILE, 'utf8');
        return JSON.parse(data);
    } catch (_) {
        return null;
    }
}

// Helper: Write cache atomically
function writeCache(data) {
    const tempFile = `${CACHE_FILE}.${process.pid}.tmp`;
    try {
        ensureCacheDir();
        fs.writeFileSync(tempFile, JSON.stringify(data), 'utf8');
        fs.renameSync(tempFile, CACHE_FILE);
    } catch (_) {
        try { fs.unlinkSync(tempFile); } catch (e) { }
    }
}

// Helper: Compare semver (simple numeric split)
// Returns 1 if b > a (update available), 0 if equal, -1 if b < a
function compareVersions(current, latest) {
    try {
        const v1 = current.split('-')[0].split('.').map(Number);
        const v2 = latest.split('-')[0].split('.').map(Number);
        for (let i = 0; i < 3; i++) {
            const n1 = v1[i] || 0;
            const n2 = v2[i] || 0;
            if (n2 > n1) return 1;
            if (n2 < n1) return -1;
        }
        return 0;
    } catch (_) {
        return 0;
    }
}

// Helper: Get current version from package.json
function getCurrentVersion() {
    try {
        // Try to find package.json relative to this script
        const pkgPath = path.resolve(__dirname, '../package.json');
        const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
        return pkg.version;
    } catch (_) {
        return '0.0.0';
    }
}

// Helper: Fetch latest version from registry (Promise-wrapper for https)
function fetchLatestVersion(timeoutMs = 5000) {
    return new Promise((resolve, reject) => {
        const req = https.get(REGISTRY_URL, { timeout: timeoutMs }, (res) => {
            if (res.statusCode !== 200) {
                res.resume();
                return reject(new Error(`Status ${res.statusCode}`));
            }
            let data = '';
            res.on('data', (chunk) => data += chunk);
            res.on('end', () => {
                try {
                    const json = JSON.parse(data);
                    if (json.version) resolve(json.version);
                    else reject(new Error('No version in response'));
                } catch (e) {
                    reject(e);
                }
            });
        });
        req.on('error', reject);
        req.on('timeout', () => {
            req.destroy();
            reject(new Error('Timeout'));
        });
    });
}

// -------------------------------------------------------------------------
// BACKGROUND WORKER LOGIC
// -------------------------------------------------------------------------
if (process.argv[2] === '__update_worker__') {
    (async () => {
        try {
            // 10s total timeout for the worker process
            setTimeout(() => process.exit(0), 10000).unref();

            // Lock file check - if PID in lockfile exists, exit
            if (fs.existsSync(LOCK_FILE)) {
                try {
                    const pid = parseInt(fs.readFileSync(LOCK_FILE, 'utf8'), 10);
                    try {
                        process.kill(pid, 0); // Check if process exists
                        process.exit(0); // It exists, so we exit
                    } catch (e) {
                        // Process doesn't exist, stale lock
                    }
                } catch (_) { }
            }

            // Write lock
            try {
                ensureCacheDir();
                fs.writeFileSync(LOCK_FILE, process.pid.toString(), 'utf8');
            } catch (_) {
                process.exit(0);
            }

            // Fetch
            const latest = await fetchLatestVersion(5000);

            // Update cache
            const currentCache = readCache() || {};
            writeCache({
                latest,
                checked_at: Date.now(),
                last_notified_version: currentCache.last_notified_version
            });

            // Cleanup lock
            try { fs.unlinkSync(LOCK_FILE); } catch (_) { }

        } catch (_) {
            // Fail silently
        }
    })();
    return; // Stop execution of module exports
}


// -------------------------------------------------------------------------
// EXPORTED API
// -------------------------------------------------------------------------

/**
 * Non-blocking check for the main command path.
 * Spawns background worker if cache is stale.
 * Prints banner if update available.
 */
function maybeNotifyUpdate(context) {
    try {
        // 1. Guards
        if (
            context.isJson ||
            !process.stderr.isTTY ||
            process.env.CI === 'true' ||
            process.env.BRIDGE_SKIP_UPDATE_CHECK === '1' ||
            context.command === 'context-pack'
        ) {
            return;
        }

        const cache = readCache();
        const now = Date.now();
        const current = getCurrentVersion();

        // 2. Check Cache
        if (cache && cache.latest && (now - (cache.checked_at || 0)) < CHECK_INTERVAL_MS) {
            // Cache is fresh
            const comparison = compareVersions(current, cache.latest);

            // If update available AND not already notified AND stable version
            if (
                comparison === 1 &&
                cache.last_notified_version !== cache.latest &&
                !cache.latest.includes('-') // Stable only
            ) {
                process.stderr.write(
                    `\nUpdate available: ${current} → ${cache.latest} — run \`npm update -g agent-bridge\`\n\n`
                );

                // Update notification timestamp/version
                cache.last_notified_version = cache.latest;
                writeCache(cache);
            }
            return;
        }

        // 3. Cache Stale/Missing -> Spawn Background Fetch
        // Check lock first to avoid spawn storm
        let locLocked = false;
        try {
            if (fs.existsSync(LOCK_FILE)) {
                const pid = parseInt(fs.readFileSync(LOCK_FILE, 'utf8'), 10);
                process.kill(pid, 0);
                locLocked = true;
            }
        } catch (_) {
            // Lock is stale
        }

        if (!locLocked) {
            const child = spawn(process.execPath, [__filename, '__update_worker__'], {
                detached: true,
                stdio: 'ignore',
                env: { ...process.env, BRIDGE_SKIP_UPDATE_CHECK: undefined } // Ensure worker not skipped if env differs (though usually it inherits)
            });
            child.unref();
        }

    } catch (_) {
        // Fail silent
    }
}

/**
 * Synchronous check for 'bridge doctor'.
 * Blocks to fetch registry.
 * Returns structured status.
 */
function checkNowForDoctor() {
    const current = getCurrentVersion();
    const result = {
        current,
        latest: null,
        up_to_date: true,
        error: null
    };

    try {
        // We can't use the async fetchLatestVersion here easily since we need to be synchronous?
        // Wait, the spec says "Synchronous HTTP to registry".
        // Node.js doesn't have a native synchronous HTTP client.
        // However, we are in a 'scripts' context, maybe we can spawn a child synchronously?
        // Or just use spawnSync with curl/wget? No, can't rely on those.
        // actually, `child_process.spawnSync` calling this script in a special mode that prints to stdout?

        // Let's use the spawnSync trick to run our own async fetcher and wait for it.
        const child = require('child_process').spawnSync(process.execPath, [__filename, '__update_worker_sync__'], {
            encoding: 'utf8',
            timeout: 10000
        });

        if (child.error) {
            throw child.error;
        }
        if (child.status !== 0) {
            throw new Error(child.stderr || 'Worker failed');
        }

        const output = JSON.parse(child.stdout);
        if (output.error) throw new Error(output.error);

        result.latest = output.latest;
        result.up_to_date = compareVersions(current, result.latest) < 1;

        // Refresh cache implicitly via the worker or explicitly here?
        // Let's write cache here if successful
        if (result.latest) {
            const cache = readCache() || {};
            writeCache({
                latest: result.latest,
                checked_at: Date.now(),
                last_notified_version: cache.last_notified_version
            });
        }

    } catch (e) {
        result.error = e.message || 'registry unreachable';
        // On error, try to use cached value if freshish? No, doctor asks for "now".
        // But maybe fallback to cache if available? Spec says "Offline: ... error"
    }

    return result;
}

// -------------------------------------------------------------------------
// SYNCHRONOUS WORKER (for doctor)
// -------------------------------------------------------------------------
if (process.argv[2] === '__update_worker_sync__') {
    (async () => {
        try {
            const latest = await fetchLatestVersion(8000);
            process.stdout.write(JSON.stringify({ latest }));
        } catch (e) {
            process.stdout.write(JSON.stringify({ error: e.message }));
        }
    })();
    return;
}

module.exports = {
    maybeNotifyUpdate,
    checkNowForDoctor
};
