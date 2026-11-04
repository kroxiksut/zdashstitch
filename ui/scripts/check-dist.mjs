// Build output guard for the opt-in DEMO build (synthetic drafts for manual accessibility checks).
//   node scripts/check-dist.mjs normal dist      -> fails if any demo draft reached the normal build
//   node scripts/check-dist.mjs demo dist-demo   -> fails if the demo build lost its demo drafts
// Demo drafts carry `local_id`s with the prefix below (see src/demo/drafts.ts).

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const DEMO_MARKER = 'demo-synthetic-';
const [kind, directory] = process.argv.slice(2);

if ((kind !== 'normal' && kind !== 'demo') || !directory) {
  console.error('usage: node scripts/check-dist.mjs normal|demo <dist directory>');
  process.exit(2);
}

function files(root) {
  return readdirSync(root).flatMap((name) => {
    const path = join(root, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

const root = resolve(directory);
const all = files(root);
if (!all.some((path) => path.endsWith('.js'))) {
  console.error(`${directory}: no JavaScript output found`);
  process.exit(1);
}
const withDemo = all.filter((path) => readFileSync(path, 'latin1').includes(DEMO_MARKER));

if (kind === 'normal' && withDemo.length > 0) {
  console.error(`demo data leaked into the normal build: ${withDemo.map((path) => relative(root, path)).join(', ')}`);
  process.exit(1);
}
if (kind === 'demo' && withDemo.length === 0) {
  console.error(`${directory}: the demo build contains no demo drafts`);
  process.exit(1);
}
console.log(`${directory}: ${kind === 'normal' ? 'no demo data' : 'demo data present'} (${all.length} files checked)`);
