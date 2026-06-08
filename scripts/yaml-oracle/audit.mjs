#!/usr/bin/env node
// YAML consumer-oracle audit driver.
//
// Walks the vendored yaml-test-suite fixture tree (the same hashed-subcase
// recursion the Rust harness uses in tests/yaml.rs::all_case_paths) and, for
// every case, records how each *real* consumer of Panache YAML treats it:
//
//   - yaml12        : the abstract YAML-1.2 verdict, inherited from the suite
//                     (presence of an `error` file => "error", else "valid").
//   - psych_libyaml : Ruby Psych / libyaml 0.2.5  (≈ pandoc's Haskell yaml lib).
//   - pandoc_direct : pandoc reading the document as a YAML metadata block
//                     (the REAL frontmatter consumer; ground truth for it).
//   - jsyaml        : js-yaml DEFAULT_SCHEMA (the parser Quarto uses for
//                     frontmatter and hashpipe `#|` options).
//   - ryaml         : R's `yaml` package — the parser the RMarkdown toolchain
//                     uses (rmarkdown frontmatter + knitr `#|` options). Like
//                     libyaml but REJECTS duplicate keys.
//
// Emits scripts/yaml-oracle/oracle.json (committed) plus a human-readable
// scripts/yaml-oracle/oracle-discrepancies.md flagging cases where the
// frontmatter consumers (pandoc_direct vs psych_libyaml) disagree.
//
// Read-only with respect to the parser/crate: this changes no production code.

import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, writeFileSync, existsSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import yaml from 'js-yaml';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const FIXTURE_ROOT = join(
  SCRIPT_DIR,
  '..',
  '..',
  'crates',
  'panache-parser',
  'tests',
  'fixtures',
  'yaml-test-suite',
);

// --- fixture walk: mirror all_case_paths() in tests/yaml.rs -----------------

function isHashedParent(name) {
  return name.length === 4 && /^[A-Z0-9]{4}$/.test(name);
}

function allCasePaths() {
  const entries = [];
  for (const name of readdirSync(FIXTURE_ROOT)) {
    const path = join(FIXTURE_ROOT, name);
    if (!isDir(path)) continue;
    if (existsSync(join(path, 'in.yaml'))) {
      entries.push([name, path]);
      continue;
    }
    // No in.yaml at top level: recurse only into 4-char hashed parents that
    // hold numbered subcases (e.g. 2G84/00). Skip the name/ and tags/ symlink
    // index directories to avoid double-counting.
    if (!isHashedParent(name)) continue;
    for (const sub of readdirSync(path)) {
      const subPath = join(path, sub);
      if (!isDir(subPath) || !existsSync(join(subPath, 'in.yaml'))) continue;
      entries.push([`${name}/${sub}`, subPath]);
    }
  }
  entries.sort((a, b) => a[0].localeCompare(b[0]));
  return entries;
}

