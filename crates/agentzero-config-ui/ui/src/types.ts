// Types mirroring the Rust backend schema

export interface NodeTypeDescriptor {
  type_id: string;
  display_name: string;
  color: string;
  category: string;
  ports: PortDescriptor[];
  properties: PropertyDescriptor[];
}

export interface PortDescriptor {
  id: string;
  label: string;
  direction: 'input' | 'output';
  accepts: string[];
  cardinality: 'one' | 'many';
}

export interface PropertyDescriptor {
  key: string;
  label: string;
  kind: 'bool' | 'string' | 'number' | 'text' | 'enum' | 'string_list' | 'key_value_map';
  default_value: unknown;
  description?: string;
  group?: string;
  enum_values?: string[];
}

export interface ToolSummary {
  name: string;
  description: string;
  category: string;
  always_available: boolean;
  gate_flag?: string;
  input_schema?: unknown;
}

export interface GraphModel {
  nodes: GraphNode[];
  edges: GraphEdge[];
  viewport: Viewport;
}

export interface GraphNode {
  id: string;
  type_id: string;
  position: Position;
  data: Record<string, unknown>;
  parent_id?: string;
  width?: number;
  height?: number;
}

export interface GraphEdge {
  id: string;
  source: string;
  source_port: string;
  target: string;
  target_port: string;
  edge_type: string;
}

export interface Viewport {
  x: number;
  y: number;
  zoom: number;
}

export interface Position {
  x: number;
  y: number;
}

export interface ValidationError {
  node_id?: string;
  field?: string;
  message: string;
  severity: 'error' | 'warning';
}

export interface ImportResponse {
  graph: GraphModel;
}

export interface ExportResponse {
  toml: string;
}

export interface ValidateResponse {
  errors: ValidationError[];
  valid: boolean;
}

// Node data types for each node type
export interface ToolNodeData {
  name: string;
  enabled: boolean;
  description: string;
  category?: string;
}

export interface SecurityPolicyNodeData {
  [key: string]: unknown;
}

export interface AgentNodeData {
  name: string;
  provider: string;
  model: string;
  system_prompt: string;
  max_depth: number;
  agentic: boolean;
  max_iterations: number;
  privacy_boundary: string;
  max_tokens?: number;
  max_cost_usd?: number;
  allowed_tools: string[];
}

export interface ModelRouteNodeData {
  hint: string;
  provider: string;
  model: string;
  max_tokens?: number;
}

export interface ClassificationRuleNodeData {
  hint: string;
  keywords: string[];
  patterns: string[];
  priority: number;
}

export interface DepthPolicyNodeData {
  max_depth: number;
  allowed_tools: string[];
  denied_tools: string[];
}

// Persistent agent types (from AgentStore, not TOML config)
export interface AgentRecord {
  agent_id: string;
  name: string;
  description: string;
  provider: string;
  model: string;
  system_prompt?: string;
  keywords: string[];
  allowed_tools: string[];
  status: 'active' | 'stopped';
  created_at: number;
  updated_at: number;
}

export interface CreateAgentRequest {
  name: string;
  description?: string;
  provider?: string;
  model?: string;
  system_prompt?: string;
  keywords?: string[];
  allowed_tools?: string[];
}

export interface UpdateAgentRequest {
  name?: string;
  description?: string;
  provider?: string;
  model?: string;
  system_prompt?: string;
  keywords?: string[];
  allowed_tools?: string[];
}
