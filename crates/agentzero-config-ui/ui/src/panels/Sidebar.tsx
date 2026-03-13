import { useState } from 'react';
import { useGraphStore } from '../store/graphStore';

const TOOL_CATEGORIES = [
  { id: 'file', label: 'File & Search', color: '#3b82f6' },
  { id: 'system', label: 'System', color: '#6b7280' },
  { id: 'memory', label: 'Memory', color: '#8b5cf6' },
  { id: 'web', label: 'Web & Network', color: '#10b981' },
  { id: 'orchestration', label: 'Orchestration', color: '#f59e0b' },
  { id: 'automation', label: 'Automation', color: '#06b6d4' },
  { id: 'vcs', label: 'Version Control', color: '#ef4444' },
  { id: 'sop', label: 'SOPs', color: '#f97316' },
  { id: 'hardware', label: 'Hardware', color: '#78716c' },
  { id: 'plugin', label: 'Plugins', color: '#a855f7' },
  { id: 'integration', label: 'Integrations', color: '#ec4899' },
];

const NODE_TYPES = [
  { id: 'agent', label: 'Agent', color: '#8b5cf6', desc: 'Core AI agent with tools & prompts' },
  { id: 'security_policy', label: 'Security Policy', color: '#ef4444', desc: 'Tool access controls' },
  { id: 'model_route', label: 'Model Route', color: '#f59e0b', desc: 'Route queries to models' },
  { id: 'classification_rule', label: 'Classification Rule', color: '#06b6d4', desc: 'Route matching rules' },
  { id: 'provider', label: 'Provider', color: '#ec4899', desc: 'LLM provider config' },
  { id: 'autonomy', label: 'Autonomy', color: '#f97316', desc: 'Risk & autonomy controls' },
  { id: 'depth_policy', label: 'Depth Policy', color: '#10b981', desc: 'Depth-level tool restrictions' },
  { id: 'plugin', label: 'Plugin', color: '#a855f7', desc: 'WASM plugin module' },
];

export default function Sidebar() {
  const [search, setSearch] = useState('');
  const [tab, setTab] = useState<'nodes' | 'tools'>('nodes');
  const tools = useGraphStore((s) => s.tools);
  const nodes = useGraphStore((s) => s.nodes);
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId);
  const addNode = useGraphStore((s) => s.addNode);
  const addToolToAgent = useGraphStore((s) => s.addToolToAgent);

  const selectedAgent = nodes.find(
    (n) => n.id === selectedNodeId && n.type === 'agent',
  );

  const filteredTools = tools.filter(
    (t) =>
      t.name.toLowerCase().includes(search.toLowerCase()) ||
      t.description.toLowerCase().includes(search.toLowerCase()) ||
      t.category.toLowerCase().includes(search.toLowerCase()),
  );

  const groupedTools = new Map<string, typeof filteredTools>();
  for (const t of filteredTools) {
    if (!groupedTools.has(t.category)) groupedTools.set(t.category, []);
    groupedTools.get(t.category)!.push(t);
  }

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div className="tabs-bar">
        <button
          className={`tab-btn ${tab === 'nodes' ? 'active' : ''}`}
          onClick={() => setTab('nodes')}
        >
          Nodes
        </button>
        <button
          className={`tab-btn ${tab === 'tools' ? 'active' : ''}`}
          onClick={() => setTab('tools')}
        >
          Tools ({tools.length})
        </button>
      </div>

      <div style={{ padding: '8px 12px 0' }}>
        <input
          type="text"
          placeholder={tab === 'nodes' ? 'Search nodes...' : 'Search tools...'}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="palette-search"
        />
      </div>

      <div style={{ flex: 1, overflowY: 'auto', padding: '0 12px 12px' }}>
        {tab === 'nodes' && (
          <>
            <div className="palette-group-label">Add Node</div>
            {NODE_TYPES.filter(
              (t) =>
                t.label.toLowerCase().includes(search.toLowerCase()) ||
                t.desc.toLowerCase().includes(search.toLowerCase()),
            ).map((t) => (
              <button
                key={t.id}
                className="palette-item"
                onClick={() => {
                  const x = 300 + Math.random() * 200;
                  const y = 150 + Math.random() * 200;
                  addNode(t.id, { x, y });
                }}
              >
                <div className="palette-item-dot" style={{ background: t.color }} />
                <div>
                  <div style={{ fontWeight: 500 }}>{t.label}</div>
                  <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>{t.desc}</div>
                </div>
              </button>
            ))}
          </>
        )}

        {tab === 'tools' && (
          <>
            {selectedAgent && (
              <div className="sidebar-agent-hint">
                Adding tools to: <strong>{(selectedAgent.data as Record<string, unknown>).name as string || 'Unnamed Agent'}</strong>
              </div>
            )}
            {!selectedAgent && nodes.some((n) => n.type === 'agent') && (
              <div className="sidebar-agent-hint" style={{ color: 'var(--warning)' }}>
                Select an agent node to attach tools to it
              </div>
            )}

            {[...groupedTools.entries()].map(([category, catTools]) => {
              const catMeta = TOOL_CATEGORIES.find((c) => c.id === category);
              return (
                <div key={category}>
                  <div className="palette-group-label" style={{ color: catMeta?.color }}>
                    {catMeta?.label ?? category}
                  </div>
                  {catTools.map((t) => (
                    <button
                      key={t.name}
                      className="palette-item"
                      onClick={() => {
                        if (selectedAgent) {
                          addToolToAgent(t.name, selectedAgent.id);
                        } else {
                          // Add as standalone tool node
                          const x = 100 + Math.random() * 300;
                          const y = 100 + Math.random() * 300;
                          addNode('tool', { x, y }, {
                            name: t.name,
                            enabled: true,
                            description: t.description,
                            category: t.category,
                          });
                        }
                      }}
                    >
                      <div className="palette-item-dot" style={{ background: catMeta?.color ?? '#6b7280' }} />
                      <div>
                        <div style={{ fontWeight: 500 }}>{t.name}</div>
                        <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>
                          {t.description.slice(0, 40)}
                          {!t.always_available && t.gate_flag && (
                            <span style={{ color: 'var(--warning)', marginLeft: 4 }}>gated</span>
                          )}
                        </div>
                      </div>
                    </button>
                  ))}
                </div>
              );
            })}
          </>
        )}
      </div>
    </div>
  );
}
