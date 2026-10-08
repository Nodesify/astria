import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,cpSync,copyFileSync,existsSync} from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {createHash} from 'node:crypto';
import {loadTokenizer} from '../tokenize.mjs';
const configPath=path.resolve(process.argv[2]||'');
if(!process.argv[2]) throw Error('Usage: node scripts/bench/paired/run.mjs <config.json>');
const config=JSON.parse(readFileSync(configPath,'utf8'));
const reservedCatalog=JSON.parse(readFileSync(new URL('../reserved-corpora.json',import.meta.url),'utf8'));
const reservedSpec=spec=>String(spec.split||'').startsWith('reserved')||reservedCatalog.golden_files.some(file=>path.basename(file)===path.basename(spec.golden));
if(config.corpora.some(reservedSpec)&&config.allow_reserved!==true)throw Error('Reserved corpora require allow_reserved: true; first retrieval consumes reservation.');
const BUDGETS=Array.isArray(config.budgets)&&config.budgets.length>0?config.budgets:[1000,4000];
const repo=process.cwd(), root=path.resolve(config.output);
if(existsSync(root)) throw Error('Output must be a new directory: '+root);
const tok=await loadTokenizer(); if(!tok) throw Error('Exact js-tiktoken tokenizer required');
const python=path.resolve(config.python);
const suppliedCli=path.resolve(config.astria_cli), native=path.resolve(config.astria_native);
const env={...process.env,ASTRIA_LLM_BACKEND:'none',PYTHONUTF8:'1',PYTHONIOENCODING:'utf-8'};
mkdirSync(root,{recursive:true});
const runtime=path.join(root,'runtime'); mkdirSync(runtime);
cpSync(path.dirname(suppliedCli),path.join(runtime,'dist'),{recursive:true});
copyFileSync(native,path.join(runtime,'astria.node'));
const cli=path.join(runtime,'dist',path.basename(suppliedCli));
copyFileSync(path.resolve(path.dirname(suppliedCli),'../package.json'),path.join(runtime,'package.json'));
env.NODE_PATH=path.resolve(path.dirname(suppliedCli),'../node_modules');
const exec=(cmd,args,cwd=repo)=>{const start=performance.now();const r=spawnSync(cmd,args,{cwd,env,encoding:'utf8',timeout:600000,maxBuffer:64*1024*1024});return {seconds:(performance.now()-start)/1000,status:r.status,stdout:r.stdout||'',stderr:r.stderr||'',error:r.error?.message};};
const git=(dir,...args)=>{const r=exec('git',['-C',dir,...args]);if(r.status!==0)throw Error(r.stderr);return r.stdout.trim();};
const sha=f=>createHash('sha256').update(readFileSync(f)).digest('hex');
if(!/^[0-9a-f]{40}$/.test(config.graphify_commit))throw Error('Graphify requires full commit pin');
if(git(config.graphify_source,'rev-parse','HEAD')!==config.graphify_commit) throw Error('Graphify source pin mismatch');
if(git(config.graphify_source,'status','--porcelain','--untracked-files=no')) throw Error('Graphify source must have no tracked changes');
const imported=exec(python,['-c','import graphify; print(graphify.__file__)']);
if(imported.status!==0 || !path.resolve(imported.stdout.trim()).startsWith(path.resolve(config.graphify_source)+path.sep)) throw Error('Python must use editable Graphify from the pinned source');
const results={schema_version:4,date:new Date().toISOString(),provenance:{astria_commit:git(repo,'rev-parse','HEAD'),source_status:git(repo,'status','--porcelain'),graphify_commit:config.graphify_commit,graphify_status:git(config.graphify_source,'status','--porcelain','--untracked-files=no'),native_sha256:sha(path.join(runtime,'astria.node')),astria_cli_sha256:sha(cli),astria_native:native,astria_cli:cli,graphify_import:imported.stdout.trim(),profile:config.profile||'unspecified',node:process.version,python:exec(python,['--version']).stdout.trim(),platform:os.platform(),cpu:os.cpus()[0].model,logical_cpus:os.cpus().length},method:{line_contract:"one-based-declaration-line",budgets:BUDGETS,depth:2,tokenizer:tok.name,llm:false,embeddings:false,query_repetitions:1,build_repetitions:1,notes:'Fresh git archive copies per tool; serial builds; alternating query order; process startup included. Complete NODE lines only, unique source files in output order; same exact complete-line token clipping for both; definitions matched by grounded file and declaration line, separately from file recall. File retrieval, not generated answer correctness. Graphify structural pipeline via orig_run.py; Astria native pipeline, excluding CLI post-build token benchmark.'},corpora:[]};
const definitionMatch=(n,d,cwd,tool)=>{let f=(n.source_file||'').replaceAll('\\','/');if(path.isAbsolute(f))f=path.relative(cwd,f).replaceAll('\\','/'); const line=n.source_line??Number(String(n.source_location||'').match(/\d+/)?.[0]);return f===d.path && line===d.line;};
const save=()=>writeFileSync(path.join(root,'results.json'),JSON.stringify(results,null,2));
const clip=(text,budget)=>{if(tok.count(text)<=budget)return text;let output='';for(const line of text.match(/[^\n]*\n|[^\n]+$/g)||[]){if(tok.count(output+line)>budget)break;output+=line;}return output;};
const files=(text,cwd)=>{const all=[];for(const line of text.split('\n')){if(!line.startsWith('NODE '))continue;const m=line.match(/\bsrc=(.*?) (?:loc|community)=/);if(!m)continue;let f=m[1].replace(/:\d+(?::\d+)?$/,'').replaceAll('\\','/');if(path.isAbsolute(f))f=path.relative(cwd,f).replaceAll('\\','/');f=f.replace(/^\.\//,'');if(f&&!all.includes(f))all.push(f);}return all;};
writeFileSync(path.join(runtime,'build.cjs'),`const b=require(${JSON.stringify(path.join(runtime,'astria.node'))});console.log(JSON.stringify(b.runPipeline(process.argv[2],false,false,false,false)));`);
for(const spec of config.corpora){
  if(!/^[0-9a-f]{40}$/.test(spec.commit))throw Error('Corpus requires full commit pin');
  const prepared=exec(python,[path.join(repo,'scripts/bench/paired/prepare.py'),spec.source,spec.commit,root,spec.name]);
  if(prepared.status!==0) throw Error(prepared.stderr);
  const corpus={...spec,golden_sha256:sha(spec.golden),builds:{},queries:[],summaries:[]};results.corpora.push(corpus);
  const items=readFileSync(spec.golden,'utf8').trim().split('\n').map(JSON.parse);
  const diagnostics=spec.definitions?JSON.parse(readFileSync(spec.definitions,'utf8')):[];
  if(spec.definitions)corpus.definitions_sha256=sha(spec.definitions);
  for(const item of items)item.definitions=[...(item.definitions||[]),...diagnostics.filter(d=>d.question===item.id)];
  for(const item of items)for(const f of item.expected_files)if(!existsSync(path.join(root,spec.name+'-astria',f)))throw Error('Missing golden '+f);
  for(const item of items)for(const d of item.definitions||[]) if(!readFileSync(path.join(root,spec.name+'-astria',d.path),'utf8').split(/\r?\n/)[d.line-1]?.includes(d.contains)) throw Error('Definition grounding changed: '+item.id);
  for(const item of items)for(const e of item.evidence||[])if(!readFileSync(path.join(root,spec.name+'-astria',e.path),'utf8').includes(e.contains))throw Error('Ungrounded '+e.path);
  for(const tool of ['graphify','astria']){
    const cwd=path.join(root,spec.name+'-'+tool);console.log('BUILD '+spec.name+' '+tool);
    const r=tool==='astria'?exec(process.execPath,[path.join(runtime,'build.cjs'),cwd],cwd):exec(python,[path.join(repo,'scripts/bench/orig_run.py'),cwd,path.join(root,spec.name+'-graphify-build.json')],cwd);
    writeFileSync(path.join(root,spec.name+'-'+tool+'-build.log'),r.stdout+'\nSTDERR\n'+r.stderr);
    if(r.status!==0)throw Error('Build failed '+tool+' '+r.stderr);
    const g=JSON.parse(readFileSync(path.join(cwd,tool==='astria'?'.astria/graph.json':'graphify-out/graph.json'),'utf8'));
    let retention=null;
    if(tool==='astria'){const audit=exec(python,[path.join(repo,'scripts/bench/paired/audit-symbols.py'),cwd]); if(audit.status!==0)throw Error(audit.stderr); retention=JSON.parse(audit.stdout);}
    corpus.builds[tool]={retention,definitions:items.flatMap(item=>(item.definitions||[]).map(d=>({question:item.id,...d,present:g.nodes.some(n=>definitionMatch(n,d,cwd,tool))}))),seconds:r.seconds,nodes:g.nodes.length,edges:(g.edges||g.links).length,stderr:r.stderr};save();console.log(`${tool}: ${g.nodes.length} nodes; ${retention?.preserved??'n/a'} cached definition IDs retained`);
  }
  if(reservedSpec(spec)){corpus.reserved_exposure={started_at:new Date().toISOString(),status:'reservation-consumed-before-first-query'};save();}
  for(const budget of BUDGETS)for(const [index,item] of items.entries())for(const tool of index%2?['graphify','astria']:['astria','graphify']){
    const cwd=path.join(root,spec.name+'-'+tool);
    const r=tool==='astria'?exec(process.execPath,[cli,'query',item.question,'--budget',String(budget),'--depth','2'],cwd):exec(python,['-m','graphify','query',item.question,'--budget',String(budget),'--graph',path.join(cwd,'graphify-out/graph.json')],cwd);
    const output=clip(r.stdout,budget), ranked=r.status===0?files(output,cwd):[];
    const expected=[...new Set(item.expected_files)];const rank=ranked.findIndex(f=>expected.includes(f))+1;
    const returned=output.split('\n').filter(l=>l.startsWith('NODE ')).map(l=>{const m=l.match(/src=(.*?) (?:loc|community)=/); const source=m?.[1]||'';return {source_file:source.replace(/:\d+(?::\d+)?$/,''),source_line:Number(l.match(/ loc=L(\d+)/)?.[1]||source.match(/:(\d+)(?::\d+)?$/)?.[1])};});
    const definitions=(item.definitions||[]).map(d=>({...d,rank:r.status===0?(returned.findIndex(n=>definitionMatch(n,d,cwd,tool))+1)||null:null}));
    const row={definitions,tool,budget,id:item.id,question:item.question,expected,seconds:r.seconds,status:r.status,error:r.error,stderr:r.stderr,raw_tokens:tok.count(r.stdout),tokens:tok.count(output),clipped:output!==r.stdout,rank:rank||null,files:ranked,recall:Object.fromEntries([1,3,5,10].map(k=>[k,expected.filter(f=>ranked.slice(0,k).includes(f)).length/expected.length])),stdout:r.stdout};
    corpus.queries.push(row);save();console.log(`${spec.name} ${budget} ${tool} ${item.id} rank=${rank||'miss'} ${r.seconds.toFixed(2)}s`);
  }
  for(const budget of BUDGETS)for(const tool of ['astria','graphify']){
    const rows=corpus.queries.filter(r=>r.tool===tool&&r.budget===budget),n=rows.length;
    const avg=k=>rows.reduce((s,r)=>s+r[k],0)/n;
    corpus.summaries.push({tool,budget,n,errors:rows.filter(r=>r.status!==0).length,avg_seconds:avg('seconds'),median_seconds:rows.map(r=>r.seconds).sort((a,b)=>a-b)[Math.floor(n/2)],avg_tokens:avg('tokens'),avg_raw_tokens:avg('raw_tokens'),over_budget:rows.filter(r=>r.raw_tokens>budget).length,mrr:rows.reduce((s,r)=>s+(r.rank?1/r.rank:0),0)/n,...Object.fromEntries([1,3,5,10].flatMap(k=>[[`hit@${k}`,rows.filter(r=>r.rank&&r.rank<=k).length/n],[`recall@${k}`,rows.reduce((s,r)=>s+r.recall[k],0)/n]]))});
  }save();console.log('SUMMARY '+spec.name+' '+JSON.stringify(corpus.summaries));
}
console.log('RESULTS '+path.join(root,'results.json'));
