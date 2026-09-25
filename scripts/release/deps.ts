
import { changelogUpdate, loadCargoDeps, regexUpdate, type ChangelogContext } from 'relacher';

const genChangelog = (folderPath: string, opts = {}) =>
  changelogUpdate(`${folderPath}${folderPath.at(-1) === '/' ? '' : '/'}CHANGELOG.md`, {
    onlyOn: ['major', 'minor', 'patch'],
    ...opts
  });

export const depsBuilder = (root: string) =>
  loadCargoDeps(root).onPackageBump("ocklog",
    regexUpdate("./flake.nix", {
      search: 'version = "[^"]+"',
      replace: 'version = "{{version}}"',
    }),
    genChangelog('./crates/ocklog'),
    genChangelog('./', {
      global: true,
      template: cliffTemplate
    }))
    .onPackageBump("ocklog-tasker", genChangelog('./crates/ocklog-tasker'))
    .onPackageBump("ocklog-rune-actions", genChangelog('./crates/ocklog-rune-actions'))
    .couple('ocklog-rune-actions', 'ocklog-rune-actions-derive')
    .couple('ocklog-tasker', 'ocklog-tasker-derive')
    .addWatchFiles('ocklog', './scripts/release/release-gh.ts');



function cliffTemplate({ version, date, commits }: ChangelogContext): string {
  const cleanVersion = version ? version.replace(/^v/, "") : "Unreleased";
  const lines = ['', `## [${cleanVersion}] - ${date}`];

  const grouped = groupBy(commits, "type");

  for (const [group, groupList] of Object.entries(grouped)) {
    if (groupList.length === 0) continue;
    lines.push(`\n### ${group.charAt(0).toUpperCase() + group.slice(1)}`);
    for (const commit of groupList) {
      const breaking = commit.isBreaking ? `[**breaking**] ` : ``;
      const scope = commit.scope ? `**${commit.scope}:** ` : ``;
      const desc = commit.description || commit.message;
      const msg = desc.charAt(0).toUpperCase() + desc.slice(1);
      lines.push(
        `- ${breaking}${scope}${msg} — [\`${commit.shortHash}\`](https://github.com/emilien-jegou/ocklog/commit/${commit.hash}) by ${commit.author}`,
      );
    }
  }

  return lines.join("\n");
}

function groupBy<T, K extends keyof T>(arr: T[], key: K): Record<string, T[]> {
  return arr.reduce(
    (acc, item) => {
      const group = String(item[key]);
      if (!acc[group]) acc[group] = [];
      acc[group].push(item);
      return acc;
    },
    {} as Record<string, T[]>,
  );
}
