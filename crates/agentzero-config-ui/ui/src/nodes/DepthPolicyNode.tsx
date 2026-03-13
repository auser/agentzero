import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';

interface DepthData {
  max_depth: number;
  allowed_tools: string[];
  denied_tools: string[];
}

function DepthPolicyNode({ data, selected }: NodeProps) {
  const d = data as unknown as DepthData;

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : '#10b981' }}>
      <div className="az-node-label" style={{ color: '#34d399' }}>Depth Policy</div>
      <div className="az-node-sub">max depth: {d.max_depth}</div>
      {(d.allowed_tools ?? []).length > 0 && (
        <div style={{ fontSize: 9, color: '#22c55e', marginTop: 2 }}>
          allow: {d.allowed_tools.length} tools
        </div>
      )}
      {(d.denied_tools ?? []).length > 0 && (
        <div style={{ fontSize: 9, color: '#ef4444', marginTop: 2 }}>
          deny: {d.denied_tools.length} tools
        </div>
      )}
      <Handle type="source" position={Position.Right} id="tools_out" style={{ background: '#3b82f6' }} />
    </div>
  );
}

export default memo(DepthPolicyNode);
