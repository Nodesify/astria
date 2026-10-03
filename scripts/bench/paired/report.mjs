import {readFileSync,writeFileSync} from 'node:fs';
import path from 'node:path';
const input=process.argv[2];
if(!input) throw Error('Usage: node scripts/bench/paired/report.mjs <results.json>');
const data=JSON.parse(readFileSync(input,'utf8'));
const pct=x=>(100*x).toFixed(1)+'%';
let out='# Paired retrieval measurements\n\n'+data.date+'; schema '+data.schema_version+'. Each corpus/split is reported separately. Single measurements include process startup; no statistical superiority claim.\n\n';
for(const c of data.corpora){
  out+=`## ${c.name} (${c.split||'unspecified'})\n\nCorpus ${c.commit}; golden SHA-256 ${c.golden_sha256}.\n\n`;
  // The metric trio retrieval validation requires side by side: exact-symbol
  // ranking, file recall, and delivered tokens. A change that improves one
  // while regressing another must be visible in a single table.
  out+='| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n';
  for(const s of c.summaries){
    const definitions=c.queries.filter(q=>q.tool===s.tool&&q.budget===s.budget).flatMap(q=>q.definitions||[]);
    const hits=definitions.filter(d=>d.rank);
    const defRecall5=definitions.length?definitions.filter(d=>d.rank&&d.rank<=5).length/definitions.length:null;
    const defMrr=definitions.length?hits.reduce((sum,d)=>sum+1/d.rank,0)/definitions.length:null;
    out+=`| ${s.tool} | ${s.budget} | ${s.n} | ${pct(s['recall@5'])} | ${s.mrr.toFixed(3)} | ${defRecall5===null?'n/a':pct(defRecall5)} | ${defMrr===null?'n/a':defMrr.toFixed(3)} | ${Math.round(s.avg_tokens)} | ${s.over_budget} | ${s.errors} |\n`;
  }
  for(const [tool,b] of Object.entries(c.builds)){
    out+=`\n${tool}: build ${b.seconds.toFixed(3)} s; grounded definitions present ${b.definitions.filter(d=>d.present).length}/${b.definitions.length}.`;
    if(b.retention)out+=` Exact cached code IDs preserved: ${b.retention.preserved}/${b.retention.cached_code_definitions}.`;
    out+='\n';
  }
  // Failure-mode triage: every definition outside the top five, with its
  // rank or miss, per tool and budget. Same-name symbol collisions and
  // prose displacing code results surface here first.
  for(const s of c.summaries){
    const misses=c.queries
      .filter(q=>q.tool===s.tool&&q.budget===s.budget)
      .flatMap(q=>(q.definitions||[]).filter(d=>!d.rank||d.rank>5).map(d=>({question:q.id,symbol:d.symbol,contains:d.contains,rank:d.rank})));
    if(misses.length===0)continue;
    out+=`\nDefinition misses (outside top five), ${s.tool} @ ${s.budget}:\n`;
    for(const m of misses.slice(0,10)){
      out+=`- ${m.question} ${m.symbol||m.contains}: ${m.rank?`rank ${m.rank}`:'not returned'}\n`;
    }
    if(misses.length>10)out+=`- … ${misses.length-10} more\n`;
  }
  out+='\n';
}
out+='Definition ranks count returned nodes, file ranks count distinct paths. Presence matches declaration file and line; retention matches exact scoped extraction IDs. File hits do not imply that the right symbol survived. No answer correctness was measured.\n';
writeFileSync(path.join(path.dirname(input),'comparison.md'),out);
console.log(out);
