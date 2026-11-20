import React, {useEffect,useState,useId} from 'react';
import {GlassCard,GlassSegmented} from '@zakisheriff/liquid-glass';
import {ResponsiveContainer,AreaChart,Area,XAxis,YAxis,CartesianGrid,Tooltip} from 'recharts';
import {groupDevices} from './devices.mjs';

const number=value=>value==null||String(value).trim()===''?null:Number.isFinite(parseFloat(value))?parseFloat(value):null;
export function monitorSample(data,previous) {
  const counters=data.optical?.counters?.[0],optical=data.optical?.info?.[0];
  const time=data.sampled_at,rx=number(counters?.bytes_received),tx=number(counters?.bytes_sent);
  const dt=previous?time-previous.time:0;
  const valid=dt>0&&dt<=60;
  return {time,rx,tx,download:valid&&rx!=null&&previous.rx!=null&&rx>=previous.rx?(rx-previous.rx)*8/dt/1e6:null,
    upload:valid&&tx!=null&&previous.tx!=null&&tx>=previous.tx?(tx-previous.tx)*8/dt/1e6:null,
    cpu:number(data.device?.cpu),memory:number(data.device?.memory),opticalRx:number(optical?.rx_dbm),opticalTx:number(optical?.tx_dbm),
    online:Array.isArray(data.clients)?groupDevices(data.clients).filter(c=>c.status==='Online').length:null};
}

