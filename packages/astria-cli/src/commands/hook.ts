import { Command } from 'commander';
import { installGitHooks, uninstallGitHooks, statusGitHooks } from '../install/hooks';

export function registerHookCommand(program: Command) {
  const hook = program.command('hook').description('Manage git hooks for auto-rebuild');

  hook
    .command('install')
    .description('Install post-commit and post-checkout hooks')
    .action(() => {
      try {
        const results = installGitHooks('.');
        for (const msg of results) {
          console.log(msg);
        }
      } catch (err: any) {
        console.error(err.message || err);
        process.exitCode = 1;
      }
    });

  hook
    .command('uninstall')
    .description('Remove nodesify-graphify git hooks')
    .action(() => {
      try {
        const results = uninstallGitHooks('.');
        for (const msg of results) {
          console.log(msg);
        }
      } catch (err: any) {
        console.error(err.message || err);
        process.exitCode = 1;
      }
    });

  hook
    .command('status')
    .description('Show git hook status')
    .action(() => {
      try {
        const results = statusGitHooks('.');
        for (const msg of results) {
          console.log(msg);
        }
      } catch (err: any) {
        console.error(err.message || err);
        process.exitCode = 1;
      }
    });
}
