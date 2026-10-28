export type Effect = 'ReadOnly' | 'SideEffecting' | 'Unknown';
export interface Tool {
  public_name: string;
  connection_id: string;
  definition: { id: string; description: string; effect: Effect; input_schema: Record<string, unknown>; output_schema: Record<string, unknown> | null };
  url: string;
  bearer_env: string | null;
  allow_loopback_http?: boolean;
  exposed: boolean;
}
export interface Draft { generation: number; base_revision: number; exposed_tools: string[]; actor_id: string }
export type CapabilityKind = 'tools' | 'resources' | 'prompts';
export interface ContentScope { resources: string[]; prompts: string[] }
export interface ContentInventory {
  revision: number;
  resources: {id:string;description:string;mime_type:string;kind:'resource'|'template';parameter:string|null}[];
  prompts: {id:string;description:string;arguments:{name:string;description:string;required:boolean;choices:string[]}[]}[];
}
export interface ControlView {
  content?: ContentInventory;
  workspace?: { id: string; name: string; state: string };
  revision: number; profile_id: string; admin_id: string; tools: Tool[]; draft: Draft | null;
  connection: { resource: string | null; authentication: 'oauth' | 'bearer'; issuer: string | null };
}
export interface DraftInput { expected_generation: number; base_revision: number; exposed_tools: string[] }
export interface PublishInput { operation_id: string; expected_revision: number; draft_generation: number }
export interface Receipt { operation_id: string; revision: number; replayed: boolean }
export interface OperationResult { status: 'committed' | 'unconfirmed'; receipt: Receipt | null }
export interface HistoryEntry { revision: number; created_ms: number; actor_id: string; exposed_tools: string[] }
export interface History { items: HistoryEntry[]; next_before_revision: number | null }

export interface AddConnectionInput { operation_id: string; expected_revision: number; tool: Tool & { allow_loopback_http: boolean } }
export interface Verification { invocation_id: string; status: 'succeeded' | 'failed' | 'unknown' | 'unconfirmed' }
export interface VerifyInput { mcp_token: string; invocation_id: string; action: { kind: 'inspect' } | { kind: 'execute'; revision: number; public_name: string; arguments: Record<string, unknown>; confirmed: boolean } }

export type BuiltinTool = 'connections_list'|'connection_status'|'connection_statistics'|'recent_messages'|'qq_bot_info'|'qq_send_text';
export type ConversationGrant = {connection_id:string;account:string;kind:'group'|'direct'|'stream';conversation_id:string};
export type BuiltinScope = {tools:BuiltinTool[];connections:string[];conversations:ConversationGrant[];qq_send_conversations:ConversationGrant[]};
export interface McpServer {
  builtin_scope?: BuiltinScope;
  content_scope?: ContentScope; content_revision?: number;
  id: string; name: string; enabled: boolean; deleted: boolean; revision: number; tool_revision: number;
  path: string; address: string | null; exposed_tools: string[]; is_default: boolean; authentication: 'oauth' | 'bearer' | 'basic' | 'jwt' | 'none';
  public_url?: string; auth?: ServerAuth; description?: string; transport?: 'streamable_http';
}
export interface ServerChange {
  builtin_scope?: BuiltinScope;
  content_scope?: ContentScope; expected_content_revision?: number;
  operation_id: string; expected_revision: number; expected_tool_revision: number;
  name: string; enabled: boolean; exposed_tools: string[];
  public_url?: string; auth?: ServerAuth; description?: string; transport?: 'streamable_http';
}
export interface ServerDeletion { operation_id: string; expected_revision: number }

export type ServerAuth = {mode:'inherited'|'none'} | {mode:'basic';username:string;password_env:string}
 | {mode:'jwt';issuer:string;audience:string;subject:string;public_key_pem:string}
 | {mode:'oauth';issuer:string;resource:string;introspection_endpoint:string;introspection_client_id:string;introspection_secret_env:string;required_scope:string;subject:string;oauth_client_id:string;allow_loopback_http:boolean};

export interface ChangeConnectionInput {
  operation_id: string;
  expected_revision: number;
  change: {kind:'delete'} | {kind:'update';public_name:string;description:string;url:string;effect:Effect;input_schema:Record<string,unknown>;allow_loopback_http:boolean};
}
