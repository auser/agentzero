import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ReactFlow,
  MiniMap,
  Background,
  BackgroundVariant,
  Controls,
  type NodeTypes,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

import { useGraphStore } from './store/graphStore';
import { fetchSchema, fetchDefaults, fetchTools, importToml, exportToml } from './api';

import ToolNode from './nodes/ToolNode';
import SecurityPolicyNode from './nodes/SecurityPolicyNode';
import AgentGroupNode from './nodes/AgentGroupNode';
import ModelRouteNode from './nodes/ModelRouteNode';
import ClassificationRuleNode from './nodes/ClassificationRuleNode';
import DepthPolicyNode from './nodes/DepthPolicyNode';
import ProviderNode from './nodes/ProviderNode';
import AutonomyNode from './nodes/AutonomyNode';
import PluginNode from './nodes/PluginNode';

import Sidebar from './panels/Sidebar';
import PropertiesPanel from './panels/PropertiesPanel';
import TomlPreviewPanel from './panels/TomlPreviewPanel';
import ValidationPanel from './panels/ValidationPanel';

const nodeTypes: NodeTypes = {
  tool: ToolNode,
  security_policy: SecurityPolicyNode,
  agent: AgentGroupNode,
  model_route: ModelRouteNode,
  classification_rule: ClassificationRuleNode,
  depth_policy: DepthPolicyNode,
  provider: ProviderNode,
  autonomy: AutonomyNode,
  plugin: PluginNode,
};

const MINIMAP_COLORS: Record<string, string> = {
  tool: '#3b82f6',
  security_policy: '#ef4444',
  agent: '#8b5cf6',
  model_route: '#f59e0b',
  classification_rule: '#06b6d4',
  depth_policy: '#10b981',
  provider: '#ec4899',
  autonomy: '#f97316',
  plugin: '#a855f7',
};

export default function App() {
  const {
    nodes, edges, onNodesChange, onEdgesChange, onConnect,
    setSelectedNode, setNodeTypes, setTools, loadGraph, undo, redo, getGraphModel,
  } = useGraphStore();

  const [bottomPanel, setBottomPanel] = useState<'toml' | 'validation'>('toml');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let cancelled = false;
    let attempt = 0;

    async function load() {
      while (!cancelled && attempt < 20) {
        attempt++;
        try {
          const [schema, defaults, tools] = await Promise.all([
            fetchSchema(), fetchDefaults(), fetchTools(),
          ]);
          if (cancelled) return;
          setNodeTypes(schema);
          setTools(tools);
          loadGraph(defaults);
          setLoading(false);
          setError(null);
          return;
        } catch {
          if (cancelled) return;
          setError(`Connecting to backend... (attempt ${attempt})`);
          await new Promise((r) => setTimeout(r, 1000));
        }
      }
      if (!cancelled) {
        setLoading(false);
        setError('Could not connect to backend. Is the server running?');
      }
    }
    load();
    return () => { cancelled = true; };
  }, [setNodeTypes, setTools, loadGraph]);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === 'z' && !e.shiftKey) {
        e.preventDefault();
        undo();
      }
      if ((e.metaKey || e.ctrlKey) && e.key === 'z' && e.shiftKey) {
        e.preventDefault();
        redo();
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [undo, redo]);

  const handleNodeClick = useCallback(
    (_: React.MouseEvent, node: { id: string }) => setSelectedNode(node.id),
    [setSelectedNode],
  );
  const handlePaneClick = useCallback(() => setSelectedNode(null), [setSelectedNode]);

  const handleExport = useCallback(async () => {
    try {
      const graph = getGraphModel();
      const res = await exportToml(graph);
      const blob = new Blob([res.toml], { type: 'text/plain' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'agentzero.toml';
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      console.error('Export failed:', e);
    }
  }, [getGraphModel]);

  const handleImportFile = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      try {
        const text = await file.text();
        const res = await importToml(text);
        loadGraph(res.graph);
      } catch (err) {
        console.error('Import failed:', err);
      }
    },
    [loadGraph],
  );

  if (loading) {
    return (
      <div className="loading-screen">
        <div className="loading-spinner" />
        <div style={{ marginTop: 16, color: 'var(--text-muted)' }}>
          {error ?? 'Loading...'}
        </div>
      </div>
    );
  }

  if (error && nodes.length === 0) {
    return (
      <div className="loading-screen">
        <div style={{ color: 'var(--danger)', fontSize: 14 }}>{error}</div>
        <button className="toolbar-btn" style={{ marginTop: 16 }} onClick={() => window.location.reload()}>
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="app-layout">
      <div className="toolbar">
        <span className="toolbar-title">AgentZero Config</span>
        <button className="toolbar-btn" onClick={() => fileInputRef.current?.click()}>
          Import
        </button>
        <input
          ref={fileInputRef}
          type="file"
          accept=".toml"
          onChange={handleImportFile}
          style={{ display: 'none' }}
        />
        <button className="toolbar-btn" onClick={handleExport}>Export TOML</button>
        <div className="toolbar-sep" />
        <button className="toolbar-btn" onClick={undo}>Undo</button>
        <button className="toolbar-btn" onClick={redo}>Redo</button>
      </div>

      <div className="panel-left">
        <Sidebar />
      </div>

      <div className="canvas-wrap">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onNodeClick={handleNodeClick}
          onPaneClick={handlePaneClick}
          nodeTypes={nodeTypes}
          fitView
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} gap={24} size={1} color="#1a2540" />
          <Controls showInteractive={false} />
          <MiniMap
            pannable
            zoomable
            nodeColor={(n) => MINIMAP_COLORS[n.type ?? ''] ?? '#4a5a78'}
          />
        </ReactFlow>
      </div>

      <div className="panel-right">
        <PropertiesPanel />
      </div>

      <div className="panel-bottom">
        <div className="tabs-bar">
          <button
            className={`tab-btn ${bottomPanel === 'toml' ? 'active' : ''}`}
            onClick={() => setBottomPanel('toml')}
          >
            TOML Preview
          </button>
          <button
            className={`tab-btn ${bottomPanel === 'validation' ? 'active' : ''}`}
            onClick={() => setBottomPanel('validation')}
          >
            Validation
          </button>
        </div>
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {bottomPanel === 'toml' ? <TomlPreviewPanel /> : <ValidationPanel />}
        </div>
      </div>
    </div>
  );
}