function useHistory(range) {
  const [samples,setSamples]=useState([]),[error,setError]=useState(''),[loading,setLoading]=useState(true);
  useEffect(()=>{let active=true;const controller=new AbortController();setSamples([]);setLoading(true);async function load(){try{const response=await fetch(`/api/history?days=${[1,7,30,90][range]}`,{signal:controller.signal});const body=await response.json();if(!response.ok)throw new Error(body.error||'History unavailable');if(active){setSamples(body.samples);setError('');}}catch(e){if(active&&e.name!=='AbortError')setError(e.message);}finally{if(active)setLoading(false);}}load();const timer=setInterval(load,15000);return()=>{active=false;controller.abort();clearInterval(timer);};},[range]);
  return {samples,error,loading};
}
function Chart({title,unit,series,domain,samples}) {
  const id=useId().replace(/:/g,'');
  const latest=samples.at(-1);
  const longRange=samples.length>1&&latest.time-samples[0].time>86400;
  return <GlassCard><div className="chart-heading"><h3>{title}</h3></div><div className="chart-values">{series.map(([key,name,color])=><div key={key}><span className="chart-series-name"><i style={{background:color}}/>{name}</span><strong>{latest?.[key]==null?'—':Number(latest[key]).toLocaleString('en-GB',{maximumFractionDigits:unit==='devices'?1:2})}<small>{unit}</small></strong></div>)}</div><div className="monitor-chart" role="img" aria-label={`${title}, ${samples.length} samples, unit ${unit}`}>
    <ResponsiveContainer width="100%" height="100%"><AreaChart data={samples} margin={{top:16,right:12,bottom:0,left:0}}>
      <defs>{series.map(([key,,color])=><linearGradient key={key} id={`${id}-${key}`} x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor={color} stopOpacity={0.3}/><stop offset="100%" stopColor={color} stopOpacity={0.01}/></linearGradient>)}</defs>
      <CartesianGrid stroke="currentColor" strokeOpacity={0.09} vertical={false} strokeDasharray="3 6"/>
      <XAxis dataKey="time" type="number" domain={['dataMin','dataMax']} axisLine={false} tickLine={false} tickMargin={12} tickFormatter={v=>new Date(v*1000).toLocaleString('en-GB',longRange?{day:'numeric',month:'short'}:{hour:'2-digit',minute:'2-digit'})} minTickGap={60} tick={{fill:'currentColor',fontSize:11}}/>
      <YAxis width={48} axisLine={false} tickLine={false} domain={domain||['auto','auto']} tick={{fill:'currentColor',fontSize:11}}/>
      <Tooltip cursor={{stroke:'currentColor',strokeOpacity:0.2,strokeDasharray:'4 4'}} content={<ChartTooltip unit={unit}/>}/>
      {series.map(([key,name,color])=><Area key={key} type={unit==='devices'?'stepAfter':'linear'} dataKey={key} name={name} stroke={color} fill={`url(#${id}-${key})`} strokeWidth={2.5} dot={samples.length<2} activeDot={{r:5,strokeWidth:3}} isAnimationActive={false} connectNulls={false}/>)}
    </AreaChart></ResponsiveContainer></div>
  </GlassCard>;
}
function ChartTooltip({active,payload,label,unit}) {
  if(!active||!payload?.length)return null;
  return <div className="chart-tooltip"><div className="chart-tooltip-time">{new Date(Number(label)*1000).toLocaleString('en-GB',{day:'numeric',month:'short',hour:'2-digit',minute:'2-digit',second:'2-digit'})}</div>{payload.filter(item=>item.value!=null).map(item=><div className="chart-tooltip-row" key={item.dataKey}><span><i style={{background:item.color}}/>{item.name}</span><strong>{Number(item.value).toLocaleString('en-GB',{maximumFractionDigits:2})}<small>{unit}</small></strong></div>)}</div>;
}
function Gauge({title,value,color}) {
  const valid=typeof value==='number'&&Number.isFinite(value);
  const progress=valid?Math.max(0,Math.min(100,value)):0;
  return <GlassCard><div className="gauge-content"><h3>{title}</h3><div className="gauge-dial" role="meter" aria-label={`${title}, latest bucket average`} aria-valuemin={0} aria-valuemax={100} {...(valid?{'aria-valuenow':progress}:{'aria-valuetext':'No reading'})}>
    <svg viewBox="0 0 200 132" aria-hidden="true"><path d="M 25 105 A 75 75 0 1 1 175 105" fill="none" stroke="currentColor" opacity=".1" strokeWidth="12" strokeLinecap="round"/><path d="M 25 105 A 75 75 0 1 1 175 105" fill="none" stroke={color} strokeWidth="12" strokeLinecap="round" pathLength="100" strokeDasharray={`${progress} 100`}/><text x="100" y="91" textAnchor="middle" fill="currentColor" className="gauge-number">{valid?`${value.toFixed(0)}%`:'—'}</text><text x="20" y="129" fill="currentColor" className="gauge-limit">0</text><text x="180" y="129" textAnchor="end" fill="currentColor" className="gauge-limit">100%</text></svg>
    </div><p>Latest interval average</p></div></GlassCard>;
}
export default function Monitor() {
  const [range,setRange]=useState(0);
  const {samples,error,loading}=useHistory(range);
  return <section id="monitor"><div className="chart-heading"><h2>Monitor</h2><GlassSegmented size="sm" options={['1D','1W','1M','3M']} value={range} onChange={setRange} aria-label="Monitor time range"/></div>
    {loading&&<p role="status">Loading history…</p>}{error&&<p role="alert">{error}</p>}{!loading&&!samples.length&&<p>No readings in this range yet.</p>}
    <div className="monitor-grid">
      <Gauge title="CPU utilization" value={samples.at(-1)?.cpu} color="#ff9500"/>
      <Gauge title="Memory utilization" value={samples.at(-1)?.memory} color="#0088ff"/>
      <Chart samples={samples} title="GPON throughput" unit="Mbps" domain={[0,'auto']} series={[["download","Download","#0088ff"],["upload","Upload","#af52de"]]}/>
      <Chart samples={samples} title="Optical power" unit="dBm" series={[["opticalRx","Receive","#0088ff"],["opticalTx","Transmit","#af52de"]]}/>
      <Chart samples={samples} title="System utilization" unit="%" domain={[0,100]} series={[["cpu","CPU","#ff9500"],["memory","Memory","#0088ff"]]}/>
      <Chart samples={samples} title="Online devices" unit="devices" domain={[0,'auto']} series={[["online","Unique MACs online","#28a745"]]}/>
    </div>
  </section>;
}
