import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const root=process.argv[2] || path.resolve(path.dirname(new URL(import.meta.url).pathname),'../../../scratchpad/target/analysis/co-development');
const out=process.argv[3] || path.dirname(new URL(import.meta.url).pathname);
const read=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const median=a=>{a=[...a].sort((a,b)=>a-b);return a.length?a.length%2?a[(a.length-1)/2]:(a[a.length/2-1]+a[a.length/2])/2:null};
const pointer=(o,p)=>p?.split('/').slice(1).reduce((v,k)=>v?.[k.replaceAll('~1','/').replaceAll('~0','~')],o);
const result={schema:1,description:'Observed measurements across paired cycles 1–25. No interpolation; unknown is not zero. All times in milliseconds. Ranges are observed, not confidence intervals.',source_root:root,sources:[],observations:[],audits:[]};
for(const dir of fs.readdirSync(root).filter(d=>/^(cycle-\d+|promises-after-cycle-\d+)/.test(d)).sort((a,b)=>a.localeCompare(b,undefined,{numeric:true}))){
 const cycle=Number(dir.match(/cycle-(\d+)/)[1]);if(cycle>25)continue;
 for(const [file,kind] of [['resource_profiles.json','resource'],['capacity_report.json','capacity'],['search_workflow_metrics.json','workflow'],['performance_review.json','review'],['measure_run.json','run']]){
  const p=path.join(root,dir,file);if(!fs.existsSync(p))continue;const bytes=fs.readFileSync(p),d=JSON.parse(bytes);const source=`${dir}/${file}`,pp=p+'.provenance.json',prov=fs.existsSync(pp)?read(pp):null;
  result.sources.push({path:source,sha256:crypto.createHash('sha256').update(bytes).digest('hex'),quality:prov?.measurement_quality||null,context:prov?.context||null,status:d.meta?.probe_status||d.status||null});
  if(kind==='review'){result.audits.push({cycle,source,summary:d.summary,noisy_sources:d.meta.noisy_sources,scenarios:d.scenarios.map(s=>({id:s.id,title:s.title,health:s.promise_health,coverage:s.coverage_status,budget_misses:s.budget_misses,scale_checks:s.scale_checks,gaps:s.gaps,baseline_comparison:s.baseline_comparison,axes:s.axes}))});continue;}
  if(kind==='run'){const audit=result.audits.find(a=>a.cycle===cycle);if(audit)audit.run=d;continue;}
  for(const [si,s] of d.scenarios.entries()){
   const base={cycle,run:dir,kind,source,scenario_index:si,id:s.scenario||s.name,label:s.scenario_label||s.name,quality:prov?.measurement_quality?.status||'unknown'};
   if(kind==='workflow'){result.observations.push({...base,workload:s.parameter_value,budget_ms:s.threshold_ms,median_ms:s.median_ms,min_ms:Math.min(...s.sample_elapsed_ms),max_ms:s.max_ms,values_ms:s.sample_elapsed_ms,status:s.budget_status,valid:s.measurement_status,scope:s.measurement_scope,promise_workload_met:s.promise_workload_met});continue;}
   const groups=Map.groupBy?Map.groupBy(s.samples,x=>x.workload_value):s.samples.reduce((m,x)=>m.set(x.workload_value,[...(m.get(x.workload_value)||[]),x]),new Map());
   for(const [workload,ss] of groups){
    const budget=kind==='resource'?ss[0].resource_budget:null,metric=budget?.metric||'/elapsed_ns';
    const values=kind==='resource'?ss.map(x=>pointer(x,metric)).filter(x=>typeof x==='number').map(x=>x/1e6):ss.flatMap(x=>x.raw_repetitions?.length?x.raw_repetitions.map(r=>r.elapsed_ns/1e6):[x.elapsed_ms]).filter(x=>typeof x==='number'&&Number.isFinite(x));
    const local=(kind==='capacity'?ss.flatMap(x=>x.raw_repetitions||[]):ss).map(x=>x.launch_measurement_quality?.status).filter(Boolean);
    result.observations.push({...base,workload,unit:ss[0].workload_unit,metric,budget_ms:kind==='resource'?(budget?.unit==='ns'?budget.max/1e6:null):s.threshold_ms,median_ms:kind==='capacity'?ss[0].elapsed_ms:median(values),min_ms:values.length?Math.min(...values):null,max_ms:values.length?Math.max(...values):null,values_ms:values,status:s.budget_status||ss[0].status,sample_statuses:ss.map(x=>x.budget_status||x.status),sample_indices:s.samples.map((x,i)=>x.workload_value===workload?i:null).filter(x=>x!==null),quality:local.length?(local.every(x=>x==='stable')?'stable':local.includes('noisy')?'noisy':'unknown'):base.quality,scopes:[...new Set(ss.map(x=>x.measurement_scope))],contracts:ss.map(x=>x.measurement_contract),fixture_valid:ss.map(x=>x.fixture_valid),complete:ss.map(x=>x.complete),closes_measurement_gap:s.closes_measurement_gap,within_budget_workload:s.within_budget_workload??null,phases:kind==='resource'?ss.map(x=>x.phase_elapsed_ns):undefined,allocated_bytes:kind==='resource'?ss.map(x=>x.allocated_bytes):undefined});
   }
  }
 }
}
fs.mkdirSync(out,{recursive:true});fs.writeFileSync(path.join(out,'data.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({sources:result.sources.length,observations:result.observations.length,audits:result.audits.map(a=>a.cycle)}));

const columns=['cycle','run','kind','id','workload','budget_ms','median_ms','min_ms','max_ms','quality','status','source'];
const csv=v=>'"'+String(v??'').replaceAll('"','""')+'"';
fs.writeFileSync(path.join(out,'observations.csv'),[columns.join(','),...result.observations.map(o=>columns.map(k=>csv(o[k])).join(','))].join('\n')+'\n');
