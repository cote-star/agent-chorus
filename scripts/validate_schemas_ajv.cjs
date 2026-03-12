#!/usr/bin/env node
'use strict';
/**
 * Validate a JSON data file against a JSON Schema (draft-2020-12).
 * Replaces ajv-cli to eliminate transitive security vulnerabilities.
 *
 * Usage: node scripts/validate_schemas_ajv.cjs <schema.json> <data.json>
 * Exit 0 on success, 1 on validation failure, 2 on usage error.
 */

const fs = require('fs');
const path = require('path');
const Ajv = require('ajv/dist/2020');
const addFormats = require('ajv-formats');

if (process.argv.length < 4) {
  console.error('Usage: node validate_schemas_ajv.cjs <schema.json> <data.json>');
  process.exit(2);
}

const schemaPath = path.resolve(process.argv[2]);
const dataPath = path.resolve(process.argv[3]);

let schema, data;
try {
  schema = JSON.parse(fs.readFileSync(schemaPath, 'utf8'));
} catch (err) {
  console.error(`Failed to read schema: ${schemaPath}\n${err.message}`);
  process.exit(2);
}
try {
  data = JSON.parse(fs.readFileSync(dataPath, 'utf8'));
} catch (err) {
  console.error(`Failed to read data: ${dataPath}\n${err.message}`);
  process.exit(2);
}

const ajv = new Ajv({ allErrors: true, strict: false });
addFormats(ajv);

const validate = ajv.compile(schema);
const valid = validate(data);

if (!valid) {
  const schemaName = path.basename(schemaPath);
  const dataName = path.basename(dataPath);
  console.error(`FAIL: ${dataName} does not match ${schemaName}`);
  for (const err of validate.errors) {
    console.error(`  ${err.instancePath || '/'} ${err.message}`);
  }
  process.exit(1);
}
