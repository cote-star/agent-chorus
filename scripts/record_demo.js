#!/usr/bin/env node
/**
 * Record a demo HTML file as an animated WebP using Puppeteer + img2webp.
 *
 * Usage:
 *   node scripts/record_demo.js
 *   node scripts/record_demo.js --input fixtures/demo/player.html --output docs/demo.webp
 *   node scripts/record_demo.js --input fixtures/demo/player-skill-setup.html --output docs/demo-skill.webp
 *
 * Requirements:
 *   npm install --save-dev puppeteer
 *   img2webp on PATH (brew install webp)
 */

const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const { execFileSync, execSync } = require('child_process');
const os = require('os');

const DEFAULT_VIEWPORT = { width: 1080, height: 640 };
const DEFAULT_FPS = 10;
const DEFAULT_DURATION_MS = 24000;

function getArgValue(name, fallback = null) {
    const args = process.argv.slice(2);
    const prefix = `${name}=`;
    for (let i = 0; i < args.length; i += 1) {
        const arg = args[i];
        if (arg === name && i + 1 < args.length) return args[i + 1];
        if (arg.startsWith(prefix)) return arg.slice(prefix.length);
    }
    return fallback;
}

(async () => {
    const inputArg = getArgValue('--input', path.join('fixtures', 'demo', 'player.html'));
    const outputArg = getArgValue('--output', path.join('docs', 'demo.webp'));
    const durationMs = Number(getArgValue('--duration-ms', String(DEFAULT_DURATION_MS)));
    const fps = Number(getArgValue('--fps', String(DEFAULT_FPS)));
    const width = Number(getArgValue('--width', String(DEFAULT_VIEWPORT.width)));
    const height = Number(getArgValue('--height', String(DEFAULT_VIEWPORT.height)));
    const frameInterval = 1000 / fps;

    const htmlPath = path.resolve(__dirname, '..', inputArg);
    const outFile = path.resolve(__dirname, '..', outputArg);

    if (!fs.existsSync(htmlPath)) {
        console.error(`Input HTML not found: ${htmlPath}`);
        process.exit(1);
    }

    if (!Number.isFinite(durationMs) || durationMs <= 0) {
        console.error(`Invalid --duration-ms: ${durationMs}`);
        process.exit(1);
    }
    if (!Number.isFinite(fps) || fps <= 0) {
        console.error(`Invalid --fps: ${fps}`);
        process.exit(1);
    }
    if (!Number.isFinite(width) || width <= 0 || !Number.isFinite(height) || height <= 0) {
        console.error(`Invalid --width/--height: ${width}x${height}`);
        process.exit(1);
    }

    // Verify img2webp is available
    try {
        execSync('which img2webp', { stdio: 'ignore' });
    } catch {
        console.error('img2webp not found. Install with: brew install webp');
        process.exit(1);
    }

    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'chorus-demo-'));

    console.log('Launching browser...');
    const browser = await puppeteer.launch({
        headless: 'new',
        args: ['--no-sandbox', '--disable-setuid-sandbox'],
    });

    const page = await browser.newPage();
    await page.setViewport({ width, height });

    console.log('Loading', htmlPath);
    await page.goto('file://' + htmlPath, { waitUntil: 'domcontentloaded' });

    const frameCount = Math.ceil(durationMs / frameInterval);
    console.log(`Capturing ${frameCount} frames at ${fps} fps...`);

    const framePaths = [];
    for (let i = 0; i < frameCount; i++) {
        const fp = path.join(tmpDir, `frame-${String(i).padStart(5, '0')}.png`);
        await page.screenshot({ path: fp, type: 'png' });
        framePaths.push(fp);
        if (i % 20 === 0) process.stdout.write(`  frame ${i}/${frameCount}\r`);
        await new Promise(r => setTimeout(r, frameInterval));
    }
    console.log(`  Captured ${frameCount} frames.`);

    await browser.close();

    // Assemble animated WebP using img2webp
    console.log('Encoding animated WebP...');
    fs.mkdirSync(path.dirname(outFile), { recursive: true });

    const delay = Math.round(1000 / fps);
    // img2webp -loop 0 -d <delay> frame1.png -d <delay> frame2.png ... -o out.webp
    const args = ['-loop', '0', '-lossless', '-m', '6'];
    for (const fp of framePaths) {
        args.push('-d', String(delay), fp);
    }
    args.push('-o', outFile);

    execFileSync('img2webp', args, { stdio: 'inherit' });

    // Clean up temp frames
    fs.rmSync(tmpDir, { recursive: true, force: true });

    const stat = fs.statSync(outFile);
    console.log(`Written: ${outFile} (${(stat.size / 1024).toFixed(0)} KB)`);
})();
