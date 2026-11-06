# VMLab

![Licencia MIT](https://img.shields.io/badge/licencia-MIT-green.svg)

Lienzo visual para gestionar laboratorios locales: arrastra máquinas de **VirtualBox** y contenedores **Docker**, conéctalas para definir redes y contrólalas con un clic.

Para añadir una máquina, arrástrala desde el inventario al lienzo o haz doble clic sobre ella. Para devolverla, arrastra su nodo hasta el panel de inventario: se quita del lienzo, pero no se borra la VM ni el contenedor.

> El arrastre inventario → lienzo usa HTML5 *drag and drop*. En Windows, Tauri sustituye el gestor nativo de WebView2, así que la ventana lleva `"dragDropEnabled": false` en `src-tauri/tauri.conf.json`. Sin ese ajuste, el arrastre funciona en el navegador (`npm run dev`) pero no en la app de escritorio.

El botón **Bloquear lienzo** (candado de la barra superior) congela el desplazamiento y el zoom del lienzo, incluido el auto-desplazamiento al arrastrar un nodo. Los nodos siguen moviéndose con normalidad, así que puedes devolverlos al inventario sin que la vista se desplace. El estado se recuerda entre sesiones.

**Stack:** Tauri 2 (Rust) + React 18 + TypeScript + React Flow + Zustand. Sin Chromium embebido: usa el WebView2 de Windows, así que el instalador pesa pocos MB y la app consume poca RAM.

## Estructura

```
vmlab/
├─ crates/vmlab-core/        ← lógica sin UI (testeable con `cargo test`)
│  └─ src/  validate · runner · tools · vbox · docker · topology · engine · model
├─ src-tauri/                 ← envoltorio Tauri: comandos IPC + monitor + migración
│  ├─ src/  commands.rs · monitor.rs · migrate.rs · state.rs · lib.rs
│  ├─ capabilities/default.json   (permisos mínimos de ventana)
│  ├─ tauri.conf.json             (ventana sin marco, CSP, empaquetado NSIS+MSI)
│  └─ icons/                      (generados por `npm run icons`)
├─ src/                       ← interfaz React
│  ├─ components/  TitleBar · Canvas · MachineNodeView · Sidebar · Inspector
│  ├─ store/useLab.ts         (estado global, autoguardado, acciones)
│  └─ lib/  ipc.ts (puente + simulador para navegador) · topology.ts · status.ts · connectivity.ts
├─ scripts/generate-icons.mjs ← PNG/SVG → .ico + .icns + PNGs
└─ assets/icon-source.svg     ← icono base (sustitúyelo por el tuyo)
```

## Cómo funciona el puente IPC

```
React (webview)                         Rust (proceso nativo)
──────────────                          ─────────────────────
invoke('machine_action', {…}) ───────►  commands.rs  (async, no bloquea la UI)
                                           └► vmlab-core::engine
                                                ├─ validate  (rechaza todo lo que no encaje en una lista blanca)
                                                └─ runner    (programa + argv[], SIN shell, con timeout)
listen('vmlab:status')       ◄───────  monitor.rs   (cada 2 s: `VBoxManage list` + `docker ps -a`,
                                                      emite solo si algo cambió)
```

Seguridad:

- **Nunca se usa un shell.** Cada comando es `Command::new(ruta).args([...])`; no existe interpolación de cadenas.
- **Lista blanca de entradas** (`validate.rs`): nombres, imágenes y redes se validan antes de ser argumentos y no pueden empezar por `-` (evita inyección de opciones).
- **Superficie mínima:** la web solo puede invocar 12 comandos con tipos cerrados (`NodeKind`, `Action`, `GuestConfigRequest`…); no puede pedir «ejecuta esto».
- En Windows solo se localizan `.exe` (nunca `.cmd`/`.bat`) y los procesos se lanzan sin ventana de consola.
- Todo proceso tiene **timeout** y se mata si se cancela.

## Modelo de red

Cada **grupo de nodos conectados** es una red local. Al pulsar **Aplicar red**:

| Tecnología | Qué hace VMLab |
|---|---|
| VirtualBox | Conecta el adaptador elegido (2 por defecto) a la red interna `vmlab-xxxxxxxx` (`modifyvm --nicN intnet --intnetN …`). El adaptador 1 (NAT) solo se toca si lo seleccionas expresamente. La VM debe estar apagada. |
| Docker | Crea una red *bridge* `vmlab-xxxxxxxx`, conecta los contenedores y los desconecta de otras redes `vmlab-*` y de las antiguas `lablab-*`. Borra las redes vacías de ambos prefijos. |
| Nodo sin conexiones | VirtualBox: red interna propia o adaptador desactivado. Docker: sin redes `vmlab-*`. |


### Modo de red por VM

El panel derecho permite elegir para cada VM el adaptador (1–4) y su modo: **red interna**, **NAT**, **adaptador puente**, **solo anfitrión**, **red NAT** o **sin conectar**. Los cambios se aplican con **Aplicar red** (o al aplicar la topología). Si un nodo tiene conexiones dibujadas pero usa un modo distinto de red interna, se avisa de que quedará fuera del segmento del lienzo. En modo red interna manda el segmento del lienzo; el nombre propio solo se usa cuando el nodo está aislado.

### Cables del lienzo: comunicación real

El cable entre dos nodos cambia según lo que VMLab consigue observar, no solo según el estado de encendido:

- **Verde y animado**: los dos extremos están en marcha y tienen IPv4 en la misma red de VMLab. Se lee el adaptador gestionado de VirtualBox mediante las *guest properties* (`/VirtualBox/GuestInfo/Net/N/V4/IP`) y la red `vmlab-*` del contenedor con `docker inspect`. No hace falta usuario ni contraseña.
- **Rojo**: todavía no hay enlace. Puede ser porque una máquina está apagada, porque su adaptador está desconectado o en NAT, porque no tiene IP (DHCP/estática sin configurar) o porque la red aún no se ha aplicado.

La comprobación se repite cada 5 segundos y al arrancar, parar o aplicar la red. El panel derecho muestra el mismo estado por segmento («Comunicación detectada» / «Sin comunicación») con el motivo cuando se conoce.

### Arranque silencioso de Docker Desktop

Docker Desktop no ofrece un comando oficial para arrancar solo el motor, a diferencia de `VBoxManage startvm --type headless`. VMLab lo hace desde el aviso del panel **Contenedores**:

1. Escribe `"OpenUIOnStartupDisabled": true` en `%APPDATA%\Docker\settings-store.json` (sin BOM y conservando el resto de ajustes) para que Docker Desktop no abra su panel.
2. Lanza `Docker Desktop.exe` oculto y desacoplado del proceso de VMLab.
3. Espera, como máximo 180 s, a que `docker version` devuelva la versión del servidor.

El botón **Iniciar Docker** solo aparece cuando el motor no responde. Si Docker Desktop ya estaba abierto, no se vuelve a lanzar: únicamente se espera al motor. El paso 1 deja desactivada la opción «Abrir Docker Dashboard al iniciar» de Docker Desktop; puedes volver a activarla desde los ajustes de Docker.

### Configuración dentro de la VM (Guest Additions)

Sin abrir la consola, VMLab puede escribir la configuración de red de una VM Linux con `VBoxManage guestcontrol`:

- Detecta el sistema real: Netplan, NetworkManager, systemd-networkd, ifupdown, wicked, dhcpcd, ConnMan, network-scripts y el servidor ISC `dhcpd` (busca `dhcpd.conf` en `/etc/dhcp/dhcpd.conf` y `/etc/dhcpd.conf`).
- Escribe DHCP, IP estática o modo **servidor DHCP** (`dhcpd.conf`), y recarga el servicio.
- Requiere **Guest Additions** en marcha y un usuario de la VM con root o `sudo` sin contraseña. La contraseña se usa solo durante la operación y se pasa por un fichero temporal del host; nunca se guarda en el lienzo.

### Guest Additions en el inventario

Cada VM del panel izquierdo muestra una etiqueta con la versión de **Guest Additions** o el aviso de que no se detectan. La consulta usa la *guest property* persistente `/VirtualBox/GuestAdd/Version`, así que funciona aunque la VM esté apagada (siempre que las Guest Additions se hayan ejecutado alguna vez). Si VirtualBox no puede consultarla (por ejemplo, porque la VM está bloqueada), se muestra «sin datos» en vez de dar un falso negativo.

Si no están instaladas, el botón **Instalar** monta la ISO `VBoxGuestAdditions.iso` en una unidad óptica libre y, si la VM está apagada, la abre con ventana. El último paso se ejecuta dentro de la VM siguiendo las instrucciones que aparecen en el propio panel: VirtualBox no permite ejecutar nada dentro del guest sin Guest Additions ya presentes (`guestcontrol` depende de ellas). Al terminar, **Ya está instalado · comprobar** vuelve a leer el inventario.

Limitaciones conocidas: una red que mezcla VM y contenedores se avisa pero no se puentea; las VMs en pausa se muestran como «en ejecución» (`list runningvms` no distingue); «Iniciar» arranca las VMs en modo *headless* (usa «Iniciar con ventana» en el panel derecho para ver la consola).

## Requisitos (Windows 10/11)

1. [Node.js 20+](https://nodejs.org)
2. [Rust](https://rustup.rs) (toolchain *stable-msvc*)
3. **Visual Studio Build Tools** con la carga «Desarrollo para el escritorio con C++»
4. WebView2 (ya viene en Windows 11 y en Windows 10 actualizado; el instalador lo descarga si falta)

## Comandos

```powershell
npm install                 # dependencias del frontend (incluye @tauri-apps/cli y sharp)
npm run test:core           # tests Rust de la lógica (validación, parsers, topología, runner)
npm run tauri:dev           # modo desarrollo con recarga en caliente
npm run dist:win            # = npm run icons + tauri build  →  genera los instaladores
```

Los instaladores quedan en:

```
src-tauri\target\release\bundle\nsis\VMLab_0.1.0_x64-setup.exe     ← asistente visual (recomendado)
src-tauri\target\release\bundle\msi\VMLab_0.1.0_x64_es-ES.msi      ← para despliegue empresarial
```

Solo el `.exe`: `npm run tauri build -- --bundles nsis`.

Para desarrollar la interfaz **sin** VirtualBox/Docker, `npm run dev` (navegador, puerto 1420) usa un simulador de backend.

## Icono personalizado

```powershell
npm run icons -- C:\ruta\mi-logo.png            # PNG o SVG, cuadrado, ideal ≥ 1024 px
npm run icons -- mi-logo.svg --padding 8         # 8 % de margen transparente
```

Regenera `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.ico` y `icon.icns` en `src-tauri/icons/`. `dist:win` lo ejecuta antes de compilar, así que basta con reemplazar `assets/icon-source.svg` (o pasar tu imagen) y volver a construir.

## Instalador

Configurado en `src-tauri/tauri.conf.json` → `bundle`:

- **NSIS** (`.exe`): asistente gráfico con selector de idioma (español/inglés), instalación por usuario o para todos (`installMode: both`), icono propio, carpeta «VMLab» en el menú Inicio y desinstalador. Si necesitas que el acceso directo de escritorio sea obligatorio, añade un `installerHooks` (`.nsh`) a `bundle.windows.nsis`.
- **MSI** (WiX): un `.msi` por idioma (es-ES y en-US).
- **WebView2:** `downloadBootstrapper` (instalador pequeño). Para equipos sin internet cambia a `{ "type": "offlineInstaller" }`.
- Para firmar el instalador define `bundle.windows.certificateThumbprint` (sin firma, SmartScreen mostrará un aviso).

El workflow `.github/workflows/build-windows.yml` comprueba tipos y compilación del frontend en cada *push*/PR y construye ambos instaladores en Windows (los guarda como artefacto de la ejecución).

## Licencia

[MIT](LICENSE) © 2026 RvMpElsTiltsKiN

VMLab usa VirtualBox y Docker como programas externos; no los incluye ni los redistribuye. Consulta sus licencias respectivas.
