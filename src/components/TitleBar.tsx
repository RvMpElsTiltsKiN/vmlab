import { useEffect, useState } from 'react';
import { isTauri, windowControls } from '../lib/ipc';
import { useLab } from '../store/useLab';
import { CloseIcon, LockIcon, Logo, MaxIcon, MinIcon, MoonIcon, NetworkIcon, PlayIcon, RestoreIcon, StopIcon, SunIcon, UnlockIcon } from './icons';

/**
 * Barra de título propia (la ventana se crea con `decorations: false`).
 * `data-tauri-drag-region` solo actúa sobre el elemento que lo lleva, no sobre sus hijos,
 * por eso los botones siguen siendo clicables y el resto de la barra arrastra la ventana.
 */
export function TitleBar() {
  const theme = useLab((s) => s.theme);
  const toggleTheme = useLab((s) => s.toggleTheme);
  const applying = useLab((s) => s.applying);
  const applyNetwork = useLab((s) => s.applyNetwork);
  const actAll = useLab((s) => s.actAll);
  const canvasLocked = useLab((s) => s.canvasLocked);
  const toggleCanvasLock = useLab((s) => s.toggleCanvasLock);
  const hasNodes = useLab((s) => s.nodes.length > 0);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const sync = () => windowControls.isMaximized().then(setMaximized);
    sync();
    const off = windowControls.onResized(sync);
    return () => { off.then((f) => f()); };
  }, []);

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <Logo />
        <span className="brand-name" data-tauri-drag-region>VMLab</span>
      </div>

      <div className="titlebar-actions">
        <button className="btn primary" onClick={applyNetwork} disabled={!hasNodes || applying} title="Crea las redes definidas por las conexiones">
          <NetworkIcon /> {applying ? 'Aplicando…' : 'Aplicar red'}
        </button>
        <button className="btn" onClick={() => actAll('start')} disabled={!hasNodes} title="Iniciar todas las máquinas del lienzo">
          <PlayIcon /> Iniciar todo
        </button>
        <button className="btn" onClick={() => actAll('stop')} disabled={!hasNodes} title="Detener todas las máquinas del lienzo">
          <StopIcon /> Detener todo
        </button>
        <button
          className={`btn lock-toggle ${canvasLocked ? 'on' : ''}`}
          onClick={toggleCanvasLock}
          aria-pressed={canvasLocked}
          title={canvasLocked
            ? 'Lienzo bloqueado: no se desplaza ni hace zoom. Pulsa para desbloquear.'
            : 'Bloquear el lienzo para que no se desplace al arrastrar nodos'}
        >
          {canvasLocked ? <LockIcon /> : <UnlockIcon />} {canvasLocked ? 'Lienzo bloqueado' : 'Bloquear lienzo'}
        </button>
      </div>

      <div className="titlebar-spacer" data-tauri-drag-region />

      <button className="icon-btn" onClick={toggleTheme} title={theme === 'dark' ? 'Modo claro' : 'Modo oscuro'} aria-label="Cambiar tema">
        {theme === 'dark' ? <SunIcon /> : <MoonIcon />}
      </button>

      {isTauri && (
        <div className="win-controls">
          <button onClick={() => windowControls.minimize()} aria-label="Minimizar"><MinIcon /></button>
          <button onClick={() => windowControls.toggleMaximize()} aria-label={maximized ? 'Restaurar' : 'Maximizar'}>
            {maximized ? <RestoreIcon /> : <MaxIcon />}
          </button>
          <button className="close" onClick={() => windowControls.close()} aria-label="Cerrar"><CloseIcon /></button>
        </div>
      )}
    </header>
  );
}
