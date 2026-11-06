import { useCallback, useEffect, useMemo, type DragEvent } from 'react';
import {
  Background,
  BackgroundVariant,
  ConnectionMode,
  Controls,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  type Edge,
} from '@xyflow/react';
import { linkLive } from '../lib/connectivity';
import { resolveState } from '../lib/status';
import { useLab } from '../store/useLab';
import type { MachineNode, PaletteItem } from '../types';
import { LockIcon } from './icons';
import { MachineNodeView } from './MachineNodeView';

const nodeTypes = { machine: MachineNodeView };
export const DRAG_MIME = 'application/vmlab';

/** Punto del puntero, tanto para ratón como para pantalla táctil. */
function pointerPoint(e: MouseEvent | TouchEvent): { x: number; y: number } | null {
  if ('touches' in e) {
    const t = e.touches[0] ?? e.changedTouches[0];
    return t ? { x: t.clientX, y: t.clientY } : null;
  }
  return { x: e.clientX, y: e.clientY };
}

/** ¿El puntero está sobre el inventario de máquinas? */
function overSidebar(e: MouseEvent | TouchEvent): boolean {
  const p = pointerPoint(e);
  const el = document.querySelector('.sidebar');
  if (!p || !el) return false;
  const r = el.getBoundingClientRect();
  return p.x >= r.left && p.x <= r.right && p.y >= r.top && p.y <= r.bottom;
}

function Flow() {
  const nodes = useLab((s) => s.nodes);
  const edges = useLab((s) => s.edges);
  const snapshot = useLab((s) => s.snapshot);
  const probes = useLab((s) => s.probes);
  const theme = useLab((s) => s.theme);
  const canvasLocked = useLab((s) => s.canvasLocked);
  const refreshProbes = useLab((s) => s.refreshProbes);
  const { onNodesChange, onEdgesChange, onConnect, addMachine } = useLab.getState();
  const setOverSidebar = useLab((s) => s.setOverSidebar);
  const removeNode = useLab((s) => s.removeNode);
  const toast = useLab((s) => s.toast);
  const { screenToFlowPosition } = useReactFlow();

  useEffect(() => () => setOverSidebar(false), [setOverSidebar]);

  // Sonda periódica: el cable pasa a verde en cuanto ambos extremos se ven en la
  // misma red. Se relanza al arrancar/parar VMs y al cambiar adaptador o modo.
  const runningKey = nodes
    .filter((n) => resolveState(n.data, snapshot) === 'running')
    .map((n) => n.id)
    .sort()
    .join(',');
  const netKey = nodes
    .map((n) => `${n.id}:${n.data.net?.mode ?? 'intnet'}:${n.data.net?.adapter ?? 2}`)
    .join('|');
  const edgeKey = edges.map((e) => e.id).sort().join(',');
  useEffect(() => {
    if (edges.length === 0) return;
    void refreshProbes();
    const timer = setInterval(() => void refreshProbes(), 5000);
    return () => clearInterval(timer);
  }, [edgeKey, edges.length, runningKey, netKey, refreshProbes]);

  // Verde animado: comunicación detectada. Rojo: apagadas, sin IP o red sin aplicar.
  const styledEdges = useMemo<Edge[]>(() => {
    const byId = new Map(nodes.map((n) => [n.id, n]));
    return edges.map((e) => {
      const source = byId.get(e.source);
      const target = byId.get(e.target);
      const live = !!(source && target && linkLive(source, target, probes, snapshot));
      return {
        ...e,
        type: 'smoothstep',
        animated: live,
        className: live ? 'edge-live' : 'edge-off',
        style: { stroke: live ? 'var(--running)' : 'var(--danger)', strokeWidth: 2.5 },
      };
    });
  }, [nodes, edges, snapshot, probes]);

  const onDragOver = useCallback((e: DragEvent) => {
    const types = e.dataTransfer.types;
    if (!types.includes(DRAG_MIME) && !types.includes('text/plain')) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
  }, []);

  const onDrop = useCallback(
    (e: DragEvent) => {
      let raw = e.dataTransfer.getData(DRAG_MIME);
      if (!raw) {
        const text = e.dataTransfer.getData('text/plain');
        if (text.startsWith('vmlab:')) raw = text.slice('vmlab:'.length);
      }
      if (!raw) return;
      e.preventDefault();
      try {
        const item = JSON.parse(raw) as PaletteItem;
        const p = screenToFlowPosition({ x: e.clientX, y: e.clientY });
        addMachine(item, { x: p.x - 100, y: p.y - 40 });
      } catch { /* payload ajeno: se ignora */ }
    },
    [addMachine, screenToFlowPosition],
  );

  // Arrastrar un nodo hasta el inventario lo devuelve al panel (sin borrar la VM).
  const onNodeDrag = useCallback(
    (e: MouseEvent | TouchEvent) => setOverSidebar(overSidebar(e)),
    [setOverSidebar],
  );

  const onNodeDragStop = useCallback(
    (e: MouseEvent | TouchEvent, node: MachineNode) => {
      const dropped = overSidebar(e);
      setOverSidebar(false);
      if (!dropped) return;
      removeNode(node.id);
      toast('info', `«${node.data.label}» devuelta al panel`);
    },
    [removeNode, setOverSidebar, toast],
  );

  return (
    <div className="canvas" onDragOver={onDragOver} onDrop={onDrop}>
      <ReactFlow
        nodes={nodes}
        edges={styledEdges}
        nodeTypes={nodeTypes}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        onNodeDragStart={() => setOverSidebar(false)}
        onNodeDrag={onNodeDrag}
        onNodeDragStop={onNodeDragStop}
        connectionMode={ConnectionMode.Loose}
        colorMode={theme}
        deleteKeyCode={['Backspace', 'Delete']}
        panOnDrag={!canvasLocked}
        autoPanOnNodeDrag={!canvasLocked}
        zoomOnScroll={!canvasLocked}
        zoomOnPinch={!canvasLocked}
        zoomOnDoubleClick={!canvasLocked}
        fitView
        fitViewOptions={{ maxZoom: 1, padding: 0.3 }}
        minZoom={0.2}
        maxZoom={1.75}
        snapToGrid
        snapGrid={[20, 20]}
      >
        <Background variant={BackgroundVariant.Dots} gap={20} size={1.4} />
        {!canvasLocked && <Controls showInteractive={false} />}
        <MiniMap pannable={!canvasLocked} zoomable={!canvasLocked} nodeColor={(n) => (n.data?.kind === 'vbox' ? '#8b7cf6' : '#38a8f0')} />
      </ReactFlow>
      {canvasLocked && (
        <div className="canvas-lock-badge" role="status">
          <LockIcon /> Lienzo bloqueado
        </div>
      )}
      {nodes.length === 0 && (
        <div className="empty">
          <h2>Tu laboratorio está vacío</h2>
          <p>Arrastra máquinas desde el panel izquierdo (o haz doble clic en ellas), conéctalas tirando de sus bordes y pulsa <b>Aplicar red</b>.</p>
        </div>
      )}
    </div>
  );
}

export function Canvas() {
  return (
    <ReactFlowProvider>
      <Flow />
    </ReactFlowProvider>
  );
}
