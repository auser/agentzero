import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';

interface PluginData {
  name: string;
  enabled: boolean;
  source: string;
  description: string;
}

function PluginNode({ data, selected }: NodeProps) {
  const d = data as unknown as PluginData;

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : '#a855f7' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <div className="az-node-dot" style={{ background: d.enabled ? '#a855f7' : '#4a5a78' }} />
        <span className="az-node-label" style={{ color: '#c084fc', opacity: d.enabled ? 1 : 0.5 }}>
          {d.name || 'Unnamed Plugin'}
        </span>
      </div>
      {d.source && (
        <div className="az-node-sub" style={{ marginTop: 3 }}>
          {d.source.slice(0, 50)}
        </div>
      )}
      {d.description && (
        <div className="az-node-sub" style={{ marginTop: 2 }}>
          {d.description.slice(0, 50)}
        </div>
      )}
      <Handle type="source" position={Position.Right} id="agent_out" className="az-handle az-handle-tool" />
    </div>
  );
}

export default memo(PluginNode);
