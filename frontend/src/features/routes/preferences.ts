export type RoutePreferences = {
  mode: 'list' | 'graph';
  listSource: string | null;
  graphSources: string[];
  listPage: number;
  graphPage: number;
};
export const defaultRoutePreferences = (): RoutePreferences => ({mode:'list',listSource:null,graphSources:[],listPage:1,graphPage:1});
const uuid = (v:unknown):v is string => typeof v==='string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v);
export function parseRoutePreferences(raw:string|null):RoutePreferences {
  try {
    const v=JSON.parse(raw??'null');
    if(!v || typeof v!=='object')return defaultRoutePreferences();
    const page=(n:unknown)=>typeof n==='number'&&Number.isSafeInteger(n)&&n>0&&n<=1_000_000?n:1;
    return {mode:v.mode==='graph'?'graph':'list',listSource:uuid(v.listSource)?v.listSource:null,
      graphSources:Array.isArray(v.graphSources)?[...new Set<string>(v.graphSources.filter(uuid))].slice(0,48):[],listPage:page(v.listPage),graphPage:page(v.graphPage)};
  }catch{return defaultRoutePreferences();}
}
export function normalizeRoutePreferences(p:RoutePreferences,ids:string[]):RoutePreferences {
  const listSource=p.listSource&&ids.includes(p.listSource)?p.listSource:null;
  const graphSources=p.graphSources.filter(id=>ids.includes(id));
  return {...p,listSource,graphSources,listPage:listSource===p.listSource?p.listPage:1,graphPage:graphSources.length===p.graphSources.length?p.graphPage:1};
}
export function selectRouteSource(p:RoutePreferences,id:string|null):RoutePreferences {
  if(p.mode==='list')return {...p,listSource:id,listPage:1};
  return {...p,graphSources:id===null?[]:p.graphSources.includes(id)?p.graphSources.filter(s=>s!==id):[...p.graphSources,id],graphPage:1};
}
export function routeViewQuery(p:RoutePreferences) {
  return {sourceIds:p.mode==='list'?(p.listSource?[p.listSource]:[]):p.graphSources,page:p.mode==='list'?p.listPage:p.graphPage,pageSize:p.mode==='list'?10:64};
}
