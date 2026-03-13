import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { useGraphStore } from '../store/graphStore';

const TOGGLE_KEYS = [
  { key: 'enable_write_file', label: 'Write File' },
  { key: 'enable_git', label: 'Git' },
  { key: 'enable_web_search', label: 'Web Search' },
  { key: 'enable_browser', label: 'Browser' },
  { key: 'enable_http_request', label: 'HTTP Req' },
  { key: 'enable_web_fetch', label: 'Web Fetch' },
  { key: 'enable_cron', label: 'Cron' },
  { key: 'enable_mcp', label: 'MCP' },
  { key: 'enable_agents_ipc', label: 'Agents IPC' },
  { key: 'enable_wasm_plugins', label: 'WASM' },
];

function SecurityPolicyNode({ id, data, selected }: NodeProps) {
  const updateNodeData = useGraphStore((s) => s.updateNodeData);

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : '#ef4444', minWidth: 220 }}>
      <div className="az-node-label" style={{ color: '#f87171', marginBottom: 6 }}>
        Security Policy
      </div>
      <div className="toggle-grid">
        {TOGGLE_KEYS.map(({ key, label }) => (
          <label key={key} className="toggle-row">
            <input
              type="checkbox"
              checked={!!data[key]}
              onChange={(e) => updateNodeData(id, { [key]: e.target.checked })}
            />
            <span>{label}</span>
          </label>
        ))}
      </div>
      <Handle type="source" position={Position.Right} id="tools_out" className="az-handle az-handle-security" />
    </div>
  );
}

export default memo(SecurityPolicyNode);
