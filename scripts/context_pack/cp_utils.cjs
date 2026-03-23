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
    const tmp = path.join(dir, `.tmp-${path.basename(filePath)}`);
    fs.writeFileSync(tmp, text, 'utf8');
    fs.renameSync(tmp, filePath);
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
};
