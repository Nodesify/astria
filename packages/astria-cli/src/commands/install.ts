import { Command } from 'commander';
import { installPlatform, uninstallPlatform } from '../install/index';
import { PLATFORM_NAMES } from '../install/platforms';

export function registerInstallCommand(program: Command) {
  program
    .command('install')
    .description(`Install astria skill + MCP for an AI platform (${PLATFORM_NAMES.join(', ')}) or all of them`)
    .option('--platform <name>', `Platform: ${PLATFORM_NAMES.join(', ')}`, 'claude')
    .option('--all', 'Install for every supported platform')
    .action(async (opts: { platform: string; all?: boolean }) => {
      try {
        if (opts.all) {
          for (const platform of PLATFORM_NAMES) {
            for (const msg of installPlatform(platform, process.cwd())) {
              console.log(msg);
            }
          }
          return;
        }
        const results = installPlatform(opts.platform, process.cwd());
        for (const msg of results) {
          console.log(msg);
        }
        // A single-platform install is silent about every other tool — point
        // the user at the rest so multi-tool setups don't get half wired.
        if (opts.platform !== 'claude') return;
        const others = PLATFORM_NAMES.filter((p) => p !== 'claude');
        console.log(`\nOther supported platforms: ${others.join(', ')}`);
        console.log('Install for all of them with: astria install --all');
      } catch (err: any) {
        console.error(err.message || err);
        process.exitCode = 1;
      }
    });

  program
    .command('uninstall')
    .description('Uninstall astria skill for an AI platform')
    .option('--platform <name>', `Platform: ${PLATFORM_NAMES.join(', ')}`, 'claude')
    .option('--all', 'Uninstall from every supported platform')
    .action(async (opts: { platform: string; all?: boolean }) => {
      try {
        if (opts.all) {
          for (const platform of PLATFORM_NAMES) {
            for (const msg of uninstallPlatform(platform, process.cwd())) {
              console.log(msg);
            }
          }
          return;
        }
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
