import { join } from 'node:path';
import { createRelacher } from 'relacher';
import { loadCargoDeps } from 'relacher';

const root = join(import.meta.dir, '../..');

const deps = loadCargoDeps(root, { includePrivate: true })
  // Creates and updates crates/ocklog/CHANGELOG.md
  .withChangelogs()
  // Root ./CHANGELOG.md with GitHub commit links
  .withRootChangelog({ github: 'emilien-jegou/ocklog' })
  .syncVersion('ocklog', './flake.nix', 'version = "[^"]+"')
  .addWatchFiles('ocklog', './scripts/release/release-gh.ts');


const relacher = createRelacher({
  cwd: root,
  vcs: 'jj',
  packages: deps,
  sizes: {
    major: { pattern: '^[a-z]+(?:\\([^)]+\\))?!|^[a-z]+\\([^)]+\\)!:|^BREAKING CHANGE' },
    minor: { pattern: '^(feat|revert|refactor|perf)' },
    patch: { pattern: '^(fix|bugfix|patch|deps|build)' },
    skip: { pattern: '^(release|chore|infra|docs|test|ci|nit|style)' },
  },
  cascade: {
    skip: 'skip',
    patch: 'patch',
    minor: 'minor',
    major: 'minor',
  },
  commitTitle: (bumps) => {
    if ('ocklog' in bumps) {
      const others = Object.keys(bumps).length - 1;
      return `release: ocklog-v${bumps['ocklog']}${others > 0 ? ` (+${others} lib)` : ''}`;
    }
    const libs = Object.entries(bumps).map(([k, v]) => `${k}-v${v}`).join(', ');
    return `release(libs): ${libs}`;
  },
});

await relacher.cli();
