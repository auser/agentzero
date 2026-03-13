import { memo } from 'react';
import { type NodeProps } from '@xyflow/react';

interface AutonomyData {
  level: string;
  max_actions_per_hour: number;
  max_cost_per_day_cents: number;
}

const LEVEL_COLORS: Record<string, string> = {
  supervised: '#22c55e',
  semi: '#f59e0b',
  autonomous: '#ef4444',
  locked: '#6b7280',
};

function AutonomyNode({ data, selected }: NodeProps) {
  const d = data as unknown as AutonomyData;

  return (
    <div className={`az-node${selected ? ' selected' : ''}`} style={{ borderColor: selected ? undefined : '#f97316' }}>
      <div className="az-node-label" style={{ color: '#fb923c' }}>Autonomy</div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 5, marginTop: 3 }}>
        <div className="az-node-dot" style={{ background: LEVEL_COLORS[d.level] ?? '#6b7280' }} />
        <span className="az-node-sub">{d.level}</span>
      </div>
      <div className="az-node-sub" style={{ marginTop: 2 }}>
        {d.max_actions_per_hour} actions/hr &middot; ${(d.max_cost_per_day_cents / 100).toFixed(0)}/day
      </div>
    </div>
  );
}

export default memo(AutonomyNode);