function isDir(path) {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

// --- multi-document detection -----------------------------------------------
// A case is multi-document (or carries explicit doc markers) if any line is a
// bare `---`/`...` or one prefixed before content (`--- foo`). Such cases can
// never occur as frontmatter (single-document by construction) and break the
// `---\n...\n---` pandoc wrapping, so they are excluded from frontmatter
// classification.
function isMultiDoc(content) {
  return content.split('\n').some((rawLine) => {
    const line = rawLine.replace(/\r$/, '');
    return (
      line === '---' ||
      line === '...' ||
      line.startsWith('--- ') ||
      line.startsWith('... ')
    );
  });
}

// --- oracle runners ---------------------------------------------------------

function jsyamlVerdict(content) {
  try {
    yaml.load(content, { schema: yaml.DEFAULT_SCHEMA });
    return 'ok';
  } catch (e) {
    return `err:${flatten(e && e.message ? e.message : e)}`;
  }
}

function psychVerdict(content) {
  const res = spawnSync('ruby', [join(SCRIPT_DIR, 'psych_verdict.rb')], {
    input: content,
    encoding: 'utf8',
  });
  if (res.error) return `err:psych-spawn-failed:${flatten(res.error.message)}`;
  return res.stdout.trim() || `err:psych-no-output:${flatten(res.stderr)}`;
}

function ryamlVerdict(content) {
  const res = spawnSync('Rscript', ['--vanilla', join(SCRIPT_DIR, 'ryaml_verdict.R')], {
    input: content,
    encoding: 'utf8',
  });
  if (res.error) return `err:ryaml-spawn-failed:${flatten(res.error.message)}`;
  return res.stdout.trim() || `err:ryaml-no-output:${flatten(res.stderr)}`;
}

function pandocDirectVerdict(content, multidoc) {
  if (multidoc) return 'n/a-multidoc';
  const body = content.replace(/\n+$/, '');
  const doc = `---\n${body}\n---\n`;
  const res = spawnSync('pandoc', ['--from=markdown', '--to=json'], {
    input: doc,
    encoding: 'utf8',
  });
  if (res.status === 0) return 'ok';
  const stderr = flatten(res.stderr) || flatten(res.error && res.error.message);
  return `err:${stderr || `exit-${res.status}`}`;
}

function flatten(s) {
  return String(s == null ? '' : s)
    .replace(/\s+/g, ' ')
    .trim();
}

function isErr(verdict) {
  return typeof verdict === 'string' && verdict.startsWith('err:');
}

// --- provenance -------------------------------------------------------------

function toolVersions() {
  const first = (cmd, args) => {
    try {
      return flatten(execFileSync(cmd, args, { encoding: 'utf8' }).split('\n')[0]);
    } catch {
      return 'unavailable';
    }
  };
  let psychLibyaml = 'unavailable';
  try {
    psychLibyaml = flatten(
      execFileSync(
        'ruby',
        ['-rpsych', '-e', "print 'psych ' + Psych::VERSION + ' libyaml ' + Psych::LIBYAML_VERSION"],
        { encoding: 'utf8' },
      ),
    );
  } catch {
    /* leave unavailable */
  }
  let ryaml = 'unavailable';
  try {
    ryaml = flatten(
      execFileSync(
        'Rscript',
        ['--vanilla', '-e', "cat('R', as.character(getRversion()), 'yaml', as.character(packageVersion('yaml')))"],
        { encoding: 'utf8' },
      ),
    );
  } catch {
    /* leave unavailable */
  }
  return {
    pandoc: first('pandoc', ['--version']),
    quarto: first('quarto', ['--version']),
    node: process.version,
    js_yaml: yaml.version || 'unknown',
    ruby_psych: psychLibyaml,
    r_yaml: ryaml,
  };
}

// --- main -------------------------------------------------------------------

function main() {
  const cases = [];
  for (const [id, path] of allCasePaths()) {
    const content = readFileSync(join(path, 'in.yaml'), 'utf8');
    const multidoc = isMultiDoc(content);
    const yaml12 = existsSync(join(path, 'error')) ? 'error' : 'valid';
    cases.push({
      id,
      yaml12,
      multidoc,
      psych_libyaml: psychVerdict(content),
      pandoc_direct: pandocDirectVerdict(content, multidoc),
      jsyaml: jsyamlVerdict(content),
      ryaml: ryamlVerdict(content),
    });
  }

  // (id, yaml12) checksum — a fixture refresh that adds/changes cases or flips
  // a verdict changes this, so the Rust guard test can demand regeneration.
  const checksum = createHash('sha256')
    .update(cases.map((c) => `${c.id}\t${c.yaml12}`).sort().join('\n'))
    .digest('hex');

  const oracle = {
    generated_by:
      'scripts/yaml-oracle/audit.mjs — regenerate after any yaml-test-suite refresh',
    tools: toolVersions(),
    fixture_checksum: checksum,
    case_count: cases.length,
    cases,
  };
  writeFileSync(join(SCRIPT_DIR, 'oracle.json'), `${JSON.stringify(oracle, null, 2)}\n`);

  writeDiscrepancies(cases, oracle.tools);

  const summary = summarize(cases);
  process.stderr.write(
    `oracle.json: ${cases.length} cases. ` +
      `pandoc≠psych (frontmatter, non-multidoc): ${summary.pandocVsPsych}. ` +
      `checksum ${checksum.slice(0, 12)}…\n`,
  );
}

function summarize(cases) {
  let pandocVsPsych = 0;
  for (const c of cases) {
    if (c.multidoc) continue;
    if (c.pandoc_direct === 'n/a-multidoc') continue;
    if (isErr(c.pandoc_direct) !== isErr(c.psych_libyaml)) pandocVsPsych += 1;
  }
  return { pandocVsPsych };
}

function writeDiscrepancies(cases, tools) {
  const rows = cases.filter(
    (c) =>
      !c.multidoc &&
      c.pandoc_direct !== 'n/a-multidoc' &&
      isErr(c.pandoc_direct) !== isErr(c.psych_libyaml),
  );
  const lines = [];
  lines.push('# Oracle discrepancies: pandoc_direct vs psych_libyaml');
  lines.push('');
  lines.push(
    'Frontmatter (non-multidoc) cases where pandoc and raw libyaml/Psych disagree.',
  );
  lines.push(
    'For frontmatter, **pandoc_direct is ground truth** — pandoc wraps the YAML',
  );
  lines.push(
    'as a metadata block and adds rules (e.g. metadata must be a mapping) on top',
  );
  lines.push('of libyaml. These rows need human classification in Phase 1.');
  lines.push('');
  lines.push(`Tools: ${tools.pandoc} / ${tools.ruby_psych}`);
  lines.push('');
  if (rows.length === 0) {
    lines.push('_None — pandoc and Psych agreed on every frontmatter case._');
  } else {
    lines.push('| case | yaml12 | pandoc_direct | psych_libyaml |');
    lines.push('| --- | --- | --- | --- |');
    for (const c of rows) {
      lines.push(
        `| ${c.id} | ${c.yaml12} | ${md(c.pandoc_direct)} | ${md(c.psych_libyaml)} |`,
      );
    }
  }
  lines.push('');
  writeFileSync(join(SCRIPT_DIR, 'oracle-discrepancies.md'), `${lines.join('\n')}\n`);
}

function md(s) {
  return String(s).replace(/\|/g, '\\|');
}

main();
