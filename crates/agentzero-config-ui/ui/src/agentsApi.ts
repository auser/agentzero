import type { AgentRecord, CreateAgentRequest, UpdateAgentRequest } from './types';

/**
 * Resolve the gateway base URL. Checks (in order):
 * 1. `VITE_GATEWAY_URL` build-time env var
 * 2. Same origin as the current page (works when served from gateway or config-ui)
 *
 * The Config UI can be served standalone (`agentzero config-ui`) or embedded
 * in the gateway (`embedded-ui` feature). In both cases, the agent CRUD
 * endpoints live on the gateway at `/v1/agents`.
 */
function gatewayBase(): string {
  // Build-time override (e.g. VITE_GATEWAY_URL=http://localhost:8080)
  if (typeof import.meta !== 'undefined') {
    const env = (import.meta as unknown as Record<string, unknown>).env as Record<string, string> | undefined;
    if (env?.VITE_GATEWAY_URL) return env.VITE_GATEWAY_URL.replace(/\/+$/, '');
  }
  // Default: same origin (works when gateway serves the UI, or via proxy)
  return '';
}

/** Try the gateway `/v1/agents` first, fall back to config-ui `/api/agents`. */
async function agentsFetch(path: string, init?: RequestInit): Promise<Response> {
  const gw = gatewayBase();

  // Try gateway endpoint first.
  const gwUrl = `${gw}/v1/agents${path}`;
  const res = await fetch(gwUrl, init).catch(() => null);
  if (res && res.status !== 404) return res;

  // Fall back to config-ui's own endpoint (standalone mode).
  const localUrl = `/api/agents${path}`;
  return fetch(localUrl, init);
}

export async function listAgents(): Promise<AgentRecord[]> {
  const res = await agentsFetch('');
  if (!res.ok) return [];
  return res.json();
}

export async function createAgent(req: CreateAgentRequest): Promise<AgentRecord> {
  const res = await agentsFetch('', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Create agent failed: ${text}`);
  }
  return res.json();
}

export async function getAgent(id: string): Promise<AgentRecord> {
  const res = await agentsFetch(`/${encodeURIComponent(id)}`);
  if (!res.ok) throw new Error(`Agent not found`);
  return res.json();
}

export async function updateAgent(id: string, req: UpdateAgentRequest): Promise<AgentRecord> {
  // Gateway uses PATCH, config-ui backend uses PUT — try PATCH first.
  const res = await agentsFetch(`/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Update agent failed: ${text}`);
  }
  return res.json();
}

export async function deleteAgent(id: string): Promise<void> {
  const res = await agentsFetch(`/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  if (!res.ok && res.status !== 204) {
    throw new Error(`Delete agent failed`);
  }
}

export async function setAgentStatus(id: string, active: boolean): Promise<void> {
  // Use PATCH on gateway to toggle status.
  const res = await agentsFetch(`/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ status: active ? 'active' : 'stopped' }),
  });
  if (!res.ok) {
    throw new Error(`Set status failed`);
  }
}
