// Shared by the runners: loading cases and the matching rule.
import { readdirSync, readFileSync } from 'node:fs';

const dir = new URL('./cases/', import.meta.url);

export function loadCases() {
  return readdirSync(dir).filter(f => f.endsWith('.json')).sort()
    .flatMap(file => JSON.parse(readFileSync(new URL(file, dir), 'utf8')).map(testCase => ({ file, testCase })));
}

/// `expected` matches `actual` when every field it names matches; objects may
/// have extra fields, arrays must have the same length. Returns a problem or null.
export function matches(expected, actual, path = '') {
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual)) return `${path || 'reply'}: expected an array, got ${JSON.stringify(actual)}`;
    if (actual.length !== expected.length) return `${path}: expected ${expected.length} items, got ${actual.length}`;
    for (let i = 0; i < expected.length; i++) { const p = matches(expected[i], actual[i], `${path}[${i}]`); if (p) return p; }
    return null;
  }
  if (expected && typeof expected === 'object') {
    if (!actual || typeof actual !== 'object' || Array.isArray(actual)) return `${path || 'reply'}: expected an object, got ${JSON.stringify(actual)}`;
    for (const [k, v] of Object.entries(expected)) { const p = matches(v, actual[k] ?? null, path ? `${path}.${k}` : k); if (p) return p; }
    return null;
  }
  return expected === actual ? null : `${path}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`;
}
