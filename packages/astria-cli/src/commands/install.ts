import { Command } from 'commander';
import { installPlatform, uninstallPlatform } from '../install/index';
import { PLATFORM_NAMES } from '../install/platforms';

export function registerInstallCommand(program: Command) {
  program
    .command('install')
    .description('Install nodesify-graphify skill for an AI platform')
    .option('--platform <name>', `Platform: ${PLATFORM_NAMES.join(', ')}`, 'claude')
    .action(async (opts: { platform: string }) => {
      try {
        const results = installPlatform(opts.platform, process.cwd());
        for (const msg of results) {
          console.log(msg);
        }
      } catch (err: any) {
        console.error(err.message || err);
        process.exitCode = 1;
      }
    });

  program
    .command('uninstall')
    .description('Uninstall nodesify-graphify skill for an AI platform')
    .option('--platform <name>', `Platform: ${PLATFORM_NAMES.join(', ')}`, 'claude')
    .action(async (opts: { platform: string }) => {
      try {
        const results = uninstallPlatform(opts.platform, process.cwd());
        for (const msg of results) {
          console.log(msg);
        }
      } catch (err: any) {
        console.error(err.message || err);
        process.exitCode = 1;
      }
    });
}
