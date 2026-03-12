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

module.exports = {
    runGit,
    ensureDir,
    isProcessRunning,
    safeWriteText,
    safeWriteTextAtomic,
};
