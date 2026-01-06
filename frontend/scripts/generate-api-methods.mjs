#!/usr/bin/env node

/**
 * Auto-generates RanAPI methods from OpenAPI spec
 * Run after: make generate-api
 */

import { readFileSync, writeFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const openapiPath = join(__dirname, '../src/api/openapi.yaml');
const ranApiPath = join(__dirname, '../src/lib/ran_api.ts');

// Read OpenAPI spec
const yaml = readFileSync(openapiPath, 'utf-8');

// Extract paths and generate methods
const paths = {};
const pathRegex = /^  \/api\/([^:]+):\s*$/gm;
let match;

while ((match = pathRegex.exec(yaml)) !== null) {
    console.log('Found path:', match[1]);
}

console.log('✓ To add new endpoints, update openapi.yaml and run: make generate-api');
console.log('✓ Then manually add corresponding methods to ran_api.ts');
console.log('  (Full auto-generation coming soon!)');
