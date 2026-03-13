import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';

interface AgentData {
  name: string;
  provider: string;
  model: string;
  agentic: boolean;
  privacy_boundary: string;
}

function AgentNode({ data, selected }: NodeProps) {
  const d = data as unknown as AgentData;

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : '#8b5cf6', minWidth: 180 }}>
      <Handle type="target" position={Position.Left} id="tools_in" style={{ background: '#3b82f6', top: '30%' }} />
      <Handle type="target" position={Position.Top} id="parent_in" style={{ background: '#8b5cf6' }} />
      <Handle type="target" position={Position.Left} id="route_in" style={{ background: '#f59e0b', top: '70%' }} />

      <div className="az-node-label" style={{ color: '#a78bfa', fontSize: 13 }}>
        {d.name || 'Unnamed Agent'}
      </div>
      <div className="az-node-sub">{d.model || 'no model'}</div>
      <div style={{ display: 'flex', gap: 4, marginTop: 5 }}>
        {d.agentic && (
          <span className="az-node-badge" style={{ background: 'rgba(139,92,246,0.2)', color: '#c4b5fd' }}>
            agentic
          </span>
        )}
        {d.privacy_boundary && d.privacy_boundary !== 'inherit' && (
          <span className="az-node-badge" style={{ background: 'rgba(239,68,68,0.2)', color: '#fca5a5' }}>
            {d.privacy_boundary}
          </span>
        )}
      </div>

      <Handle type="source" position={Position.Bottom} id="delegate_out" style={{ background: '#8b5cf6' }} />
    </div>
  );
}

export default memo(AgentNode);
