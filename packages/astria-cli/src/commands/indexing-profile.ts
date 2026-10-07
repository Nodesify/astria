import { existsSync, readFileSync } from 'fs';
import { join } from 'path';

export interface IndexingOptions {
  dedup?: boolean; backend?: string; judge?: string; model?: string;
  embed?: boolean; labelCommunities?: boolean; deep?: boolean;
  refreshPolicy?: boolean; llmBudget?: string;
}
interface Profile {
  version: number; environment: Record<string, string>; dedup: boolean;
  embed: boolean; label_communities: boolean; deep: boolean;
}
const KEYS = ['LLM_BACKEND', 'LLM_MODEL', 'LLM_BASE_URL', 'LLM_JUDGE', 'KIMI_BASE_URL',
  'AZURE_ENDPOINT', 'AZURE_DEPLOYMENT', 'AZURE_API_VERSION', 'AWS_REGION', 'LLM_BUDGET'];

/** Updates reuse the project's policy, including an explicitly disabled backend. */
export function indexingOptions(root: string, opts: IndexingOptions, update: boolean): IndexingOptions {
  if (opts.refreshPolicy) process.env.ASTRIA_REFRESH_POLICY = '1';
  else delete process.env.ASTRIA_REFRESH_POLICY;
  const filename = join(root, '.astria', 'indexing-profile.json');
  const profile: Profile | undefined = update && existsSync(filename)
    ? JSON.parse(readFileSync(filename, 'utf8')) : undefined;
  if (profile && profile.version !== 1) throw new Error('Unsupported indexing profile version; rebuild with an explicit policy');
  const incoming = Object.fromEntries(KEYS.map(k => [`ASTRIA_${k}`, process.env[`ASTRIA_${k}`]]));
  if (profile) {
    for (const key of KEYS) {
      const name = `ASTRIA_${key}`;
      if (profile.environment[name] !== undefined) process.env[name] = profile.environment[name];
      else delete process.env[name];
    }
    // Only an explicit refresh opts into new shell-level routing settings.
    if (opts.refreshPolicy) for (const [key, value] of Object.entries(incoming)) {
      if (value !== undefined) process.env[key] = value;
    }
  } else if (update && !opts.backend) {
    process.env.ASTRIA_LLM_BACKEND = 'none';
    delete process.env.ASTRIA_LLM_JUDGE;
  }
  if (opts.backend) process.env.ASTRIA_LLM_BACKEND = opts.backend;
  if (opts.backend?.trim().toLowerCase() === 'none') delete process.env.ASTRIA_LLM_JUDGE;
  if (opts.judge) process.env.ASTRIA_LLM_JUDGE = opts.judge;
  if (opts.model) process.env.ASTRIA_LLM_MODEL = opts.model;
  if (opts.llmBudget !== undefined) {
    if (!/^[1-9]\d*$/.test(opts.llmBudget)) throw new Error('--llm-budget must be a positive integer');
    process.env.ASTRIA_LLM_BUDGET = opts.llmBudget;
  }
  const effective = {
    ...opts,
    dedup: opts.dedup ?? profile?.dedup ?? true,
    embed: opts.embed ?? profile?.embed ?? false,
    labelCommunities: opts.labelCommunities ?? (opts.backend?.trim().toLowerCase() === 'none' ? false : profile?.label_communities) ?? false,
    deep: opts.deep ?? (opts.backend?.trim().toLowerCase() === 'none' ? false : profile?.deep) ?? false,
  };
  const backend = process.env.ASTRIA_LLM_BACKEND?.trim().toLowerCase();
  const paid = Boolean(backend && backend !== 'none');
  const changed = profile && (
    KEYS.filter(k => k !== 'LLM_BUDGET').some(k => profile.environment[`ASTRIA_${k}`] !== process.env[`ASTRIA_${k}`]) ||
    profile.label_communities !== effective.labelCommunities || profile.deep !== effective.deep
  );
  const previousPaid = profile && profile.environment.ASTRIA_LLM_BACKEND !== 'none';
  if (update && (paid || previousPaid) && (changed || !profile || opts.refreshPolicy)) {
    if (!opts.refreshPolicy) throw new Error('Changing indexing policy requires --refresh-policy');
    if (paid && !opts.llmBudget) throw new Error('Changing paid indexing policy requires --llm-budget <positive tokens>');
  }
  if (opts.refreshPolicy && !opts.llmBudget && paid) throw new Error('Paid refresh requires --llm-budget <positive tokens>');
  return effective;
}
