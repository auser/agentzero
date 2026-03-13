import { useGraphStore } from '../store/graphStore';

export default function ValidationPanel() {
  const errors = useGraphStore((s) => s.validationErrors);
  const isValid = useGraphStore((s) => s.isValid);
  const setSelectedNode = useGraphStore((s) => s.setSelectedNode);

  if (errors.length === 0) {
    return <div className="validation-ok">No validation errors</div>;
  }

  return (
    <div>
      <div
        className="validation-count"
        style={{ color: isValid ? '#f59e0b' : '#ef4444' }}
      >
        {errors.length} issue{errors.length !== 1 ? 's' : ''}
      </div>
      {errors.map((err, i) => (
        <div
          key={i}
          className={`validation-item ${err.severity} ${err.node_id ? 'clickable' : ''}`}
          onClick={() => err.node_id && setSelectedNode(err.node_id)}
        >
          {err.field && <span className="validation-field">{err.field}: </span>}
          {err.message}
        </div>
      ))}
    </div>
  );
}
