import React, {useState} from 'react';
import {createRoot} from 'react-dom/client';
import {PlotViewer} from '../qu-ui-components/src/components/PlotViewer';
import {FigureViewer} from '../qu-ui-components/src/components/FigureViewer';
import './src/index.css';
import svg from '../crest_factor.svg?raw';
const x=Array.from({length:300},(_,i)=>i/30);
const y=x.map(t=>Math.exp(-t/7)*Math.sin(t*2.1));
const data=[{uid:'observed',name:'Observed response',type:'line' as const,x,y},{uid:'envelope',name:'Decay envelope',type:'line' as const,x,y:x.map(t=>Math.exp(-t/7)),line:{dash:'dash'}}];
const images=['data:image/svg+xml;base64,'+btoa(unescape(encodeURIComponent(svg)))];
function Review(){const[theme,setTheme]=useState<'light'|'dark'>('light');const[reversed,setReversed]=useState(false);return <main style={{padding:24,background:theme==='light'?'#eff2f6':'#10141b',minHeight:'100vh',color:theme==='light'?'#263344':'#e0e6ef'}}>
<header style={{display:'flex',gap:20,marginBottom:20}}><strong>Qu Studio · figure design review</strong><button onClick={()=>setTheme(theme==='light'?'dark':'light')}>Toggle theme</button><button onClick={()=>setReversed(!reversed)}>Reorder layers</button></header>
<div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) 350px',gap:20,alignItems:'start'}}>
<PlotViewer title="A damped response with persistent oscillation" subtitle="Two layers, one shared coordinate system · experimental fixture" caption="Source: simulated signal. Dashed line indicates the decay envelope." legendTitle="Signal" xlabel="Time (s)" ylabel="Amplitude (V)" height={650} theme={theme} data={reversed?[...data].reverse():data} />
<div style={{display:'flex',flexDirection:'column',gap:20}}><PlotViewer title="Signal profile" subtitle="300 values · sampled preview" caption="Preview retains the source values." xlabel="Time (s)" ylabel="Amplitude" theme={theme} height={430} data={data.slice(0,1)} showLegend={false}/><FigureViewer images={images} theme={theme}/></div>
</div></main>}
createRoot(document.getElementById('root')!).render(<Review/>);
