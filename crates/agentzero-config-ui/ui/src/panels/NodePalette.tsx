import { useState } from 'react';
import { useGraphStore } from '../store/graphStore';

export default function NodePalette() {
  const [search, setSearch] = useState('');
  const nodeTypes = useGraphStore((s) => s.nodeTypes);
  const addNode = useGraphStore((s) => s.addNode);

  const filtered = nodeTypes.filter(
    (t) =>
      t.display_name.toLowerCase().includes(search.toLowerCase()) ||
      t.category.toLowerCase().includes(search.toLowerCase()),
  );

  const groups = new Map<string, typeof filtered>();
  for (const t of filtered) {
    if (!groups.has(t.category)) groups.set(t.category, []);
    groups.get(t.category)!.push(t);
  }

  return (
    <div>
      <div className="panel-header">Nodes</div>
      <div style={{ padding: '0 12px' }}>
        <input
          type="text"
          placeholder="Search nodes..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="palette-search"
        />
      </div>
      {[...groups.entries()].map(([category, types]) => (
        <div key={category} style={{ padding: '0 12px' }}>
          <div className="palette-group-label">{category}</div>
          {types.map((t) => (
            <button
              key={t.type_id}
              className="palette-item"
              onClick={() => {
                const x = 200 + Math.random() * 400;
                const y = 200 + Math.random() * 300;
                addNode(t.type_id, { x, y });
              }}
            >
              <div className="palette-item-dot" style={{ background: t.color }} />
              <span>{t.display_name}</span>
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}
