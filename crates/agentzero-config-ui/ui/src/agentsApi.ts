import type { AgentRecord, CreateAgentRequest, UpdateAgentRequest } from './types';

const BASE = '';

export async function listAgents(): Promise<AgentRecord[]> {
  const res = await fetch(`${BASE}/api/agents`);
  if (!res.ok) return []; // agents endpoint may not be available
  return res.json();
}

export async function createAgent(req: CreateAgentRequest): Promise<AgentRecord> {
  const res = await fetch(`${BASE}/api/agents`, {
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
  const res = await fetch(`${BASE}/api/agents/${encodeURIComponent(id)}`);
  if (!res.ok) throw new Error(`Agent not found`);
  return res.json();
}

export async function updateAgent(id: string, req: UpdateAgentRequest): Promise<AgentRecord> {
  const res = await fetch(`${BASE}/api/agents/${encodeURIComponent(id)}`, {
    method: 'PUT',
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
  const res = await fetch(`${BASE}/api/agents/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  if (!res.ok && res.status !== 204) {
    throw new Error(`Delete agent failed`);
  }
}

export async function setAgentStatus(id: string, active: boolean): Promise<void> {
  const res = await fetch(`${BASE}/api/agents/${encodeURIComponent(id)}/status`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ active }),
  });
  if (!res.ok) {
    throw new Error(`Set status failed`);
  }
}
