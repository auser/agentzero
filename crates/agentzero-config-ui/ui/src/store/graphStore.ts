import { create } from 'zustand';
import {
  type Node,
  type Edge,
  type Connection,
  type NodeChange,
  type EdgeChange,
  applyNodeChanges,
  applyEdgeChanges,
  addEdge,
} from '@xyflow/react';
import type { GraphModel, ValidationError, NodeTypeDescriptor, ToolSummary } from '../types';
import { exportToml, validateGraph as apiValidate } from '../api';

interface GraphSnapshot {
  nodes: Node[];
  edges: Edge[];
}

interface GraphState {
  nodes: Node[];
  edges: Edge[];
  nodeTypes: NodeTypeDescriptor[];
  tools: ToolSummary[];
  tomlPreview: string;
  tomlLoading: boolean;
  validationErrors: ValidationError[];
  isValid: boolean;
  selectedNodeId: string | null;
  history: GraphSnapshot[];
  historyIndex: number;

  setNodeTypes: (types: NodeTypeDescriptor[]) => void;
  setTools: (tools: ToolSummary[]) => void;
  onNodesChange: (changes: NodeChange[]) => void;
  onEdgesChange: (changes: EdgeChange[]) => void;
  onConnect: (connection: Connection) => void;
  setSelectedNode: (id: string | null) => void;
  updateNodeData: (id: string, data: Record<string, unknown>) => void;
  addNode: (typeId: string, position: { x: number; y: number }, data?: Record<string, unknown>, parentId?: string) => void;
  addToolToAgent: (toolName: string, agentId: string) => void;
  removeNode: (id: string) => void;
  loadGraph: (graph: GraphModel) => void;
  syncToml: () => void;
  runValidation: () => void;
  undo: () => void;
  redo: () => void;
  pushHistory: () => void;
  getGraphModel: () => GraphModel;
  debouncedSync: () => void;
  toggleCollapse: (agentId: string) => void;
}

let nextId = 100;
function genId() {
  return `n${nextId++}`;
}

let syncTimer: ReturnType<typeof setTimeout> | null = null;

