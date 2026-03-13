import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';

interface RouteData {
  hint: string;
  provider: string;
  model: string;
}

function ModelRouteNode({ data, selected }: NodeProps) {
  const d = data as unknown as RouteData;

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : '#f59e0b' }}>
      <Handle type="target" position={Position.Left} id="rule_in" className="az-handle az-handle-classification" />
      <div className="az-node-label" style={{ color: '#fbbf24' }}>
        {d.hint || 'untitled route'}
      </div>
      <div className="az-node-sub">{d.provider}/{d.model}</div>
      <Handle type="source" position={Position.Right} id="agent_out" className="az-handle az-handle-route" />
    </div>
  );
}

export default memo(ModelRouteNode);
