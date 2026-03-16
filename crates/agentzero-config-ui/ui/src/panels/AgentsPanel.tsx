import { useCallback, useEffect, useState } from 'react';
import type { AgentRecord } from '../types';
import { listAgents, createAgent, deleteAgent, setAgentStatus } from '../agentsApi';

export default function AgentsPanel() {
  const [agents, setAgents] = useState<AgentRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const data = await listAgents();
      setAgents(data);
      setError(null);
    } catch {
      setError('Could not load agents');
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const handleDelete = useCallback(async (id: string, name: string) => {
    if (!confirm(`Delete agent "${name}"?`)) return;
    try {
      await deleteAgent(id);
      refresh();
    } catch {
      setError('Delete failed');
    }
  }, [refresh]);

  const handleToggleStatus = useCallback(async (id: string, currentlyActive: boolean) => {
    try {
      await setAgentStatus(id, !currentlyActive);
      refresh();
    } catch {
      setError('Status change failed');
    }
  }, [refresh]);

  return (
    <div style={{ padding: '12px 16px', fontSize: 13 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <span style={{ fontWeight: 600, color: 'var(--text)' }}>
          Persistent Agents ({agents.length})
        </span>
        <button className="toolbar-btn" onClick={() => setShowCreate(!showCreate)}>
          {showCreate ? 'Cancel' : '+ Create'}
        </button>
      </div>

      {error && <div style={{ color: 'var(--danger)', marginBottom: 8 }}>{error}</div>}

      {showCreate && <CreateForm onCreated={() => { setShowCreate(false); refresh(); }} />}

      {agents.length === 0 && !showCreate && (
        <div style={{ color: 'var(--text-muted)' }}>No persistent agents yet.</div>
      )}

      <table style={{ width: '100%', borderCollapse: 'collapse' }}>
        <tbody>
          {agents.map((a) => (
            <tr key={a.agent_id} style={{ borderBottom: '1px solid var(--border)' }}>
              <td style={{ padding: '6px 8px', fontWeight: 500 }}>{a.name}</td>
              <td style={{ padding: '6px 8px', color: 'var(--text-muted)' }}>{a.model || '(default)'}</td>
              <td style={{ padding: '6px 8px' }}>
                <span
                  style={{
                    padding: '2px 8px',
                    borderRadius: 4,
                    fontSize: 11,
                    background: a.status === 'active' ? '#10b98133' : '#ef444433',
                    color: a.status === 'active' ? '#10b981' : '#ef4444',
                  }}
                >
                  {a.status}
                </span>
              </td>
              <td style={{ padding: '6px 8px', color: 'var(--text-muted)', fontSize: 11 }}>
                {a.keywords.length > 0 ? a.keywords.join(', ') : ''}
              </td>
              <td style={{ padding: '6px 4px', textAlign: 'right', whiteSpace: 'nowrap' }}>
                <button
                  className="toolbar-btn"
                  style={{ fontSize: 11, padding: '2px 6px', marginRight: 4 }}
                  onClick={() => handleToggleStatus(a.agent_id, a.status === 'active')}
                >
                  {a.status === 'active' ? 'Stop' : 'Start'}
                </button>
                <button
                  className="toolbar-btn"
                  style={{ fontSize: 11, padding: '2px 6px', color: 'var(--danger)' }}
                  onClick={() => handleDelete(a.agent_id, a.name)}
                >
                  Delete
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function CreateForm({ onCreated }: { onCreated: () => void }) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [model, setModel] = useState('');
  const [provider, setProvider] = useState('');
  const [keywords, setKeywords] = useState('');
  const [systemPrompt, setSystemPrompt] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) { setError('Name is required'); return; }
    setSubmitting(true);
    setError(null);
    try {
      await createAgent({
        name: name.trim(),
        description: description.trim() || undefined,
        model: model.trim() || undefined,
        provider: provider.trim() || undefined,
        system_prompt: systemPrompt.trim() || undefined,
        keywords: keywords.trim() ? keywords.split(',').map(k => k.trim()) : undefined,
      });
      onCreated();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  const inputStyle: React.CSSProperties = {
    width: '100%',
    padding: '4px 8px',
    background: 'var(--bg-input, #1e293b)',
    border: '1px solid var(--border, #334155)',
    borderRadius: 4,
    color: 'var(--text)',
    fontSize: 12,
  };

  return (
    <form onSubmit={handleSubmit} style={{ marginBottom: 16, display: 'flex', flexDirection: 'column', gap: 6 }}>
      {error && <div style={{ color: 'var(--danger)', fontSize: 12 }}>{error}</div>}
      <input placeholder="Name *" value={name} onChange={e => setName(e.target.value)} style={inputStyle} />
      <input placeholder="Description" value={description} onChange={e => setDescription(e.target.value)} style={inputStyle} />
      <div style={{ display: 'flex', gap: 6 }}>
        <input placeholder="Provider" value={provider} onChange={e => setProvider(e.target.value)} style={inputStyle} />
        <input placeholder="Model" value={model} onChange={e => setModel(e.target.value)} style={inputStyle} />
      </div>
      <input placeholder="Keywords (comma-separated)" value={keywords} onChange={e => setKeywords(e.target.value)} style={inputStyle} />
      <textarea placeholder="System prompt" value={systemPrompt} onChange={e => setSystemPrompt(e.target.value)} rows={2} style={{ ...inputStyle, resize: 'vertical' }} />
      <button className="toolbar-btn" type="submit" disabled={submitting} style={{ alignSelf: 'flex-start' }}>
        {submitting ? 'Creating...' : 'Create Agent'}
      </button>
    </form>
  );
}
