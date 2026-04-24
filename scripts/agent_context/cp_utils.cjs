'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

function runGit(args, cwd, allowFailure = false) {
    try {
        return execFileSync('git', args, {
            cwd,
            encoding: 'utf8',
            stdio: ['ignore', 'pipe', 'pipe'],
        }).trim();
    } catch (error) {
        if (allowFailure) return '';
        throw error;
    }
}

function ensureDir(dirPath) {
    fs.mkdirSync(dirPath, { recursive: true });
}

function isProcessRunning(pid) {
    try {
        process.kill(pid, 0);
        return true;
    } catch (_) {
        return false;
    }
}

function checkSymlink(filePath) {
    try {
        const stats = fs.lstatSync(filePath);
        if (stats.isSymbolicLink()) {
            throw new Error(`Refusing to write: target is a symlink: ${filePath}`);
        }
    } catch (error) {
        if (error.code !== 'ENOENT') {
            throw error;
        }
    }
}

function safeWriteText(filePath, text) {
    checkSymlink(filePath);
    ensureDir(path.dirname(filePath));
    fs.writeFileSync(filePath, text, 'utf8');
}

function safeWriteTextAtomic(filePath, text) {
    checkSymlink(filePath);
    const dir = path.dirname(filePath);
    ensureDir(dir);
    const tmp = path.join(dir, `.tmp-${path.basename(filePath)}.${process.pid}`);
    const fd = fs.openSync(tmp, 'w');
    try {
        fs.writeFileSync(fd, text, 'utf8');
        try {
            fs.fsyncSync(fd);
        } catch (_) {
            /* fsync unsupported on some FS — tolerate */
        }
    } finally {
        fs.closeSync(fd);
    }
    fs.renameSync(tmp, filePath);
}

/**
 * Append an entry to a JSONL file with opportunistic rotation (F55).
 * Rotates to `${base}.N` + rewrites history_index.json when the active file
 * exceeds 5MB or 1000 lines. Rotation is performed under the seal lock held
 * by the caller, so no additional synchronization is needed here.
 */
const HISTORY_ROTATE_MAX_BYTES = 5 * 1024 * 1024;
const HISTORY_ROTATE_MAX_ENTRIES = 1000;

function rotateHistoryIfNeeded(historyPath) {
    if (!fs.existsSync(historyPath)) return;
    let stat;
    try {
        stat = fs.statSync(historyPath);
    } catch (_) {
        return;
    }
    const size = stat.size;
    let lineCount = 0;
    if (size > 0) {
        const raw = fs.readFileSync(historyPath, 'utf8');
        for (const line of raw.split('\n')) {
            if (line.trim().length > 0) lineCount += 1;
        }
    }
    if (size < HISTORY_ROTATE_MAX_BYTES && lineCount < HISTORY_ROTATE_MAX_ENTRIES) return;

    const dir = path.dirname(historyPath);
    const baseName = path.basename(historyPath);

    let nextIndex = 1;
    for (const name of fs.readdirSync(dir)) {
        if (name.startsWith(`${baseName}.`)) {
            const tail = name.slice(baseName.length + 1);
            const n = parseInt(tail, 10);
            if (!Number.isNaN(n) && n >= nextIndex) nextIndex = n + 1;
        }
    }

    let firstId = null;
    let lastId = null;
    let entries = 0;
    try {
        const raw = fs.readFileSync(historyPath, 'utf8');
        for (const line of raw.split('\n')) {
            const trimmed = line.trim();
            if (!trimmed) continue;
            entries += 1;
            try {
                const obj = JSON.parse(trimmed);
                if (obj && typeof obj.snapshot_id === 'string') {
                    if (firstId === null) firstId = obj.snapshot_id;
                    lastId = obj.snapshot_id;
                }
            } catch (_) {
                /* skip malformed line */
            }
        }
    } catch (_) {
        /* ignore */
    }

    const rotatedName = `${baseName}.${nextIndex}`;
    const rotatedPath = path.join(dir, rotatedName);
    fs.renameSync(historyPath, rotatedPath);
    safeWriteTextAtomic(historyPath, '');

    const indexPath = path.join(dir, 'history_index.json');
    let filesArr = [];
    if (fs.existsSync(indexPath)) {
        try {
            const existing = JSON.parse(fs.readFileSync(indexPath, 'utf8'));
            if (existing && Array.isArray(existing.files)) filesArr = existing.files;
        } catch (_) {
            /* ignore malformed prior index */
        }
    }
    filesArr.push({
        name: rotatedName,
        first_id: firstId,
        last_id: lastId,
        entries,
    });
    const indexValue = {
        schema_version: 1,
        active: baseName,
        files: filesArr,
    };
    safeWriteTextAtomic(indexPath, `${JSON.stringify(indexValue, null, 2)}\n`);
    try {
        process.stderr.write(
            `[context-pack] rotated history: ${baseName} -> ${rotatedName} (${entries} entries, ${size} bytes)\n`
        );
    } catch (_) {
        /* ignore */
    }
}

function appendJsonl(filePath, value) {
    ensureDir(path.dirname(filePath));
    rotateHistoryIfNeeded(filePath);
    fs.appendFileSync(filePath, `${JSON.stringify(value)}\n`, 'utf8');
}

/**
 * Upsert a managed block into a file (prepend if new, replace if exists).
 * Block is delimited by HTML comment markers:
 *   <!-- {markerPrefix}:start -->  ...  <!-- {markerPrefix}:end -->
 *
 * Idempotent — running twice produces the same result.
 */
function upsertContextPackBlock(filePath, block, markerPrefix) {
    const startMarker = `<!-- ${markerPrefix}:start -->`;
    const endMarker = `<!-- ${markerPrefix}:end -->`;
    const managedBlock = `${startMarker}\n${block}\n${endMarker}`;

    if (fs.existsSync(filePath)) {
        let content = fs.readFileSync(filePath, 'utf8');
        const startIdx = content.indexOf(startMarker);
        const endIdx = content.indexOf(endMarker);
        if (startIdx !== -1 && endIdx !== -1) {
            // Replace existing managed block in place
            content = content.slice(0, startIdx) + managedBlock + content.slice(endIdx + endMarker.length);
        } else {
            // Prepend before existing content
            content = managedBlock + '\n\n' + content;
        }
        safeWriteText(filePath, content);
    } else {
        // Create file with just the block
        safeWriteText(filePath, managedBlock + '\n');
    }
}

module.exports = {
    runGit,
    ensureDir,
    isProcessRunning,
    safeWriteText,
    safeWriteTextAtomic,
    upsertContextPackBlock,
    appendJsonl,
    rotateHistoryIfNeeded,
};
