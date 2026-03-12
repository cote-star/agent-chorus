/**
 * Context pack integrity verification.
 * Validates manifest.json checksums against actual file content.
 */

'use strict';

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

function sha256(input) {
  return crypto.createHash('sha256').update(input).digest('hex');
}

function verify(packDir) {
  const currentDir = path.join(packDir, 'current');
  const manifestPath = path.join(currentDir, 'manifest.json');

  if (!fs.existsSync(manifestPath)) {
    throw new Error(`[context-pack] verify failed: manifest.json not found at ${manifestPath}`);
  }

  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  const files = manifest.files;
  if (!Array.isArray(files)) {
    throw new Error('[context-pack] verify failed: manifest has no \'files\' array');
  }

  let passCount = 0;
  let failCount = 0;

  for (const entry of files) {
    const filePath = entry.path || 'unknown';
    const expectedHash = entry.sha256 || '';
    const actualPath = path.join(currentDir, filePath);

    if (!fs.existsSync(actualPath)) {
      console.error(`  FAIL  ${filePath}  (file missing)`);
      failCount++;
      continue;
    }

    const content = fs.readFileSync(actualPath, 'utf8');
    const actualHash = sha256(content);

    if (actualHash === expectedHash) {
      console.log(`  PASS  ${filePath}`);
      passCount++;
    } else {
      console.error(`  FAIL  ${filePath}  (checksum mismatch)`);
      failCount++;
    }
  }

  // Verify pack_checksum if present
  if (manifest.pack_checksum) {
    const packInput = files.map(f => `${f.path || 'unknown'}:${f.sha256 || ''}`).join('\n');
    const actualPackChecksum = sha256(packInput);
    if (actualPackChecksum === manifest.pack_checksum) {
      console.log('  PASS  pack_checksum');
      passCount++;
    } else {
      console.error('  FAIL  pack_checksum (mismatch)');
      failCount++;
    }
  }

  const total = passCount + failCount;
  console.log(`\n  Results: ${passCount}/${total} passed`);

  if (failCount > 0) {
    throw new Error(`[context-pack] verify failed: ${failCount} file(s) did not match`);
  }
  console.log('  Context pack integrity verified.');
}

// CLI entry point
if (require.main === module) {
  const args = process.argv.slice(2);
  const packDir = args.find((_, i, a) => a[i - 1] === '--pack-dir') || '.agent-context';
  try {
    verify(packDir);
  } catch (err) {
    console.error(err.message);
    process.exit(1);
  }
}

module.exports = { verify };