export const useGraphStore = create<GraphState>((set, get) => ({
  nodes: [],
  edges: [],
  nodeTypes: [],
  tools: [],
  tomlPreview: '',
  tomlLoading: false,
  validationErrors: [],
  isValid: true,
  selectedNodeId: null,
  history: [],
  historyIndex: -1,

  setNodeTypes: (types) => set({ nodeTypes: types }),
  setTools: (tools) => set({ tools }),

  onNodesChange: (changes) => {
    set((state) => ({ nodes: applyNodeChanges(changes, state.nodes) }));
    get().debouncedSync();
  },

  onEdgesChange: (changes) => {
    set((state) => ({ edges: applyEdgeChanges(changes, state.edges) }));
    get().debouncedSync();
  },

  onConnect: (connection) => {
    get().pushHistory();
    set((state) => ({
      edges: addEdge(
        { ...connection, id: `e${Date.now()}`, type: 'animated' },
        state.edges,
      ),
    }));
    get().debouncedSync();
  },

  setSelectedNode: (id) => set({ selectedNodeId: id }),

  updateNodeData: (id, data) => {
    get().pushHistory();
    set((state) => ({
      nodes: state.nodes.map((node) =>
        node.id === id ? { ...node, data: { ...node.data, ...data } } : node,
      ),
    }));
    get().debouncedSync();
  },

  addNode: (typeId, position, data, parentId) => {
    get().pushHistory();
    const schema = get().nodeTypes.find((t) => t.type_id === typeId);
    const defaultData: Record<string, unknown> = {};
    if (schema) {
      for (const prop of schema.properties) {
        defaultData[prop.key] = prop.default_value;
      }
    }
    const id = genId();
    const node: Node = {
      id,
      type: typeId,
      position,
      data: { ...defaultData, ...data },
    };
    // If it's an agent (group container), set dimensions
    if (typeId === 'agent') {
      node.style = { width: 560, height: 420 };
    }
    // If it has a parent, nest it
    if (parentId) {
      node.parentId = parentId;
      node.extent = 'parent';
      node.expandParent = true;
    }
    set((state) => ({
      nodes: [...state.nodes, node],
    }));
    get().debouncedSync();
    return id;
  },

  addToolToAgent: (toolName, agentId) => {
    get().pushHistory();
    const tool = get().tools.find((t) => t.name === toolName);
    const agentNode = get().nodes.find((n) => n.id === agentId);
    if (!agentNode) return;

    // Count existing children of this agent
    const childCount = get().nodes.filter((n) => n.parentId === agentId).length;
    const col = childCount % 3;
    const row = Math.floor(childCount / 3);

    const toolId = genId();
    set((state) => ({
      nodes: [
        ...state.nodes,
        {
          id: toolId,
          type: 'tool',
          // Position relative to parent agent
          position: { x: 20 + col * 175, y: 70 + row * 75 },
          parentId: agentId,
          extent: 'parent' as const,
          expandParent: true,
          data: {
            name: toolName,
            enabled: true,
            description: tool?.description ?? '',
            category: tool?.category ?? 'system',
          },
        },
      ],
    }));
    get().debouncedSync();
  },

  removeNode: (id) => {
    get().pushHistory();
    set((state) => ({
      // Also remove children of removed node
      nodes: state.nodes.filter((n) => n.id !== id && n.parentId !== id),
      edges: state.edges.filter((e) => e.source !== id && e.target !== id),
      selectedNodeId: state.selectedNodeId === id ? null : state.selectedNodeId,
    }));
    get().debouncedSync();
  },

  toggleCollapse: (agentId) => {
    get().pushHistory();
    set((state) => {
      const agent = state.nodes.find((n) => n.id === agentId);
      if (!agent) return state;
      const collapsed = !(agent.data as Record<string, unknown>).collapsed;
      return {
        nodes: state.nodes.map((n) => {
          if (n.id === agentId) {
            return {
              ...n,
              data: { ...n.data, collapsed },
              style: collapsed
                ? { width: 300, height: 52 }
                : { width: 560, height: 420 },
            };
          }
          // Hide/show children
          if (n.parentId === agentId) {
            return { ...n, hidden: collapsed };
          }
          return n;
        }),
      };
    });
    get().debouncedSync();
  },

  loadGraph: (graph) => {
    // Ensure parent nodes appear before children in the array
    const parentIds = new Set(
      graph.nodes.filter((n) => n.parent_id).map((n) => n.parent_id!),
    );
    const parents = graph.nodes.filter((n) => parentIds.has(n.id));
    const children = graph.nodes.filter((n) => n.parent_id);
    const others = graph.nodes.filter(
      (n) => !parentIds.has(n.id) && !n.parent_id,
    );

    const orderedGraphNodes = [...parents, ...others, ...children];

    const nodes: Node[] = orderedGraphNodes.map((n) => {
      const node: Node = {
        id: n.id,
        type: n.type_id,
        position: n.position,
        data: n.data,
      };
      // Agent group container
      if (n.type_id === 'agent') {
        node.style = {
          width: n.width ?? 560,
          height: n.height ?? 420,
        };
      }
      // Child node
      if (n.parent_id) {
        node.parentId = n.parent_id;
        node.extent = 'parent';
        node.expandParent = true;
      }
      return node;
    });

    const edges: Edge[] = graph.edges.map((e) => ({
      id: e.id,
      source: e.source,
      sourceHandle: e.source_port,
      target: e.target,
      targetHandle: e.target_port,
      type: 'animated',
      animated: true,
    }));

    set({
      nodes,
      edges,
      history: [{ nodes, edges }],
      historyIndex: 0,
      selectedNodeId: null,
    });
    get().syncToml();
  },

  getGraphModel: (): GraphModel => {
    const { nodes, edges } = get();
    return {
      nodes: nodes.map((n) => ({
        id: n.id,
        type_id: n.type ?? 'unknown',
        position: n.position,
        data: n.data as Record<string, unknown>,
        parent_id: n.parentId,
        width: n.type === 'agent' ? (n.style?.width as number | undefined) : undefined,
        height: n.type === 'agent' ? (n.style?.height as number | undefined) : undefined,
      })),
      edges: edges.map((e) => ({
        id: e.id,
        source: e.source,
        source_port: e.sourceHandle ?? '',
        target: e.target,
        target_port: e.targetHandle ?? '',
        edge_type: 'default',
      })),
      viewport: { x: 0, y: 0, zoom: 1 },
    };
  },

  syncToml: async () => {
    const graph = get().getGraphModel();
    set({ tomlLoading: true });
    try {
      const res = await exportToml(graph);
      set({ tomlPreview: res.toml, tomlLoading: false });
    } catch {
      set({ tomlLoading: false });
    }
  },

  runValidation: async () => {
    const graph = get().getGraphModel();
    try {
      const res = await apiValidate(graph);
      set({ validationErrors: res.errors, isValid: res.valid });
    } catch {
      // noop
    }
  },

  pushHistory: () => {
    const { nodes, edges, history, historyIndex } = get();
    const newHistory = history.slice(0, historyIndex + 1);
    newHistory.push({ nodes: [...nodes], edges: [...edges] });
    if (newHistory.length > 50) newHistory.shift();
    set({ history: newHistory, historyIndex: newHistory.length - 1 });
  },

  undo: () => {
    const { history, historyIndex } = get();
    if (historyIndex <= 0) return;
    const prev = history[historyIndex - 1];
    set({ nodes: prev.nodes, edges: prev.edges, historyIndex: historyIndex - 1 });
    get().debouncedSync();
  },

  redo: () => {
    const { history, historyIndex } = get();
    if (historyIndex >= history.length - 1) return;
    const next = history[historyIndex + 1];
    set({ nodes: next.nodes, edges: next.edges, historyIndex: historyIndex + 1 });
    get().debouncedSync();
  },

  debouncedSync: () => {
    if (syncTimer) clearTimeout(syncTimer);
    syncTimer = setTimeout(() => {
      get().syncToml();
      get().runValidation();
    }, 300);
  },
}));
