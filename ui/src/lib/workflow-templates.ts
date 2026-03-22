/**
 * Pre-built workflow templates for the template gallery.
 * Each template defines a complete workflow graph with nodes, edges, and metadata.
 */

interface TemplateNode {
  id: string
  type: string
  position: { x: number; y: number }
  data: {
    name: string
    nodeType: string
    status: string
    metadata: Record<string, unknown>
  }
}

interface TemplateEdge {
  id: string
  source: string
  target: string
  sourceHandle: string
  targetHandle: string
  data?: { port_type?: string }
}

export interface WorkflowTemplate {
  id: string
  name: string
  description: string
  category: string
  nodeCount: number
  nodes: TemplateNode[]
  edges: TemplateEdge[]
}

// ── Research Pipeline ────────────────────────────────────────────────────────

const RESEARCH_PIPELINE: WorkflowTemplate = {
  id: 'research-pipeline',
  name: 'Research Pipeline',
  description: 'Multi-step research: gather sources, analyze findings, write a summary report.',
  category: 'research',
  nodeCount: 4,
  nodes: [
    {
      id: 'trigger-1', type: 'channel',
      position: { x: 0, y: 120 },
      data: { name: 'chat', nodeType: 'channel', status: 'queued', metadata: { channel_type: 'chat' } },
    },
    {
      id: 'researcher-1', type: 'agent',
      position: { x: 300, y: 0 },
      data: {
        name: 'Researcher', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a research assistant. Search for reliable sources on the given topic and compile key findings with citations.' },
      },
    },
    {
      id: 'analyst-1', type: 'agent',
      position: { x: 300, y: 240 },
      data: {
        name: 'Analyst', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a data analyst. Analyze the research findings, identify patterns, and extract actionable insights.' },
      },
    },
    {
      id: 'writer-1', type: 'agent',
      position: { x: 600, y: 120 },
      data: {
        name: 'Report Writer', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a technical writer. Synthesize the research and analysis into a clear, well-structured report with an executive summary.' },
      },
    },
  ],
  edges: [
    { id: 'e1', source: 'trigger-1', target: 'researcher-1', sourceHandle: 'message', targetHandle: 'input', data: { port_type: 'text' } },
    { id: 'e2', source: 'trigger-1', target: 'analyst-1', sourceHandle: 'message', targetHandle: 'input', data: { port_type: 'text' } },
    { id: 'e3', source: 'researcher-1', target: 'writer-1', sourceHandle: 'response', targetHandle: 'input', data: { port_type: 'text' } },
    { id: 'e4', source: 'analyst-1', target: 'writer-1', sourceHandle: 'response', targetHandle: 'context', data: { port_type: 'json' } },
  ],
}

// ── Content Generator ────────────────────────────────────────────────────────

const CONTENT_GENERATOR: WorkflowTemplate = {
  id: 'content-generator',
  name: 'Content Generator',
  description: 'Draft content, review for quality, then publish to a channel.',
  category: 'content',
  nodeCount: 4,
  nodes: [
    {
      id: 'schedule-1', type: 'schedule',
      position: { x: 0, y: 80 },
      data: { name: 'daily trigger', nodeType: 'schedule', status: 'queued', metadata: { cron: '0 9 * * *' } },
    },
    {
      id: 'drafter-1', type: 'agent',
      position: { x: 250, y: 80 },
      data: {
        name: 'Content Drafter', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a creative content writer. Generate engaging, well-structured content on the given topic. Include a compelling headline.' },
      },
    },
    {
      id: 'reviewer-1', type: 'agent',
      position: { x: 520, y: 80 },
      data: {
        name: 'Editor', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a strict editor. Review the content for clarity, grammar, factual accuracy, and engagement. Rewrite if needed.' },
      },
    },
    {
      id: 'publish-1', type: 'channel',
      position: { x: 790, y: 80 },
      data: { name: 'slack', nodeType: 'channel', status: 'queued', metadata: { channel_type: 'slack' } },
    },
  ],
  edges: [
    { id: 'e1', source: 'schedule-1', target: 'drafter-1', sourceHandle: 'trigger', targetHandle: 'input', data: { port_type: 'event' } },
    { id: 'e2', source: 'drafter-1', target: 'reviewer-1', sourceHandle: 'response', targetHandle: 'input', data: { port_type: 'text' } },
    { id: 'e3', source: 'reviewer-1', target: 'publish-1', sourceHandle: 'response', targetHandle: 'send', data: { port_type: 'text' } },
  ],
}

// ── Code Review ──────────────────────────────────────────────────────────────

