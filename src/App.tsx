import { useEffect } from 'react';
import { Canvas } from './components/Canvas';
import { Inspector } from './components/Inspector';
import { Sidebar } from './components/Sidebar';
import { TitleBar } from './components/TitleBar';
import { useLab } from './store/useLab';

function Toasts() {
  const toasts = useLab((s) => s.toasts);
  const dismiss = useLab((s) => s.dismissToast);
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`} onClick={() => dismiss(t.id)}>{t.text}</div>
      ))}
    </div>
  );
}

export default function App() {
  const init = useLab((s) => s.init);
  useEffect(() => { void init(); }, [init]);

  return (
    <div className="app">
      <TitleBar />
      <main className="workspace">
        <Sidebar />
        <Canvas />
        <Inspector />
      </main>
      <Toasts />
    </div>
  );
}
