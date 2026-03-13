import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';

interface ProviderData {
  kind: string;
  base_url: string;
  model: string;
}

function ProviderNode({ data, selected }: NodeProps) {
  const d = data as unknown as ProviderData;

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : '#ec4899' }}>
      <div className="az-node-label" style={{ color: '#f472b6' }}>Provider</div>
      <div className="az-node-sub">{d.kind} &middot; {d.model}</div>
      <Handle type="source" position={Position.Right} id="agent_out" className="az-handle az-handle-route" />
    </div>
  );
}

export default memo(ProviderNode);
