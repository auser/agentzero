import { memo, useCallback } from 'react';
import { Handle, Position, NodeResizer, type NodeProps } from '@xyflow/react';
import { useGraphStore } from '../store/graphStore';

interface AgentGroupData {
  name: string;
  provider: string;
  model: string;
  agentic: boolean;
  privacy_boundary: string;
  collapsed?: boolean;
}

function AgentGroupNode({ id, data, selected }: NodeProps) {
  const d = data as unknown as AgentGroupData;
  const toggleCollapse = useGraphStore((s) => s.toggleCollapse);

  const handleCollapse = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      toggleCollapse(id);
    },
    [id, toggleCollapse],
  );

  return (
    <>
      <NodeResizer
        color="#8b5cf6"
        isVisible={selected ?? false}
        minWidth={300}
        minHeight={d.collapsed ? 52 : 150}
      />
      {/* Incoming handles on the agent container */}
      <Handle
        type="target"
        position={Position.Left}
        id="security_in"
        className="az-handle az-handle-security"
      />
      <Handle
        type="target"
        position={Position.Top}
        id="parent_in"
        className="az-handle az-handle-delegation"
      />

      <div className="az-group-header">
        <div className="az-group-header-left">
          <div className="az-group-dot" />
          <span className="az-group-name">{d.name || 'Unnamed Agent'}</span>
          <span className="az-group-model">{d.model || 'no model'}</span>
          {d.agentic && <span className="az-group-badge">agentic</span>}
          {d.privacy_boundary && d.privacy_boundary !== 'inherit' && (
            <span className="az-group-badge az-group-badge-warn">{d.privacy_boundary}</span>
          )}
        </div>
        <button className="az-group-collapse-btn" onClick={handleCollapse}>
          {d.collapsed ? '+' : '\u2013'}
        </button>
      </div>

      {!d.collapsed && <div className="az-group-body" />}

      {/* Outgoing handles */}
      <Handle
        type="source"
        position={Position.Bottom}
        id="delegate_out"
        className="az-handle az-handle-delegation"
      />
      <Handle
        type="source"
        position={Position.Right}
        id="route_out"
        className="az-handle az-handle-route"
      />
    </>
  );
}

export default memo(AgentGroupNode);
