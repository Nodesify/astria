import {readFileSync,writeFileSync} from 'node:fs';
import path from 'node:path';
const input=process.argv[2];
if(!input) throw Error('Usage: node scripts/bench/paired/report.mjs <results.json>');
const data=JSON.parse(readFileSync(input,'utf8'));
const pct=x=>(100*x).toFixed(1)+'%';
let out='# Paired retrieval measurements\n\n'+data.date+'; schema '+data.schema_version+'. Each corpus/split is reported separately. Single measurements include process startup; no statistical superiority claim.\n\n';
for(const c of data.corpora){
  out+=`## ${c.name} (${c.split||'unspecified'})\n\nCorpus ${c.commit}; golden SHA-256 ${c.golden_sha256}.\n\n`;
  out+='| Tool | Budget | Questions | File recall@5 | File MRR | Raw over budget | Errors | Definition recall@5 |\n|---|---:|---:|---:|---:|---:|---:|---:|\n';
  for(const s of c.summaries){
    const definitions=c.queries.filter(q=>q.tool===s.tool&&q.budget===s.budget).flatMap(q=>q.definitions||[]);
    out+=`| ${s.tool} | ${s.budget} | ${s.n} | ${pct(s['recall@5'])} | ${s.mrr.toFixed(3)} | ${s.over_budget} | ${s.errors} | ${definitions.length?pct(definitions.filter(d=>d.rank&&d.rank<=5).length/definitions.length):'n/a'} |\n`;
  }
  for(const [tool,b] of Object.entries(c.builds)){
    out+=`\n${tool}: build ${b.seconds.toFixed(3)} s; grounded definitions present ${b.definitions.filter(d=>d.present).length}/${b.definitions.length}.`;
    if(b.retention)out+=` Exact cached code IDs preserved: ${b.retention.preserved}/${b.retention.cached_code_definitions}.`;
    out+='\n';
  }
  out+='\n';
}
out+='Definition ranks count returned nodes, file ranks count distinct paths. Presence matches declaration file and line; retention matches exact scoped extraction IDs. File hits do not imply that the right symbol survived. No answer correctness was measured.\n';
writeFileSync(path.join(path.dirname(input),'comparison.md'),out);
console.log(out);
