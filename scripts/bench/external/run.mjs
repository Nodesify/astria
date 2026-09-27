// Explicit opt-in benchmark; clones pinned upstream source without executing it.
import {spawnSync} from 'node:child_process';
import {readFileSync, existsSync, mkdirSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {loadTokenizer} from '../tokenize.mjs';
const dir = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(dir, '../../..');
const cli = path.join(repo, 'packages/astria-cli/dist/index.js');
if (!await loadTokenizer()) throw new Error('Paired suite requires js-tiktoken (o200k_base)');
if (!existsSync(cli) || !existsSync(path.join(repo, 'packages/astria-cli/dist/astria.node')) || existsSync(path.join(repo, 'packages/astria-cli/astria.node'))) throw new Error('Build dist/index.js and dist/astria.node; package-root astria.node must not exist');
const work = path.join(repo, 'bench-work/external');
mkdirSync(work, {recursive:true});
const run = (cmd,args,cwd=repo) => {
  const r=spawnSync(cmd,args,{cwd,encoding:'utf8',stdio:['ignore','pipe','inherit']});
  if(r.status!==0 || r.error) throw new Error(`${cmd} failed: ${r.error || r.status}`);
  return r.stdout.trim();
};
for(const c of JSON.parse(readFileSync(path.join(dir,'corpora.json'),'utf8'))) {
  const root=path.join(work,c.name);
  if(!existsSync(root)) {
    run('git',['clone','--no-checkout',c.repository,root]);
    run('git',['-C',root,'checkout','--detach',c.commit]);
  }
  if(run('git',['rev-parse','HEAD'],root)!==c.commit || run('git',['status','--porcelain','--untracked-files=all','--','.',':(exclude).astria/**'],root)) throw new Error(`Corpus ${c.name} must be clean at ${c.commit}`);
  for(const line of readFileSync(path.join(dir,c.golden),'utf8').trim().split('\n')) {
    for(const e of JSON.parse(line).evidence) if(!readFileSync(path.join(root,e.path),'utf8').includes(e.contains)) throw new Error(`Ungrounded evidence: ${c.name}/${e.path}`);
  }
  if(existsSync(path.join(root,'.astria'))) throw new Error(`Use a fresh corpus directory: ${root} already has a graph`);
  run(process.execPath,[cli,'run','.'],root);
  for(const budget of [1000,4000]) for(const baseline of [false,true]) {
    const output=path.join(repo,'scripts/bench/quality/out',`${c.name}-${budget}-${baseline?'rg':'astria'}.json`);
    run(process.execPath,[path.join(repo,'scripts/bench/quality/run-quality.mjs'),'--root',root,'--golden',path.join(dir,c.golden),'--astria',cli,'--budget',String(budget),'--out',output,...(baseline?['--baseline']:[])]);
  }
}
