#!/usr/bin/env node
// Reads a YAML document on stdin and reports whether js-yaml (YAML 1.2, the
// parser Quarto uses for frontmatter and hashpipe `#|` cell options) accepts
// it. Prints "ok" on success or "err:<message>" on a parse error.
//
// Standalone counterpart to the inline js-yaml check in audit.mjs; handy for
// spot-checking a single document by hand.

import yaml from 'js-yaml';

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => {
  input += chunk;
});
process.stdin.on('end', () => {
  try {
    yaml.load(input, { schema: yaml.DEFAULT_SCHEMA });
    process.stdout.write('ok');
  } catch (e) {
    const msg = String(e && e.message ? e.message : e).replace(/\s+/g, ' ').trim();
    process.stdout.write(`err:${msg}`);
  }
});
