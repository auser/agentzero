import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';

interface ToolData {
  name: string;
  enabled: boolean;
  description: string;
  category?: string;
}

const CATEGORY_COLORS: Record<string, string> = {
  file: '#3b82f6',
  web: '#10b981',
  system: '#6b7280',
  memory: '#8b5cf6',
  orchestration: '#f59e0b',
  automation: '#06b6d4',
  vcs: '#ef4444',
  sop: '#f97316',
  hardware: '#78716c',
  plugin: '#a855f7',
  integration: '#ec4899',
};

function ToolNode({ data, selected }: NodeProps) {
  const d = data as unknown as ToolData;
  const color = CATEGORY_COLORS[d.category ?? 'system'] ?? '#6b7280';

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : color }}>
      <Handle type="target" position={Position.Left} id="policy_in" className="az-handle az-handle-security" />
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <div className="az-node-dot" style={{ background: d.enabled ? '#22c55e' : '#4a5a78' }} />
        <span className="az-node-label" style={{ color, opacity: d.enabled ? 1 : 0.5 }}>
          {d.name}
        </span>
      </div>
      {d.description && (
        <div className="az-node-sub" style={{ marginTop: 3 }}>
          {d.description.slice(0, 60)}
        </div>
      )}
      <Handle type="source" position={Position.Right} id="agent_out" className="az-handle az-handle-tool" />
    </div>
  );
}

export default memo(ToolNode);
