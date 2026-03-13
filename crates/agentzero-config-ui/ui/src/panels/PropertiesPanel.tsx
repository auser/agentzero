import { useGraphStore } from '../store/graphStore';
import type { PropertyDescriptor } from '../types';

export default function PropertiesPanel() {
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId);
  const nodes = useGraphStore((s) => s.nodes);
  const nodeTypes = useGraphStore((s) => s.nodeTypes);
  const updateNodeData = useGraphStore((s) => s.updateNodeData);

  const node = nodes.find((n) => n.id === selectedNodeId);
  if (!node) {
    return <div className="props-empty">Select a node to edit its properties</div>;
  }

  const schema = nodeTypes.find((t) => t.type_id === node.type);
  const properties = schema?.properties ?? [];

  const groups = new Map<string, PropertyDescriptor[]>();
  for (const prop of properties) {
    const group = prop.group ?? '';
    if (!groups.has(group)) groups.set(group, []);
    groups.get(group)!.push(prop);
  }

  return (
    <div className="props-panel">
      <div className="props-title">{schema?.display_name ?? node.type}</div>

      {[...groups.entries()].map(([group, props]) => (
        <div key={group}>
          {group && <div className="props-group-label" style={{ color: '#60a5fa' }}>{group}</div>}
          {props.map((prop) => (
            <PropertyField
              key={prop.key}
              prop={prop}
              value={node.data[prop.key]}
              onChange={(val) => updateNodeData(node.id, { [prop.key]: val })}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

function PropertyField({
  prop,
  value,
  onChange,
}: {
  prop: PropertyDescriptor;
  value: unknown;
  onChange: (val: unknown) => void;
}) {
  return (
    <div className="prop-field">
      <label className="prop-label">
        {prop.label}
        {prop.description && <span className="prop-help" title={prop.description}>?</span>}
      </label>

      {prop.kind === 'bool' && (
        <label className="prop-toggle">
          <input
            type="checkbox"
            checked={!!value}
            onChange={(e) => onChange(e.target.checked)}
          />
          <span>{value ? 'On' : 'Off'}</span>
        </label>
      )}

      {prop.kind === 'string' && (
        <input
          type="text"
          className="prop-input"
          value={(value as string) ?? ''}
          onChange={(e) => onChange(e.target.value)}
        />
      )}

      {prop.kind === 'number' && (
        <input
          type="number"
          className="prop-input"
          value={value != null ? Number(value) : ''}
          onChange={(e) => onChange(e.target.value ? Number(e.target.value) : null)}
        />
      )}

      {prop.kind === 'text' && (
        <textarea
          className="prop-input"
          value={(value as string) ?? ''}
          onChange={(e) => onChange(e.target.value)}
          rows={3}
          style={{ resize: 'vertical' }}
        />
      )}

      {prop.kind === 'enum' && (
        <select
          className="prop-input"
          value={(value as string) ?? ''}
          onChange={(e) => onChange(e.target.value)}
        >
          {(prop.enum_values ?? []).map((v) => (
            <option key={v} value={v}>{v}</option>
          ))}
        </select>
      )}

      {prop.kind === 'string_list' && (
        <input
          type="text"
          className="prop-input"
          value={Array.isArray(value) ? (value as string[]).join(', ') : ''}
          onChange={(e) =>
            onChange(
              e.target.value
                .split(',')
                .map((s) => s.trim())
                .filter((s) => s),
            )
          }
          placeholder="comma-separated values"
        />
      )}
    </div>
  );
}
