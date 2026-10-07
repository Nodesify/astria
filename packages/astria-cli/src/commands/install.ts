import { Command } from 'commander';
import { installPlatform, uninstallPlatform, purgeData } from '../install/index';
import { PLATFORM_NAMES } from '../install/platforms';
import { parseScope, readInstallState } from '../install/state';
import { uninstallGitHooks } from '../install/hooks';
import { mergeDriverUninstall } from './merge-driver';

type Options = { platform: string; scope: string; all?: boolean; purgeProject?: boolean; purgeGlobal?: boolean };

function report(work: () => string[]): boolean {
  try { for (const message of work()) console.log(message); return true; }
  catch (error: any) { console.error(`Failed: ${error.message || error}`); process.exitCode = 1; return false; }
}

export function registerInstallCommand(program: Command) {
  program.command('install')
    .description('Install Astria integrations in this project, or explicitly for the user')
    .option('--platform <name>', `Platform: ${PLATFORM_NAMES.join(', ')}`, 'claude')
    .option('--scope <scope>', 'project or user', 'project')
    .option('--all', 'Install for every supported project platform')
    .action((opts: Options) => {
      try {
        const scope = parseScope(opts.scope);
        if (opts.all && scope === 'user') throw new Error('Choose one platform for --scope user; --all is project-only.');
        for (const platform of opts.all ? PLATFORM_NAMES : [opts.platform]) {
          report(() => installPlatform(platform, process.cwd(), scope));
        }
        if (!process.exitCode) console.log('Integration setup complete. Restart your assistant/MCP server, then run astria doctor.');
      } catch (error: any) { console.error(error.message); process.exitCode = 1; }
    });

  program.command('uninstall')
    .description('Remove scoped integrations; data is kept unless separately selected for deletion')
    .option('--platform <name>', `Platform: ${PLATFORM_NAMES.join(', ')}`, 'claude')
    .option('--scope <scope>', 'project or user', 'project')
    .option('--all', 'Remove every recorded integration in the selected scope')
    .option('--purge-project', 'Also remove this project graph, git hooks and merge-driver wiring')
    .option('--purge-global', 'Also delete the user-wide cross-repository store')
    .action((opts: Options) => {
      try {
        const scope = parseScope(opts.scope);
        const platforms = opts.all ? readInstallState(process.cwd(), scope).platforms : [opts.platform];
        let complete = true;
        for (const platform of platforms) complete = report(() => uninstallPlatform(platform, process.cwd(), scope)) && complete;
        if (opts.purgeProject) {
          complete = report(() => uninstallGitHooks(process.cwd())) && complete;
          complete = report(() => mergeDriverUninstall(process.cwd())) && complete;
          // Retain data when integration removal failed, so recovery remains possible.
          if (complete) complete = report(() => purgeData(process.cwd(), false));
        }
        if (opts.purgeGlobal && complete) complete = report(() => purgeData(process.cwd(), true));
        console.log(complete ? 'Selected cleanup completed. The npm package remains installed; see the lifecycle guide for package removal.' : 'Cleanup incomplete. Resolve the failures above and retry; remaining graph data was retained.');
      } catch (error: any) { console.error(error.message); process.exitCode = 1; }
    });
}
