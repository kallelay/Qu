import React from 'react';
import {createRoot} from 'react-dom/client';
import {GuiPanel, GuiMessage} from './src/GuiPanel';
import './src/index.css';
import './src/seriplot.css';
let handler: (message:GuiMessage)=>void;
const subscribe = async (next: typeof handler) => {handler=next;return ()=>{};};
const invoke = async <T,>(command:string,args:any):Promise<T> => {
 const response=await fetch('/gui-review-api',{method:'POST',body:JSON.stringify({command,args})});
 const data=await response.json();if(data.error)throw Error(data.error);return data as T;
};
setInterval(async()=>{const response=await fetch('/gui-review-api');for(const message of await response.json())handler?.(message);},100);
createRoot(document.getElementById('root')!).render(<div className="qu-inspector" data-theme="light" style={{height:'100vh',display:'flex'}}><GuiPanel theme="light" invoke={invoke} subscribe={subscribe}/></div>);
