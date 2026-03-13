import { useGraphStore } from '../store/graphStore';

export default function TomlPreviewPanel() {
  const tomlPreview = useGraphStore((s) => s.tomlPreview);
  const tomlLoading = useGraphStore((s) => s.tomlLoading);

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div className="panel-header">
        TOML Preview
        {tomlLoading && <span className="toml-syncing">syncing...</span>}
      </div>
      <pre className="toml-pre">
        {tomlPreview || '# Edit the graph to generate TOML config'}
      </pre>
    </div>
  );
}
