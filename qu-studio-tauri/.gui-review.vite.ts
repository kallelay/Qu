import config from './vite.config';
import {spawn} from 'node:child_process';
import {writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
let child:any;let messages:any[]=[];
export default {...config,cacheDir:join(tmpdir(),'qu-gui-review-cache'),server:{host:'127.0.0.1',port:1422,strictPort:true},plugins:[...(config.plugins??[]),{name:'gui-review',configureServer(server:any){
 server.httpServer.on('close',()=>child?.kill());
 server.middlewares.use('/gui-review-api',(req:any,res:any)=>{
  res.setHeader('Content-Type','application/json');
  if(req.method==='GET'){res.end(JSON.stringify(messages.splice(0)));return;}
  let body='';req.on('data',(chunk:any)=>body+=chunk);req.on('end',()=>{try{
   const {command,args}=JSON.parse(body);
   if(command==='gui_start'){
    child?.kill();const path=join(tmpdir(),'qu-gui-review.qu');writeFileSync(path,args.code);
    const id=args.id;child=spawn(resolve('../engine/target/debug/qu.exe'),['gui',path],{windowsHide:true});let buffer='';
    child.stdout.on('data',(chunk:any)=>{buffer+=chunk.toString();let i;while((i=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,i);buffer=buffer.slice(i+1);try{messages.push({id,packet:JSON.parse(line)});}catch{messages.push({id,error:line});}}});
    child.stderr.on('data',(chunk:any)=>messages.push({id,error:chunk.toString()}));child.on('exit',()=>messages.push({id,done:true}));
   }else if(command==='gui_event'){child.stdin.write(JSON.stringify({target:args.target,event:args.event,value:args.value})+'\n');}
   else if(command==='gui_stop')child?.kill();res.end('{}');
  }catch(error){res.end(JSON.stringify({error:String(error)}));}});
 });
}}]};