const CODE_REVIEW: WorkflowTemplate = {
  id: 'code-review',
  name: 'Code Review Pipeline',
  description: 'Automated code review: analyze code, check security, generate feedback.',
  category: 'engineering',
  nodeCount: 5,
  nodes: [
    {
      id: 'webhook-1', type: 'channel',
      position: { x: 0, y: 120 },
      data: { name: 'webhook', nodeType: 'channel', status: 'queued', metadata: { channel_type: 'webhook' } },
    },
    {
      id: 'code-reader', type: 'tool',
      position: { x: 260, y: 120 },
      data: { name: 'read_file', nodeType: 'tool', status: 'queued', metadata: { tool_name: 'read_file' } },
    },
    {
      id: 'reviewer-1', type: 'agent',
      position: { x: 520, y: 0 },
      data: {
        name: 'Code Reviewer', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a senior code reviewer. Analyze the code for correctness, readability, performance, and adherence to best practices. Be constructive.' },
      },
    },
    {
      id: 'security-1', type: 'agent',
      position: { x: 520, y: 240 },
      data: {
        name: 'Security Auditor', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a security auditor. Check the code for OWASP top 10 vulnerabilities, injection risks, auth issues, and data exposure. Flag severity levels.' },
      },
    },
    {
      id: 'summarizer-1', type: 'agent',
      position: { x: 800, y: 120 },
      data: {
        name: 'Feedback Compiler', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'Combine the code review and security audit into a single structured feedback document with severity-ranked findings and suggested fixes.' },
      },
    },
  ],
  edges: [
    { id: 'e1', source: 'webhook-1', target: 'code-reader', sourceHandle: 'message', targetHandle: 'input', data: { port_type: 'text' } },
    { id: 'e2', source: 'code-reader', target: 'reviewer-1', sourceHandle: 'result', targetHandle: 'input', data: { port_type: 'json' } },
    { id: 'e3', source: 'code-reader', target: 'security-1', sourceHandle: 'result', targetHandle: 'input', data: { port_type: 'json' } },
    { id: 'e4', source: 'reviewer-1', target: 'summarizer-1', sourceHandle: 'response', targetHandle: 'input', data: { port_type: 'text' } },
    { id: 'e5', source: 'security-1', target: 'summarizer-1', sourceHandle: 'response', targetHandle: 'context', data: { port_type: 'json' } },
  ],
}

// ── Customer Support ─────────────────────────────────────────────────────────

const CUSTOMER_SUPPORT: WorkflowTemplate = {
  id: 'customer-support',
  name: 'Customer Support Triage',
  description: 'Classify incoming messages, route to specialist agents, require approval for escalations.',
  category: 'support',
  nodeCount: 5,
  nodes: [
    {
      id: 'telegram-1', type: 'channel',
      position: { x: 0, y: 120 },
      data: { name: 'telegram', nodeType: 'channel', status: 'queued', metadata: { channel_type: 'telegram' } },
    },
    {
      id: 'classifier-1', type: 'agent',
      position: { x: 280, y: 120 },
      data: {
        name: 'Triage Agent', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a customer support triage agent. Classify the message as: billing, technical, feature_request, or escalation. Respond with ONLY the category.' },
      },
    },
    {
      id: 'gate-1', type: 'gate',
      position: { x: 540, y: 120 },
      data: { name: 'Escalation Gate', nodeType: 'gate', status: 'queued', metadata: {} },
    },
    {
      id: 'responder-1', type: 'agent',
      position: { x: 800, y: 40 },
      data: {
        name: 'Support Agent', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a helpful customer support agent. Provide a clear, empathetic response to the customer inquiry. Include next steps.' },
      },
    },
    {
      id: 'escalation-1', type: 'agent',
      position: { x: 800, y: 220 },
      data: {
        name: 'Escalation Handler', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'This is an escalated support case. Summarize the issue, acknowledge the severity, and outline the resolution plan. Flag for human follow-up.' },
      },
    },
  ],
  edges: [
    { id: 'e1', source: 'telegram-1', target: 'classifier-1', sourceHandle: 'message', targetHandle: 'input', data: { port_type: 'text' } },
    { id: 'e2', source: 'classifier-1', target: 'gate-1', sourceHandle: 'response', targetHandle: 'request', data: { port_type: 'json' } },
    { id: 'e3', source: 'gate-1', target: 'responder-1', sourceHandle: 'approved', targetHandle: 'input', data: { port_type: 'event' } },
    { id: 'e4', source: 'gate-1', target: 'escalation-1', sourceHandle: 'denied', targetHandle: 'input', data: { port_type: 'event' } },
  ],
}

// ── Data Analysis ────────────────────────────────────────────────────────────

const DATA_ANALYSIS: WorkflowTemplate = {
  id: 'data-analysis',
  name: 'Data Analysis Pipeline',
  description: 'Extract data, run analysis, generate visualizations and insights.',
  category: 'analytics',
  nodeCount: 3,
  nodes: [
    {
      id: 'chat-1', type: 'channel',
      position: { x: 0, y: 80 },
      data: { name: 'chat', nodeType: 'channel', status: 'queued', metadata: { channel_type: 'chat' } },
    },
    {
      id: 'analyst-1', type: 'agent',
      position: { x: 300, y: 80 },
      data: {
        name: 'Data Analyst', nodeType: 'agent', status: 'queued',
        metadata: { system_prompt: 'You are a data analyst with access to shell and file tools. Analyze the data described by the user. Write and execute code to produce insights. Use charts when helpful.' },
      },
    },
    {
      id: 'canvas-1', type: 'channel',
      position: { x: 600, y: 80 },
      data: { name: 'canvas', nodeType: 'channel', status: 'queued', metadata: { channel_type: 'chat' } },
    },
  ],
  edges: [
    { id: 'e1', source: 'chat-1', target: 'analyst-1', sourceHandle: 'message', targetHandle: 'input', data: { port_type: 'text' } },
    { id: 'e2', source: 'analyst-1', target: 'canvas-1', sourceHandle: 'response', targetHandle: 'send', data: { port_type: 'text' } },
  ],
}

// ── Registry ─────────────────────────────────────────────────────────────────

export const ALL_TEMPLATES: WorkflowTemplate[] = [
  RESEARCH_PIPELINE,
  CONTENT_GENERATOR,
  CODE_REVIEW,
  CUSTOMER_SUPPORT,
  DATA_ANALYSIS,
]

export function getTemplate(id: string): WorkflowTemplate | undefined {
  return ALL_TEMPLATES.find((t) => t.id === id)
}
