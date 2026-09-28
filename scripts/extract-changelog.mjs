// Extract one version's section from CHANGELOG.md as a GitHub Release body, so
// the release page carries the same curated notes as the changelog instead of
// only the auto-generated PR list.
//
// The Release workflow runs this twice:
//   verify job:  --check fails the tag before the build matrix when the
//                CHANGELOG section for the tag is missing (i.e. [Unreleased]
//                was never promoted to a dated version heading)
//   publish job: the extracted body goes to action-gh-release's body_path;
//                generate_release_notes appends the PR list below it
//
// Section = the text between the "## [<version>]" heading and the next "## "
// heading. Relative markdown link targets are rewritten to blob URLs pinned at
// the tag (they would otherwise resolve against the release page and 404), and
// the file's link-reference definitions are appended so a version heading
// links to its compare URL, matching how CHANGELOG.md renders on GitHub.
//
// Zero runtime dependencies.
// Run: node scripts/extract-changelog.mjs <version> [--check] [--file <path>]
//   <version>  the tag minus the leading "v", e.g. 1.0.6
//   --check    validate only: exit 0/1, print nothing
//   --file     changelog path to parse (default: ../CHANGELOG.md; for tests)
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const check = args.includes('--check');
let changelogPath = fileURLToPath(new URL('../CHANGELOG.md', import.meta.url));
let version;
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--file') {
    if (!args[i + 1]) {
      console.error('--file requires a path');
      process.exit(2);
    }
    changelogPath = args[++i];
    continue;
  }
  if (!args[i].startsWith('--') && version === undefined) {
    version = args[i];
  }
}

if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(version)) {
  console.error('Usage: node scripts/extract-changelog.mjs <version> [--check] [--file <path>]   # e.g. 1.0.6');
  process.exit(2);
}

const lines = readFileSync(changelogPath, 'utf8').split('\n');

let start = -1;
for (let i = 0; i < lines.length; i++) {
  const m = lines[i].match(/^## \[([^\]]+)\]/);
  if (m && m[1] === version) {
    start = i;
    break;
  }
}
if (start === -1) {
  console.error(
    `CHANGELOG.md has no "## [${version}]" section — promote the [Unreleased] entries to a dated "## [${version}]" heading before tagging v${version}.`
  );
  process.exit(1);
}
let end = lines.length;
for (let i = start + 1; i < lines.length; i++) {
  if (/^## /.test(lines[i])) {
    end = i;
    break;
  }
}
let body = lines.slice(start, end).join('\n').trimEnd();

// Relative link targets (e.g. website/docs/...) do not resolve on a release
// page; pin them at the tag. Absolute URLs, anchors, root-relative, and
// mailto: are left alone.
const repo = process.env.GITHUB_REPOSITORY || 'Nodesify/astria';
const ref = `v${version}`;
body = body.replace(/\]\(([^()\s]+)\)/g, (full, target) => {
  if (/^(https?:\/\/|mailto:|#|\/)/.test(target)) return full;
  return `](https://github.com/${repo}/blob/${ref}/${target})`;
});

// Keep-a-Changelog keeps per-version compare links as reference definitions at
// the file tail; without them the extracted "[X.Y.Z]" headings render as bare
// brackets on the release page.
const defs = lines.filter((l) => /^\[[^\]]+\]:\s*\S/.test(l));
if (defs.length) body += `\n\n${defs.join('\n')}`;

if (check) process.exit(0);
console.log(body);
