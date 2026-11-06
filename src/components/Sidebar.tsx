import { useEffect, useState, type DragEvent } from 'react';
import { ipc } from '../lib/ipc';
import { STATE_LABEL } from '../lib/status';
import { useLab } from '../store/useLab';
import type { GuestAdditionsPrep, MachineInfo, PaletteItem } from '../types';
import { DRAG_MIME } from './Canvas';
import { CheckIcon, ContainerIcon, CubeIcon, PlayIcon, PlusIcon, RefreshIcon, WarnIcon } from './icons';

const NAME_RE = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$/;
const IMAGE_RE = /^[A-Za-z0-9][A-Za-z0-9._/:@-]{0,199}$/;
const errText = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e));

function GuestAdditions({ m }: { m: MachineInfo }) {
  const refresh = useLab((s) => s.refreshInventory);
  const toast = useLab((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const [prep, setPrep] = useState<GuestAdditionsPrep | null>(null);
  const [error, setError] = useState<string | null>(null);
  const version = m.guestAdditions?.version ?? null;
  const known = m.guestAdditions?.known ?? false;

  // Al detectar una versión nueva (tras instalar y comprobar) se oculta la guía.
  useEffect(() => {
    if (version) {
      setPrep(null);
      setError(null);
    }
  }, [version]);

  const prepare = async () => {
    setBusy(true);
    setError(null);
    setPrep(null);
    try {
      const p = await ipc.prepareGuestAdditions(m.target);
      setPrep(p);
      toast('ok', 'Guest Additions: ISO preparada. Ejecuta el instalador dentro de la VM.');
    } catch (e) {
      const text = errText(e);
      setError(text);
      toast('error', text);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ga-box">
      {version ? (
        <span className="ga-chip ok" title="Guest Additions instaladas">
          <CheckIcon /> Guest Additions {version}
        </span>
      ) : known ? (
        <button
          className="ga-chip missing"
          onClick={prepare}
          onPointerDown={(e) => e.stopPropagation()}
          draggable={false}
          disabled={busy}
          title="Monta la ISO de Guest Additions y abre la VM para instalarlas"
        >
          <WarnIcon /> {busy ? 'Preparando instalación…' : 'Instalar Guest Additions'}
        </button>
      ) : (
        <span className="ga-chip unknown" title="VirtualBox no pudo consultar la VM (¿está bloqueada por la ventana de VirtualBox?)">
          <WarnIcon /> Guest Additions: sin datos
        </span>
      )}

      {prep && (
        <div className="ga-guide" onPointerDown={(e) => e.stopPropagation()}>
          <p>
            <b>{prep.attached ? 'ISO montada' : 'ISO no montada'}</b>
            {prep.started ? ' · VM abierta' : prep.alreadyRunning ? ' · VM ya en marcha' : ''}
          </p>
          {prep.os === 'windows' ? (
            <p>En la VM, abre la unidad de CD/DVD y ejecuta <b>VBoxWindowsAdditions.exe</b> como administrador. Reinicia al terminar.</p>
          ) : prep.os === 'linux' ? (
            <>
              <p>En la VM, monta la ISO y ejecuta el instalador:</p>
              <code>sudo mount /dev/cdrom /mnt</code>
              <code>sudo /mnt/VBoxLinuxAdditions.run</code>
              <p>Si pide compilador, instala <b>build-essential</b> y los headers de tu kernel.</p>
            </>
          ) : (
            <p>En la VM, ejecuta el instalador de la unidad de CD/DVD y reinicia.</p>
          )}
          {prep.warnings.map((w, i) => <p key={i} className="hint warn">{w}</p>)}
          <button className="btn" onClick={() => { refresh(); toast('info', 'Revisando de nuevo las Guest Additions…'); }}>
            Ya está instalado · comprobar
          </button>
        </div>
      )}
      {error && <p className="hint err">{error}</p>}
    </div>
  );
}

function Item({ m, used }: { m: MachineInfo; used: boolean }) {
  const addMachine = useLab((s) => s.addMachine);
  const item: PaletteItem = { kind: m.kind, label: m.label, target: m.target, image: m.image ?? undefined };
  const fromControl = (target: EventTarget | null) =>
    target instanceof HTMLElement && !!target.closest('button, .ga-box');
  const onDragStart = (e: DragEvent) => {
    if (fromControl(e.target)) {
      e.preventDefault();
      return;
    }
    const payload = JSON.stringify(item);
    e.dataTransfer.setData(DRAG_MIME, payload);
    // Respaldo para WebViews que no conservan tipos MIME personalizados.
    e.dataTransfer.setData('text/plain', `vmlab:${payload}`);
    e.dataTransfer.effectAllowed = 'copy';
  };
  return (
    <li className={`pal-item ${m.kind} ${used ? 'used' : ''}`} draggable onDragStart={onDragStart}
      onDoubleClick={(e) => { if (!fromControl(e.target)) addMachine(item); }}
      title="Arrastra al lienzo o haz doble clic">
      <span className="pal-icon">{m.kind === 'vbox' ? <CubeIcon /> : <ContainerIcon />}</span>
      <span className="pal-text">
        <span className="pal-name">{m.label}</span>
        <span className="pal-sub">{m.image ?? m.target.slice(0, 13)}</span>
      </span>
      <span className="status" data-state={m.state} title={STATE_LABEL[m.state]}><i className="dot" /></span>
      {m.kind === 'vbox' && <GuestAdditions m={m} />}
    </li>
  );
}

export function Sidebar() {
  const tools = useLab((s) => s.tools);
  const inventory = useLab((s) => s.inventory);
  const nodes = useLab((s) => s.nodes);
  const refresh = useLab((s) => s.refreshInventory);
  const addMachine = useLab((s) => s.addMachine);
  const toast = useLab((s) => s.toast);
  const dockerStarting = useLab((s) => s.dockerStarting);
  const startDocker = useLab((s) => s.startDocker);
  const overSidebar = useLab((s) => s.overSidebar);
  const [name, setName] = useState('');
  const [image, setImage] = useState('');

  const used = (m: MachineInfo) => nodes.some((n) => n.data.kind === m.kind && n.data.target === m.target);

  const addContainer = () => {
    if (!NAME_RE.test(name)) return toast('error', 'Nombre no válido: letras, números, «_», «.» o «-» (máx. 64)');
    if (!IMAGE_RE.test(image)) return toast('error', 'Imagen no válida (ejemplo: nginx:latest)');
    addMachine({ kind: 'docker', label: name, target: name, image });
    setName('');
    setImage('');
  };

  return (
    <aside className={`sidebar ${overSidebar ? 'drop-active' : ''}`}>
      {overSidebar && <div className="sidebar-drop">Suelta aquí para devolverla al panel</div>}
      <div className="side-section">
        <div className="side-title">
          <span>Herramientas</span>
          <button className="icon-btn sm" onClick={refresh} title="Volver a buscar máquinas"><RefreshIcon /></button>
        </div>
        <div className="pills">
          <span className={`pill ${tools?.vboxmanage ? 'ok' : 'bad'}`}>VirtualBox</span>
          <span className={`pill ${tools?.docker && !inventory?.dockerError ? 'ok' : 'bad'}`}>
            Docker{dockerStarting ? ' · iniciando…' : ''}
          </span>
        </div>
      </div>

      <div className="side-scroll">
        <div className="side-section">
          <div className="side-title"><span>Máquinas virtuales</span></div>
          {inventory?.vboxError && <p className="hint err">{inventory.vboxError}</p>}
          <ul className="pal-list">
            {inventory?.vbox.map((m) => <Item key={m.target} m={m} used={used(m)} />)}
          </ul>
          {inventory && !inventory.vboxError && inventory.vbox.length === 0 && <p className="hint">No hay VMs registradas.</p>}
          <p className="hint">Doble clic o arrastra al lienzo. Arrastra un nodo del lienzo hasta este panel para devolverlo.</p>
        </div>

        <div className="side-section">
          <div className="side-title"><span>Contenedores</span></div>
          {inventory?.dockerError && (
            <div className="docker-alert">
              <p className="hint err">{inventory.dockerError}</p>
              {inventory.dockerEngineOff && (
                <button className="btn primary docker-start" onClick={() => void startDocker()} disabled={dockerStarting}>
                  <PlayIcon /> {dockerStarting ? 'Iniciando Docker…' : 'Iniciar Docker'}
                </button>
              )}
              {dockerStarting && (
                <p className="hint">Arrancando en segundo plano, sin abrir el panel de Docker. La primera vez puede tardar 1–2 minutos.</p>
              )}
              {inventory.dockerErrorDetail && (
                <details className="docker-detail">
                  <summary>Detalles técnicos</summary>
                  <code>{inventory.dockerErrorDetail}</code>
                </details>
              )}
            </div>
          )}
          <ul className="pal-list">
            {inventory?.docker.map((m) => <Item key={m.target} m={m} used={used(m)} />)}
          </ul>
        </div>

        <div className="side-section">
          <div className="side-title"><span>Nuevo contenedor</span></div>
          <div className="form">
            <input placeholder="nombre (p. ej. web-01)" value={name} onChange={(e) => setName(e.target.value)} spellCheck={false} />
            <input placeholder="imagen (p. ej. nginx:latest)" value={image} onChange={(e) => setImage(e.target.value)} spellCheck={false}
              onKeyDown={(e) => e.key === 'Enter' && addContainer()} />
            <button className="btn" onClick={addContainer}><PlusIcon /> Añadir al lienzo</button>
          </div>
          <p className="hint">Se crea con <code>docker run</code> la primera vez que lo inicies.</p>
        </div>
      </div>
    </aside>
  );
}
