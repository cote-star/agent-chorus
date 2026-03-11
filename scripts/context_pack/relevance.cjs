#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');

// ---------------------------------------------------------------------------
// Default relevance configuration (matches init.cjs defaults)
// ---------------------------------------------------------------------------
const DEFAULT_CONFIG = {
    include: ['**'],
    exclude: [
        '.agent-context/**',
        '.git/**',
        'node_modules/**',
        'target/**',
        'dist/**',
        'build/**',
        'vendor/**',
        'tmp/**',
    ],
};

// ---------------------------------------------------------------------------
// Glob matching using Node.js built-ins only
// ---------------------------------------------------------------------------

/**
 * Convert a glob pattern to a RegExp.
 *
 * Supported syntax:
 *   **  — matches zero or more path segments (any depth)
 *   *   — matches any characters within a single path segment (no /)
 *   ?   — matches exactly one non-/ character
 *
 * All other characters are escaped for literal matching.
 *
 * @param {string} pattern  Glob pattern (forward-slash separated)
 * @returns {RegExp}
 */
function globToRegex(pattern) {
    let i = 0;
    let regex = '^';
    const len = pattern.length;

    while (i < len) {
        const ch = pattern[i];

        if (ch === '*') {
            if (pattern[i + 1] === '*') {
                // '**' — match any depth
                // Consume optional trailing slash so `foo/**/bar` works
                i += 2;
                if (pattern[i] === '/') {
                    i += 1;
                    // ** followed by / — match zero-or-more directories
                    regex += '(?:.+/)?';
                } else {
                    // trailing ** — match anything remaining
                    regex += '.*';
                }
            } else {
                // '*' — match within a single segment (no /)
                regex += '[^/]*';
                i += 1;
            }
        } else if (ch === '?') {
            regex += '[^/]';
            i += 1;
        } else if (ch === '.') {
            regex += '\\.';
            i += 1;
        } else if (ch === '(' || ch === ')' || ch === '{' || ch === '}' ||
            ch === '[' || ch === ']' || ch === '+' || ch === '^' ||
            ch === '$' || ch === '|' || ch === '\\') {
            regex += '\\' + ch;
            i += 1;
        } else {
            regex += ch;
            i += 1;
        }
    }

    regex += '$';
    return new RegExp(regex);
}

/**
 * Test whether a file path matches a glob pattern.
 *
 * @param {string} filePath  Forward-slash normalized, repo-relative path
 * @param {string} pattern   Glob pattern
 * @returns {boolean}
 */
