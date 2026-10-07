// Runs the shared protocol cases against the reference model's core
// (reference/kmux/core.js). Usage: node tests/kmux-protocol/run-model.mjs
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { loadCases, matches } from './cases.mjs';

const here = new URL('.', import.meta.url);
const source = readFileSync(new URL('../../reference/kmux/core.js', here), 'utf8');

function freshCore() {
  const context = vm.createContext({ setTimeout, console, URL });
  vm.runInContext(`${source}\n;globalThis.core = { handle, reset: () => { S = freshState(); } };`, context);
  context.core.reset();
  return context.core;
}

let failed = 0, passed = 0;
for (const { file, testCase } of loadCases()) {
  const core = freshCore();
  let error = null;
  for (const [i, step] of testCase.steps.entries()) {
    const reply = await core.handle({ id: i + 1, ...step.send });
    const problem = matches(step.expect ?? { ok: true }, reply);
    if (problem) { error = `step ${i + 1} ${JSON.stringify(step.send)}: ${problem}\n    reply: ${JSON.stringify(reply)}`; break; }
  }
  if (error) { failed++; console.log(`✘ ${file}: ${testCase.name}\n    ${error}`); } else passed++;
}
console.log(`model: ${passed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
