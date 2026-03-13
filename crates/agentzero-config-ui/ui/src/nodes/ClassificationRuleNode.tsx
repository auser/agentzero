import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';

interface RuleData {
  hint: string;
  keywords: string[];
  priority: number;
}

function ClassificationRuleNode({ data, selected }: NodeProps) {
  const d = data as unknown as RuleData;

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : '#06b6d4' }}>
      <div className="az-node-label" style={{ color: '#22d3ee' }}>
        {d.hint || 'untitled rule'}
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3, marginTop: 4 }}>
        {(d.keywords ?? []).slice(0, 5).map((kw, i) => (
          <span key={i} className="keyword-chip">{kw}</span>
        ))}
      </div>
      {d.priority !== 0 && (
        <div className="az-node-sub" style={{ marginTop: 3 }}>priority: {d.priority}</div>
      )}
      <Handle type="source" position={Position.Right} id="route_out" className="az-handle az-handle-route" />
    </div>
  );
}

export default memo(ClassificationRuleNode);
