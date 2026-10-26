import {Children,cloneElement,createContext,isValidElement,useContext,useId,type ReactElement,type ReactNode} from 'react';
import {t} from '../../shared/i18n';
import type {FieldErrors} from './validation';
export const ConnectionFormContext=createContext<{errors:FieldErrors;touch:(name:string)=>void;clear:(name:string)=>void}>({errors:{},touch:()=>{},clear:()=>{}});
export function Field({label,name=label,children}:{label:string;name?:string;children:ReactNode}){
 const id=useId(),ctx=useContext(ConnectionFormContext),error=ctx.errors[name];
 const child=Children.only(children);
 const control=isValidElement(child)?cloneElement(child as ReactElement<Record<string,unknown>>,{
  id,'data-field':name,'aria-invalid':error?true:undefined,'aria-describedby':error?`${id}-error`:undefined,
  onBlur:(e:unknown)=>{const fn=(child.props as {onBlur?:(e:unknown)=>void}).onBlur;fn?.(e);ctx.touch(name);},
  onChange:(e:unknown)=>{const fn=(child.props as {onChange?:(e:unknown)=>void}).onChange;fn?.(e);ctx.clear(name);},
 }):child;
 return <div className={`ui-field${error?' has-error':''}`}><label htmlFor={id}>{t(label)}</label><div className="connection-field-control">{control}{error&&<small id={`${id}-error`} className="field-error" role="alert">{t(error)}</small>}</div></div>;
}
export function FormIssue({name}:{name:string}){const {errors}=useContext(ConnectionFormContext);return errors[name]?<p className="field-error" role="alert" data-field-group={name} tabIndex={-1}>{t(errors[name])}</p>:null;}