function matchGlob(filePath, pattern) {
    return globToRegex(pattern).test(filePath);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Load relevance configuration from `.agent-context/relevance.json`.
 *
 * @param {string} packRoot  Absolute path to the repository root
 * @returns {{ include: string[], exclude: string[] }}
 */
function loadRelevanceConfig(packRoot) {
    const configPath = path.join(packRoot, '.agent-context', 'relevance.json');

    let raw;
    try {
        raw = fs.readFileSync(configPath, 'utf8');
    } catch (_err) {
        // Missing file — return defaults silently
        return { ...DEFAULT_CONFIG };
    }

    try {
        const parsed = JSON.parse(raw);
        const include = Array.isArray(parsed.include) ? parsed.include : DEFAULT_CONFIG.include;
        const exclude = Array.isArray(parsed.exclude) ? parsed.exclude : DEFAULT_CONFIG.exclude;
        return { include, exclude };
    } catch (_err) {
        process.stderr.write(
            `[relevance] WARNING: invalid JSON in ${configPath}, using defaults\n`
        );
        return { ...DEFAULT_CONFIG };
    }
}

/**
 * Determine whether a file path is relevant given a relevance config.
 *
 * Evaluation order:
 *   1. If filePath matches any exclude pattern → NOT relevant
 *   2. Else if filePath matches any include pattern → relevant
 *   3. Else → NOT relevant
 *
 * @param {string} filePath  Repo-relative path (forward-slash normalized)
 * @param {{ include: string[], exclude: string[] }} config
 * @returns {boolean}
 */
function isRelevant(filePath, config) {
    const normalized = filePath.replace(/\\/g, '/');

    for (const pattern of config.exclude) {
        if (matchGlob(normalized, pattern)) {
            return false;
        }
    }

    for (const pattern of config.include) {
        if (matchGlob(normalized, pattern)) {
            return true;
        }
    }

    return false;
}

/**
 * Filter an array of file paths to only those that are relevant.
 *
 * @param {string[]} files   Array of repo-relative file paths
 * @param {{ include: string[], exclude: string[] }} config
 * @returns {string[]}
 */
function filterRelevantFiles(files, config) {
    return files.filter((f) => isRelevant(f, config));
}

// ---------------------------------------------------------------------------
// Self-test (run via: node relevance.cjs --self-test)
// ---------------------------------------------------------------------------

function selfTest() {
    let passed = 0;
    let failed = 0;

    function assert(label, actual, expected) {
        if (actual === expected) {
            passed += 1;
        } else {
            failed += 1;
            process.stderr.write(`  FAIL: ${label} — expected ${expected}, got ${actual}\n`);
        }
    }

    process.stdout.write('[relevance] running self-tests…\n');

    // --- Test 1: Default config behavior ---
    const defaults = { ...DEFAULT_CONFIG };

    assert('default: src/index.js is relevant', isRelevant('src/index.js', defaults), true);
    assert('default: README.md is relevant', isRelevant('README.md', defaults), true);
    assert('default: node_modules/foo/bar.js excluded', isRelevant('node_modules/foo/bar.js', defaults), false);
    assert('default: .git/config excluded', isRelevant('.git/config', defaults), false);
    assert('default: .agent-context/relevance.json excluded', isRelevant('.agent-context/relevance.json', defaults), false);
    assert('default: target/debug/main excluded', isRelevant('target/debug/main', defaults), false);
    assert('default: dist/bundle.js excluded', isRelevant('dist/bundle.js', defaults), false);
    assert('default: build/output.js excluded', isRelevant('build/output.js', defaults), false);
    assert('default: vendor/lib.js excluded', isRelevant('vendor/lib.js', defaults), false);
    assert('default: tmp/scratch.txt excluded', isRelevant('tmp/scratch.txt', defaults), false);

    // --- Test 2: Custom config ---
    const custom = {
        include: ['src/**', 'lib/**', '*.md'],
        exclude: ['src/deprecated/**', '**/*.test.js'],
    };

    assert('custom: src/index.js relevant', isRelevant('src/index.js', custom), true);
    assert('custom: src/deprecated/old.js excluded', isRelevant('src/deprecated/old.js', custom), false);
    assert('custom: src/utils.test.js excluded', isRelevant('src/utils.test.js', custom), false);
    assert('custom: lib/helper.js relevant', isRelevant('lib/helper.js', custom), true);
    assert('custom: README.md relevant', isRelevant('README.md', custom), true);
    assert('custom: docs/guide.txt not included', isRelevant('docs/guide.txt', custom), false);
    assert('custom: deeply nested test excluded', isRelevant('lib/deep/thing.test.js', custom), false);

    // --- Test 3: Invalid config fallback ---
    // Simulate by calling loadRelevanceConfig on a non-existent directory
    const fallback = loadRelevanceConfig('/nonexistent_path_for_test');
    assert('fallback: include is default', JSON.stringify(fallback.include), JSON.stringify(DEFAULT_CONFIG.include));
    assert('fallback: exclude is default', JSON.stringify(fallback.exclude), JSON.stringify(DEFAULT_CONFIG.exclude));

    // --- Test 4: Fixture-based tests (if available) ---
    const fixtureDir = path.join(__dirname, '..', '..', 'fixtures', 'golden', 'relevance');
    const configFixture = path.join(fixtureDir, 'test_config.json');
    const filesFixture = path.join(fixtureDir, 'test_files.json');

    if (fs.existsSync(configFixture) && fs.existsSync(filesFixture)) {
        const config = JSON.parse(fs.readFileSync(configFixture, 'utf8'));
        const testCases = JSON.parse(fs.readFileSync(filesFixture, 'utf8'));
        for (const tc of testCases) {
            assert(`fixture: ${tc.path}`, isRelevant(tc.path, config), tc.relevant);
        }
    }

    // --- Summary ---
    process.stdout.write(`[relevance] self-test complete: ${passed} passed, ${failed} failed\n`);
    process.exit(failed > 0 ? 1 : 0);
}

// ---------------------------------------------------------------------------
// Main entry / CLI
// ---------------------------------------------------------------------------

if (require.main === module) {
    if (process.argv.includes('--self-test')) {
        selfTest();
    } else {
        process.stdout.write('Usage: node relevance.cjs --self-test\n');
    }
}

module.exports = { loadRelevanceConfig, isRelevant, filterRelevantFiles, DEFAULT_CONFIG };
