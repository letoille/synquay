import type {Route} from './api.ts';

export function combineRoute(route:Route):Route {
  const p=route.processor;
  if(p?.kind==='combined'||p?.kind==='command_set')return route;
  if(p?.kind==='respond') {
    const output=route.command?.output;
    // Preserve older dynamic templates until the editor can represent them safely.
    if(output?.kind!=='json'||typeof (output.template as {content?:unknown})?.content!=='string')return route;
    return {...route,processor:{kind:'combined',forward:false,action:null,reply:{kind:'fixed',text:(output.template as {content:string}).content}},targets:[],command:route.command?{...route.command,output:{kind:'original'}}:null,response:{kind:'none'}};
  }
  if(p?.kind==='invoke_capability') {
    const target=route.targets[0];
    if(!target)return route;
    return {...route,processor:{kind:'combined',forward:false,action:{destination_id:target.destination_id,tool_id:p.tool_id,revision:p.revision,allowed_senders:p.allowed_senders,arguments:route.command?.output.kind==='json'?route.command.output.template:{}},reply:route.response?.kind==='source'?{kind:'action_result'}:null},targets:[{...target,branch:'action'}],command:route.command?{...route.command,output:{kind:'original'}}:null,response:{kind:'none'}};
  }
  return {...route,processor:{kind:'combined',forward:true,action:null,reply:null},targets:route.targets.map(target=>({...target,branch:'forward'})),response:{kind:'none'}};
}
export function combinedValid(route:Route):boolean {
  const p=route.processor;if(p?.kind!=='combined')return false;
  const forward=route.targets.some(t=>t.branch==='forward');
  const a=p.action;
  return !!(p.forward||a||p.reply)&&(!p.forward||forward)&&(!a||!!a.destination_id&&!!a.tool_id&&a.revision>0&&a.allowed_senders.length<=100&&a.allowed_senders.every(s=>/^[a-zA-Z0-9_-]{1,128}$/.test(s))&&new Set(a.allowed_senders).size===a.allowed_senders.length&&!!route.command)&&(!p.reply||!!route.command&&(p.reply.kind==='fixed'?!!p.reply.text.trim():!!a));
}

export function makeCommandSet(route:Route):Route {
  const p=route.processor;if(p?.kind!=='combined'||!p.action)return route;
  return {...route,processor:{kind:'command_set',forward:p.forward,commands:[{id:crypto.randomUUID(),mapping:route.command??{text_pointer:'/content',trigger:'',mode:'exact',output:{kind:'original'}},action:p.action,reply:p.reply},{id:crypto.randomUUID(),mapping:{text_pointer:'/content',trigger:'',mode:'exact',output:{kind:'original'}},action:{destination_id:'',tool_id:'',revision:1,allowed_senders:[],arguments:{}},reply:null}]},command:null,targets:route.targets.filter(t=>t.branch==='forward')};
}
export function commandSetValid(route:Route):boolean {
  const p=route.processor;if(p?.kind!=='command_set')return false;
  return p.commands.length>0&&p.commands.length<=16&&route.targets.filter(t=>t.branch==='forward').length+p.commands.length+p.commands.filter(c=>c.reply?.kind==='fixed').length<=32&&(!p.forward||route.targets.some(t=>t.branch==='forward'))&&p.commands.every(c=>!!c.mapping.trigger.trim()&&c.mapping.trigger===c.mapping.trigger.trim()&&(c.mapping.text_pointer===''||c.mapping.text_pointer.startsWith('/'))&&!!c.action.destination_id&&!!c.action.tool_id&&c.action.revision>0&&c.action.allowed_senders.length<=100&&c.action.allowed_senders.every(s=>/^[a-zA-Z0-9_-]{1,128}$/.test(s))&&new Set(c.action.allowed_senders).size===c.action.allowed_senders.length&&(!c.reply||c.reply.kind==='action_result'||!!c.reply.text.trim()));
}
