import type {Connection} from './api.ts';
export type Observation={status:{state:string;code?:string|null};received:number;sent:number};
export type Observations=Record<string,Observation>;
const states=new Set(['connecting','subscribing','connected','reconnecting','stopped','error','listening','on_demand']);
export function parseObservations(raw:unknown):Observations|undefined {
 if(raw===undefined)return undefined;
 if(!raw||typeof raw!=='object'||Array.isArray(raw))throw Error('invalid_observations');
 const result:Observations={};
 for(const [id,value] of Object.entries(raw)) {
  const v=value as Observation;
  if(!v||!v.status||!states.has(v.status.state)||!Number.isSafeInteger(v.received)||v.received<0||!Number.isSafeInteger(v.sent)||v.sent<0||(v.status.code!=null&&typeof v.status.code!=='string'))throw Error('invalid_observations');
  result[id]=v;
 }
 return result;
}
export function connectionState(c:Connection,observation?:Observation):string {
 if(!c.enabled)return '已停止';
 if(!observation)return '状态未知';
 return ({connecting:'连接中',subscribing:'订阅中',connected:'Broker 已连接',reconnecting:'重连中',stopped:'未连接',error:'连接异常',listening:'接收就绪',on_demand:'按需发送'} as Record<string,string>)[observation.status.state]??'状态未知';
}
