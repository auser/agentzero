import type { NodeTypeDescriptor, ToolSummary, GraphModel, ImportResponse, ExportResponse, ValidateResponse } from './types';

const BASE = '';

export async function fetchSchema(): Promise<NodeTypeDescriptor[]> {
  const res = await fetch(`${BASE}/api/schema`);
  if (!res.ok) throw new Error(`Failed to fetch schema: ${res.statusText}`);
  return res.json();
}

export async function fetchTools(): Promise<ToolSummary[]> {
  const res = await fetch(`${BASE}/api/tools`);
  if (!res.ok) throw new Error(`Failed to fetch tools: ${res.statusText}`);
  return res.json();
}

export async function fetchDefaults(): Promise<GraphModel> {
  const res = await fetch(`${BASE}/api/defaults`);
  if (!res.ok) throw new Error(`Failed to fetch defaults: ${res.statusText}`);
  return res.json();
}

export async function importToml(toml: string): Promise<ImportResponse> {
  const res = await fetch(`${BASE}/api/import`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ toml }),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Import failed: ${text}`);
  }
  return res.json();
}

export async function exportToml(graph: GraphModel): Promise<ExportResponse> {
  const res = await fetch(`${BASE}/api/export`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(graph),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Export failed: ${text}`);
  }
  return res.json();
}

export async function validateGraph(graph: GraphModel): Promise<ValidateResponse> {
  const res = await fetch(`${BASE}/api/validate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(graph),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Validation failed: ${text}`);
  }
  return res.json();
}
