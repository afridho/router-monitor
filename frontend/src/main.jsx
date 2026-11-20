import React, {useEffect, useRef, useState} from 'react';
import {createRoot} from 'react-dom/client';
import {GlassButton, GlassInput, GlassSheet, GlassSwitch, GlassSegmented, GlassNav, GlassCard, GlassMenu} from '@zakisheriff/liquid-glass';
import '@zakisheriff/liquid-glass/styles.css';
import './style.css';
import {groupDevices} from './devices.mjs';
import Monitor, {monitorSample} from './Monitor';

// Initialize before glass mounts: its mirrored lenses capture ancestor state.
document.documentElement.dataset.theme=matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light';

async function api(path, body) {
  const response = await fetch(path, body === undefined ? {cache:'no-store'} : {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  const data = await response.json();
  if (!response.ok) throw new Error(data.error || 'Request failed');
  return data;
}
function bytes(value) {
  let n=Number(value); if(value==null || !Number.isFinite(n)) return 'Not reported';
  const units=['B','KiB','MiB','GiB','TiB'];let i=0;
  while(n>=1024&&i<4){n/=1024;i++;}return `${n.toFixed(i?2:0)} ${units[i]}`;
}
function ReadingRows({rows}) {
  return <dl>{rows.filter(([,v])=>v!=null&&v!=='').map(([label,value])=><div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>;
}
function Readings({title, rows,details=[]}) {
  return <GlassCard><h3>{title}</h3><ReadingRows rows={rows}/>{details.some(([,v])=>v!=null&&v!=='')&&<details className="reading-details"><summary>Technical details</summary><ReadingRows rows={details}/></details>}</GlassCard>;
}
function Table({headings,children}) {
  return <div className="interface-grid">{React.Children.toArray(children).map((row,i)=><GlassCard key={row.key||i}><dl>{React.Children.toArray(row.props.children).map((cell,j)=><div key={headings[j]}><dt>{headings[j]}</dt><dd>{cell.props.children}</dd></div>)}</dl></GlassCard>)}</div>;
}
function clientBand(client,wifi) {
  if(client.status!=='Online')return null;
  if(client.wifi_source==='association_table')return client.wifi_band;
  const port=String(client.port||'');
  const match=port.match(/^(?:SSID|WLAN)(\d+)$/i);
  const network=wifi.find(w=>w.interface===port||w.path===port||(match&&w.path.split('.').at(-1)===String(Number(match[1]))));
  return network?.radio?.endsWith('.2')?'5 GHz':network?.radio?.endsWith('.1')?'2.4 GHz':null;
}
function BandSummary({clients,wifi}) {
  const bands=clients.map(c=>clientBand(c,wifi));
  return <p className="description">Verified direct Wi-Fi: {bands.filter(b=>b==='2.4 GHz').length} on 2.4 GHz · {bands.filter(b=>b==='5 GHz').length} on 5 GHz. LAN interfaces do not identify the band of a downstream access point.</p>;
}
function DeviceAddresses({device}) {
  return <><p className="device-address mono">{device.ip||'No IP address'} · {device.mac}</p>{device.addresses?.length>1&&<details className="device-address-details"><summary>{device.addresses.length-1} additional router address records</summary><p>Includes older IPv6 records retained by the router; these are not necessarily active.</p>{device.addresses.filter(ip=>ip!==device.ip).map(ip=><p className="mono" key={ip}>{ip}</p>)}</details>}</>;
}
function App() {
  const [systemTheme,setSystemTheme]=useState(document.documentElement.dataset.theme);
  useEffect(()=>{const system=matchMedia('(prefers-color-scheme: dark)');const sync=()=>{const theme=system.matches?'dark':'light';document.documentElement.dataset.theme=theme;setSystemTheme(theme);};sync();system.addEventListener('change',sync);return()=>system.removeEventListener('change',sync);},[]);
  const [tab,setTab]=useState(0);
  const [data,setData]=useState(null),[status,setStatus]=useState('Checking router connection...');
  const [authenticated,setAuthenticated]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState('');
  const [open,setOpen]=useState(false),[polling,setPolling]=useState(true),[query,setQuery]=useState(''),[filter,setFilter]=useState(0);
  const [username,setUsername]=useState(''),[password,setPassword]=useState(''),[feedback,setFeedback]=useState(''),[loggingIn,setLoggingIn]=useState(false);
  const [rate,setRate]=useState(null);
  const [samples,setSamples]=useState([]);
  const [labels,setLabels]=useState({});
  const [labelError,setLabelError]=useState(''),[labelSaving,setLabelSaving]=useState(false);
  async function labelRequest(input){const response=await fetch('/api/labels',input?{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(input)}:undefined);const body=await response.json();if(!response.ok)throw new Error(body.error||'Unable to load device labels.');return body;}
  useEffect(()=>{let active=true;async function load(){try{let saved=await labelRequest();let legacy={};try{legacy=JSON.parse(localStorage.getItem('router-device-labels')||'{}');}catch{}if(legacy&&typeof legacy==='object'&&!Array.isArray(legacy)&&Object.keys(legacy).length){saved=await labelRequest({labels:legacy,import:true});localStorage.removeItem('router-device-labels');}if(active){setLabels(saved);setLabelError('');}}catch(e){if(active)setLabelError(e.message);}}load();return()=>{active=false;};},[]);
  const [labelDevice,setLabelDevice]=useState(null),[labelValue,setLabelValue]=useState('');
  async function saveLabel(event){event.preventDefault();setLabelSaving(true);try{const saved=await labelRequest({labels:{[labelDevice]:labelValue.trim()}});setLabels(saved);setLabelError('');setLabelDevice(null);}catch(e){setLabelError(e.message);}finally{setLabelSaving(false);}}
  const working=useRef(false),previous=useRef(null),connectionButton=useRef(null);
  const closeSheet=()=>{setOpen(false);requestAnimationFrame(()=>connectionButton.current?.focus());};
  async function refresh() {
    if(working.current)return;working.current=true;setBusy(true);
    try {
      const state=await api('/api/status');setAuthenticated(state.authenticated);
      setStatus(state.authenticated?'Router connected':state.reachable?'Router reachable · Sign in to load readings':'Router unreachable · Check the laptop network');
      if(state.authenticated){
        const next=await api('/api/telemetry');const counter=next.optical?.counters?.[0];
        if(counter){const now={time:next.sampled_at,rx:Number(counter.bytes_received),tx:Number(counter.bytes_sent)},old=previous.current;
          setRate(old&&now.time>old.time&&now.rx>=old.rx&&now.tx>=old.tx?{rx:((now.rx-old.rx)*8/(now.time-old.time)/1e6).toFixed(2),tx:((now.tx-old.tx)*8/(now.time-old.time)/1e6).toFixed(2)}:null);previous.current=now;}
        setData(next);setSamples(history=>{if(history.at(-1)?.time>=next.sampled_at)return history;return [...history,monitorSample(next,history.at(-1))].slice(-240);});setError((next.errors||[]).join(' '));
      }else{setData(null);previous.current=null;setRate(null);setSamples([]);}
    }catch(e){setError(e.message);}finally{working.current=false;setBusy(false);}
  }
  useEffect(()=>{refresh();},[]);
  useEffect(()=>{if(!polling)return;const timer=setInterval(()=>{if(!document.hidden)refresh();},15000);return()=>clearInterval(timer);},[polling]);
  async function login(event){event.preventDefault();setLoggingIn(true);setFeedback('Verifying router session...');try{await api('/api/login',{username,password});setFeedback('Connected.');await refresh();closeSheet();}catch(e){setFeedback(e.message);}finally{setPassword('');setLoggingIn(false);}}
  async function disconnect(){setLoggingIn(true);try{await api('/api/disconnect',{});await refresh();setFeedback('Local session disconnected.');}catch(e){setFeedback(e.message);}finally{setLoggingIn(false);}}
  const dev=data?.device?.info?.[0]||{},ont=data?.device?.ont?.[0]||{},optic=data?.optical?.info?.[0]||{},counter=data?.optical?.counters?.[0];
  const clients=groupDevices(data?.clients||[]),filtered=clients.filter(c=>(filter===0||(filter===1?c.status==='Online':filter===2?c.status==='Offline':clientBand(c,data?.wifi||[])===(filter===3?'2.4 GHz':'5 GHz')))&&[labels[c.mac],c.hostname,c.ip,c.mac,c.port].some(v=>String(v||'').toLowerCase().includes(query.toLowerCase())));
  return <React.Fragment key={systemTheme}>
    <main aria-busy={busy}><div className="dashboard-top"><div className="dashboard-title"><h1>Router Monitor</h1><p id="status" role="status">{status==='Router connected'?`Router Connected${dev.model?` - ${dev.model}`:''}`:status}</p></div><div className="dashboard-actions"><div className="toolbar-actions">
      <GlassButton id="refresh" disabled={busy} onClick={refresh}>{busy?'Reading router...':'Refresh readings'}</GlassButton>
      <label className="polling"><span>Auto-refresh</span><GlassSwitch checked={polling} onChange={setPolling} aria-label="Auto-refresh"/></label>
      <GlassButton id="session" ref={connectionButton} onClick={()=>setOpen(true)}>Connection</GlassButton>
    </div><span id="updated">{data?`Updated ${new Date(data.sampled_at*1000).toLocaleTimeString('en-GB')}`:'No sample yet'}</span></div></div>
      {error&&<div id="error" role="alert">{error}</div>}
      <div className="network-navigation"><GlassNav items={[{label:'Monitor'},{label:'Device & optical'},{label:'Connected devices'},{label:'Interfaces'}]} activeIndex={tab} onChange={setTab} aria-label="Network sections"/></div>
      {!data?<div id="empty"><h2>{busy?'Loading router readings...':'Connect to load router readings'}</h2><p>Read device health, optical signal, client inventory, Wi-Fi networks, and Ethernet counters.</p><GlassButton onClick={()=>setOpen(true)}>Open connection</GlassButton></div>:<div id="readings">
        {tab===0&&<Monitor samples={samples}/>}
        <section id="health" hidden={tab!==1}><div className="section-title"><h2>Device & optical</h2></div><div className="health">
          <Readings title="System" rows={[["Model",dev.model],["CPU utilization",data.device?.cpu],["Memory utilization",data.device?.memory],["NAT sessions (current / maximum)",data.device?.nat_sessions],["ONT state",ont.status],["ONT ID",ont.id]]} details={[["Firmware",dev.firmware],["Hardware",dev.hardware],["LAN MAC",dev.mac]]}/>
          <Readings title="Optical transceiver" rows={[["Link",optic.link?.toLowerCase()==='ok'?'Connected':optic.link],["Receive power",optic.rx_dbm&&`${optic.rx_dbm} dBm`],["Transmit power",optic.tx_dbm&&`${optic.tx_dbm} dBm`],["Temperature",optic.temperature_c&&`${optic.temperature_c} °C`],["Loss of signal",optic.los==='0'?'Not detected':optic.los==='1'?'Detected':null]]} details={[["Voltage",optic.voltage_mv&&`${(Number(optic.voltage_mv)/1000).toFixed(3)} V`],["Bias current",optic.bias_ma&&`${optic.bias_ma} mA`],["Wavelength (TX / RX)",optic.tx_nm&&`${optic.tx_nm} / ${optic.rx_nm} nm`],["Vendor",optic.vendor]]}/>
          <GlassCard><h3>GPON traffic & totals</h3><div className="traffic-summary"><div><span>Download</span><strong>{rate?rate.rx:'—'}<small>Mbps</small></strong></div><div><span>Upload</span><strong>{rate?rate.tx:'—'}<small>Mbps</small></strong></div></div>{!rate&&<p className="description">Waiting for two traffic samples.</p>}<ReadingRows rows={[["Received total",counter?bytes(counter.bytes_received):'Not reported'],["Sent total",counter?bytes(counter.bytes_sent):'Not reported']]}/><details className="reading-details"><summary>Counter details</summary><ReadingRows rows={[["Receive errors",counter?.errors_received],["Transmit errors",counter?.errors_sent],["Discarded packets",counter?.discards_received]]}/><p className="description">Totals since counter reset, not monthly usage. Throughput is counter-derived, not a speed test.</p></details></GlassCard>
        </div></section>
        <section id="clients" hidden={tab!==2}><div className="devices-toolbar"><div><h2>Connected devices</h2><p>{filtered.length} shown · {clients.length} devices · {(data?.clients||[]).length} address records</p></div><div className="device-search"><GlassInput id="search" label="Filter devices" type="search" value={query} onChange={e=>setQuery(e.target.value)} placeholder="Label, hostname, IP, or MAC"/></div></div>
          <div className="client-controls"><GlassSegmented options={['All devices','Online','Offline','2.4 GHz','5 GHz']} value={filter} onChange={setFilter} aria-label="Device state filter"/></div><p className="description">Band filters include only online devices with a verified Huawei Wi-Fi interface. Devices behind a LAN-connected access point do not report their Wi-Fi band here. Labels are stored only in this browser.</p>
          <div className="device-grid">{filtered.map((c,i)=><GlassCard key={`${c.mac}-${i}`}><article className="device-content"><div className="device-heading"><div><h3>{labels[c.mac]||c.hostname||'Unnamed device'}</h3>{labels[c.mac]&&<p className="device-original">{c.hostname||'Unnamed device'}</p>}</div><GlassMenu icon={<span aria-hidden="true">⋯</span>} aria-label={`Actions for ${labels[c.mac]||c.hostname||c.mac}`} items={[{label:'Edit label',onSelect:()=>{setLabelDevice(c.mac);setLabelValue(labels[c.mac]||'');}}]}/></div><DeviceAddresses device={c}/><div className="device-facts"><span data-status={String(c.status||'unknown').toLowerCase()}>{c.status||'Unknown state'}</span><span>{c.wifi_band||c.port||'Unknown interface'}</span>{c.type&&c.type!=='--'&&<span>{c.type}</span>}{(parseInt(c.mac?.split(':')[0],16)&2)!==0&&c.mac&&<span>Private MAC</span>}</div><dl className="device-details">{c.time&&<div><dt>Connected time</dt><dd>{c.time}</dd></div>}{c.receive_rate&&<div><dt>Reported receive rate</dt><dd>{c.receive_rate}</dd></div>}{c.send_rate&&<div><dt>Reported send rate</dt><dd>{c.send_rate}</dd></div>}</dl></article></GlassCard>)}</div>{!filtered.length&&<p id="client-empty">No devices match this filter. Clear the search or select All devices.</p>}
        </section>
        <section id="interfaces" hidden={tab!==3}><div className="section-title"><h2>Interfaces</h2><span>Wi-Fi configuration and Ethernet link state</span></div><h3>Wireless networks</h3><Table headings={['SSID','Band','Channel','Standard','Visibility','State']}>{(data.wifi||[]).map(w=><tr key={w.path}><td>{w.ssid}</td><td>{w.radio.endsWith('.2')?'5 GHz':'2.4 GHz'}</td><td>{w.channel}{w.auto_channel==='1'?' (auto)':''}</td><td>{w.standard}</td><td>{w.hidden==='1'?'Hidden':'Broadcast'}</td><td>{w.enabled==='1'?'Enabled':'Disabled'}</td></tr>)}</Table>
          <h3>Ethernet ports</h3><Table headings={['Port','Link','Sent','Received','TX packets','RX packets']}>{(data.ethernet?.ports||[]).map((p,i)=>{const c=data.ethernet.counters?.[i],sum=(l,h)=>c?bytes(Number(c[l])+Number(c[h])*4294967296):'Not reported';return <tr key={p.path}><td>LAN {i+1}</td><td>{p.status==='1'?'Up':'Down'}</td><td>{sum('tx_bytes','tx_bytes_high')}</td><td>{sum('rx_bytes','rx_bytes_high')}</td><td>{c?.tx_packets??'Not reported'}</td><td>{c?.rx_packets??'Not reported'}</td></tr>;})}</Table>
        </section>
      </div>}
    </main>
    <GlassSheet open={labelDevice!==null} onClose={()=>setLabelDevice(null)} aria-label="Edit local device label"><div className="connection-content"><h2>Device label</h2><p>Saved in the dashboard database, not on the router.</p><p>{labelDevice}</p><form onSubmit={saveLabel}><GlassInput label="Label (empty to remove)" value={labelValue} onChange={e=>setLabelValue(e.target.value)} maxLength={64}/>{labelError&&<p role="alert">{labelError}</p>}<GlassButton type="submit" disabled={labelSaving}>{labelSaving?'Saving…':'Save label'}</GlassButton><GlassButton onClick={()=>setLabelDevice(null)}>Cancel</GlassButton></form></div></GlassSheet>
    <GlassSheet open={open} onClose={closeSheet} aria-label="Router connection"><div className="connection-content"><div className="section-title"><h2>Router connection</h2><GlassButton onClick={closeSheet}>Close</GlassButton></div><p>Connect to the router configured on the server. Credentials are not saved to disk.</p>{authenticated?<><p>Your router session is active.</p><GlassButton id="disconnect" destructive disabled={loggingIn} onClick={disconnect}>Disconnect local session</GlassButton></>:<form id="login" onSubmit={login}><GlassInput label="Username" name="username" autoComplete="username" value={username} onChange={e=>setUsername(e.target.value)} required/><GlassInput label="Password" name="password" type="password" autoComplete="current-password" value={password} onChange={e=>setPassword(e.target.value)} required/><GlassButton type="submit" disabled={loggingIn}>{loggingIn?'Connecting...':'Connect router'}</GlassButton></form>}<p id="feedback" role="status">{feedback}</p></div></GlassSheet>
  </React.Fragment>;
}
createRoot(document.getElementById('app')).render(<App/>);
