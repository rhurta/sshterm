# SSH Terminal Manager – Guía de la estructura del proyecto

Documento para aprender cómo se organiza el proyecto y cómo se conectan el frontend y el backend.

> **Nota de vigencia (2026-11-17):** el mapa detallado de archivos y las cifras de pruebas de las secciones históricas se redactaron tras M0–M3 y no representan el árbol actual. La tabla de estado que sigue está actualizada a M6. Para el detalle normativo y las limitaciones usa *SSH Terminal Manager – Tauri Plan & Specs*; para los criterios y pruebas de M6 usa `docs/M6-implementation-plan.md`. La carpeta `docs/` está versionada por Git.

> Estado de M6 en `main`: tiling, consola de comandos, broadcast y persistencia de preferencias implementados; la automatización de pruebas UI y las pruebas de integración SSH/broadcast siguen como trabajo de seguimiento.

## 1. Idea general

La aplicación tiene dos mitades que se comunican entre sí:

- **Frontend (lo que se ve):** Svelte 5 + TypeScript, dentro de una ventana WebView2. Dibuja la pantalla de bloqueo, la barra lateral con la lista de conexiones, el área de trabajo con pestañas y el terminal (xterm.js).
- **Backend (lo que trabaja):** Rust con Tauri 2. Guarda el vault cifrado, abre conexiones SSH mediante `ssh.exe` sobre ConPTY, escribe los logs de sesión, resuelve las rutas portables.
- **Tauri (el puente):** empaqueta ambas mitades en un único `.exe`. El frontend llama a funciones de Rust (**comandos**) con `invoke()`; Rust envía datos al frontend mediante **eventos** (`session:data`, `session:closed`).

```
Ventana (WebView2)                    Proceso Rust
 Svelte + TypeScript  --invoke()-->   funciones #[tauri::command]
                      <--eventos---   session:data, session:closed
```

| Pieza | Lenguaje | Responsabilidad |
| --- | --- | --- |
| Interfaz | Svelte 5 + TypeScript | Mostrar y recoger acciones del usuario; capturar el buffer del terminal para los logs |
| Lógica y seguridad | Rust | Vault cifrado, configuración, rutas, sesiones SSH, logs de sesión |
| Puente | Tauri 2 | Empaquetar, comunicar frontend y backend, permisos |

## 2. Estado actual: qué hay y qué falta

| Hito | Estado | Contenido real |
| --- | --- | --- |
| M0 | ✅ | Esqueleto Tauri que compila a un exe, rutas portables en `paths.rs`, `WEBVIEW2_USER_DATA_FOLDER` apuntando a `webview-data/` |
| M1 | ✅ | Vault cifrado (Argon2id + XChaCha20-Poly1305), crear/bloquear/desbloquear/cambiar contraseña, `config.toml`, i18n inglés/español, 20 pruebas de Rust |
| M2 | ✅ | Sesiones SSH reales vía `ssh.exe` sobre ConPTY, terminal xterm.js, redimensionado, askpass, barra lateral de conexiones, editor de conexiones |
| M3 | ✅ | Un log por sesión; captura desde el buffer de pantalla de xterm.js (no bytes del PTY); sin líneas duplicadas por los redraws de ConPTY |
| M4 | ✅ | Árbol tipado, carpetas, CRUD, búsqueda, importación, drag-and-drop y editor de conexiones |
| M5 | ✅ | ProxyJump anidado y credenciales por salto dentro de los límites del motor OpenSSH |
| M6 | ✅ | Tiling recursivo, paneles con pestañas, consola multilínea, broadcast seguro, preferencias persistidas y UI inglés/español |
| M7 | 🟡 | Reenvíos TCP local/reverse ligados a sesiones interactivas; sin gestor independiente ni SOCKS |
| M8 | 📋 | SFTP nativo con `russh-sftp` |
| M9 | 🟡 | Temas globales implementados; colores por conexión, UI de host keys, reconexión y bloqueo por inactividad pendientes |
| M10 | 📋 | Motor SSH nativo `russh` |

Todavía no existen SFTP, resaltado de sintaxis (M6b), colores por conexión, UI dedicada para gestionar host keys, reconexión automática ni bloqueo por inactividad. El cambio de contraseña del vault existe en backend, pero no hay pantalla para cambiarla desde la UI. El contenido de `vault_save` no se expone como comando frontend.

## 3. Árbol del proyecto (mapa histórico M0–M3; no es el árbol actual)

```
sshterm/
  package.json            dependencias y scripts de Node
  package-lock.json       versiones exactas de Node
  svelte.config.js        SvelteKit con adapter-static (SPA)
  vite.config.js          servidor de desarrollo, puerto 1420
  tsconfig.json           reglas de TypeScript
  .gitignore              excluye node_modules, build, vault, logs y datos generados (docs/ está versionado por Git).
  README.md               visión general, tecnologías, instrucciones de desarrollo
  ROADMAP.md              tabla de hitos con detalle de implementación
  docs/                   guías y especificaciones versionadas por Git
  static/                 archivos estáticos (iconos, imágenes)
  src/                    FRONTEND
    app.html              plantilla HTML base
    routes/
      +layout.ts          ssr = false (aplicación de escritorio, sin servidor)
      +page.svelte         orquestador principal
    lib/
      LockScreen.svelte         pantalla de bloqueo / creación del vault
      LanguageSelect.svelte     selector de idioma
      ConnectionSidebar.svelte  lista de conexiones (barra lateral)
      ConnectionEditor.svelte   formulario crear/editar conexión
      Workspace.svelte          árbol de splits, pestañas por panel, toolbar, connection picker y CommandConsole
      TerminalPane.svelte       terminal xterm.js + FitAddon + LogCapture
      logCapture.ts             captura de log desde el buffer de pantalla
      sessionBus.ts             fan-out de eventos session:data / session:closed
      encoding.ts               helpers base64
      types.ts                  tipos TypeScript compartidos
      i18n/
        en.ts                   catálogo inglés (fuente de verdad de claves)
        es.ts                   catálogo español
        index.svelte.ts         estado del idioma, t(), initLocale(), setLocale()
  src-tauri/              BACKEND
    Cargo.toml            dependencias de Rust y perfil de compilación
    Cargo.lock            versiones exactas de Rust
    build.rs              script de compilación de Tauri
    tauri.conf.json       configuración de la app (identifier, ventana, iconos)
    capabilities/
      default.json        permisos del frontend (core:default, opener:default)
    icons/                iconos del exe
    gen/schemas/          esquemas generados (no editar)
    src/
      main.rs             punto de entrada; detecta modo --askpass
      lib.rs              módulos, WebView2, restauración de ventana y registro de comandos Tauri
      paths.rs            rutas portables desde el directorio del exe
      commands.rs         VaultState, comandos vault_* y config_*
      config.rs           Config: language, theme, logging, terminal settings y command-console preferences
      vault/
        mod.rs            ciclo de vida del Vault, escritura atómica, .bak
        crypto.rs         formato binario, KDF, seal/open
        model.rs          VaultData (JSON descifrado; tree/connections como Value)
        error.rs          VaultError y códigos estables
        tests.rs          15 pruebas de crypto y comportamiento del vault
      store/
        mod.rs            Connection, conn_list/get/upsert/delete; 3 pruebas
      transport/
        mod.rs            trait Transport, SshConfig, PtySize, TransportError
        openssh.rs        OpenSshTransport: build_command, ConPTY, watchdog
      askpass/
        mod.rs            AskpassServer, run_helper, PIPE_ENV, TOKEN_ENV
        acl.rs            ACL del pipe (restringido al usuario actual)
      session/
        mod.rs            session_open/write/resize/close/close_all
                          session_log_append/session_log_close
                          SessionState, log_line (diagnóstico)
      logger/
        mod.rs            SessionLogger, create_file, sanitize, stamp
                          write_text(), write_raw(), finish()
                          6 pruebas unitarias
    target/               resultado de compilar (no editar, no subir a git)
  node_modules/           librerías de Node (generado)
  build/  .svelte-kit/    salida del frontend (generado, ignorado por git)
```

## 4. Qué hace cada archivo

### Raíz del proyecto

| Archivo | Qué hace |
| --- | --- |
| `package.json` | Librerías de Node y scripts (`dev`, `build`, `check`, `tauri`). Svelte 5, SvelteKit 2, Vite 8, xterm.js, @xterm/addon-fit. |
| `svelte.config.js` | `adapter-static` + `fallback: "index.html"`: SPA servida desde disco por Tauri. |
| `vite.config.js` | Puerto 1420 (`strictPort`), ignora `src-tauri/` en el watcher. |
| `README.md` | Visión general del proyecto: objetivos, stack, estructura, instrucciones de desarrollo, arquitectura. |
| `ROADMAP.md` | Tabla de hitos M0-M10 con commits, detalle de implementación y listas de pendientes. |
| `.gitignore` | Excluye `node_modules/`, `build/`, `.svelte-kit/`, `*.vault`, `config.toml`, `webview-data/`, `logs/`, `highlight/`, `known_hosts`. |

### Frontend (`src/`)

| Archivo | Qué hace |
| --- | --- |
| `routes/+layout.ts` | `ssr = false`: aplicación de escritorio, sin renderizado en servidor. |
| `routes/+page.svelte` | Orquestador. Llama a `initLocale()`, muestra `LanguageSelect` más `LockScreen` o el `Workspace` desbloqueado. Usa runas de Svelte 5. |
| `lib/LockScreen.svelte` | Consulta `vault_status` y alterna entre: `missing` (crear), `locked` (desbloquear) y `unlocked`. Limpia campos, desactiva formulario durante operaciones, usa códigos de error traducibles. |
| `lib/LanguageSelect.svelte` | Selector fijo arriba a la derecha; llama a `setLocale()` al cambiar. |
| `lib/ConnectionSidebar.svelte` | Lista de conexiones del vault. Botón para abrir en nueva pestaña, botón para crear nueva conexión. Llama a `conn_list`. |
| `lib/ConnectionEditor.svelte` | Formulario de crear/editar conexión: nombre, host, puerto, usuario, contraseña, keepalive. Llama a `conn_upsert` y `conn_delete`. |
| `lib/Workspace.svelte` | Barra de pestañas y montaje de `TerminalPane`. Gestiona el array de pestañas abiertas, cuál está activa, y el callback `onclosed`. |
| `lib/TerminalPane.svelte` | Pane de terminal individual. Monta xterm.js con canvas renderer, carga FitAddon, suscribe a `session:data` y `session:closed` via `sessionBus`. Redimensiona el PTY con debounce de 80 ms (solo cuando la pestaña está activa). Integra `LogCapture` para enviar el texto de pantalla al log. En el cierre de sesión espera a que xterm procese el último chunk antes de cerrar el log. |
| `lib/logCapture.ts` | `LogCapture`: monitoriza el buffer de xterm.js, rastrea un `IMarker` para saber desde qué línea leer, detecta borrados de pantalla (`ESC[J`, RIS), descarta repaints reconociendo solapamiento con líneas recientes, omite la pantalla alternativa (vim, less), colapsa líneas en blanco a una, y envía lotes cada 250 ms mediante `session_log_append`. Llama a `session_log_close` al terminar. |
| `lib/sessionBus.ts` | Recibe los eventos `session:data` y `session:closed` de Tauri y los distribuye a los suscriptores por `sessionId`. Descodifica base64 → Uint8Array para xterm. |
| `lib/encoding.ts` | `toBase64()` y `fromBase64()`. |
| `lib/types.ts` | Tipos TypeScript compartidos entre componentes. |
| `lib/i18n/en.ts` | Catálogo inglés con `as const`. Fuente de verdad: define `MessageKey`. |
| `lib/i18n/es.ts` | Catálogo español declarado como `Record<MessageKey, string>`: TypeScript avisa si falta o sobra una clave. |
| `lib/i18n/index.svelte.ts` | Runa `$state` con el idioma activo, `t()` (con `{placeholders}` y fallback a inglés), `initLocale()`, `setLocale()`. Añadir idioma = copiar `en.ts`, traducir y dos líneas aquí. |

### Backend (`src-tauri/src/`)

| Archivo | Qué hace |
| --- | --- |
| `main.rs` | Punto de entrada. Llama a `maybe_run_askpass()` antes de crear la ventana: si el proceso es el helper de askpass, ejecuta esa lógica y termina sin abrir WebView2. En release oculta la consola de Windows. |
| `lib.rs` | Declara los 9 módulos Rust. Crea `webview-data/` y fija `WEBVIEW2_USER_DATA_FOLDER` antes de que Tauri cree hilos. Registra `VaultState` y `SessionState` con `.manage()`. Registra 19 comandos en `generate_handler!`: `app_info`, 7 de vault/config, 4 de store, 7 de session. |
| `paths.rs` | `app_dir()` desde `current_exe()`. Funciones: `vault_path()`, `config_path()`, `ssh_program()`, `askpass_program()`, `known_hosts_path()`, `logs_dir()`, `webview_data_dir()`, `highlight_dir()`. `ssh_program()` busca en `bin/` primero y en `System32` después. |
| `commands.rs` | `VaultState(Mutex<Option<Vault>>)`. Comandos: `vault_status`, `vault_create`, `vault_unlock`, `vault_lock`, `vault_change_password`, `config_get_language`, `config_set_language`. Los comandos de vault usan `spawn_blocking` (Argon2 no puede correr en el hilo async). |
| `config.rs` | `Config { language, log_output, log_mode }`. `LogMode::Screen` (default) | `LogMode::Raw`. `load()` tolera archivo ausente o roto; `save()` escribe atómicamente en `.tmp` y renombra. `normalize_language()` valida el código. 7 pruebas: round-trip, defaults, `log_mode` desde archivo, campos viejos ignorados (`strip_special`, `log_input`), archivo roto, campos desconocidos, validación de códigos de idioma. |

### Módulo `vault/`

No depende de Tauri; se prueba con `cargo test` sin ventana.

| Archivo | Qué hace |
| --- | --- |
| `mod.rs` | `struct Vault { path, key: Zeroizing<[u8;32]>, salt, params, data }`. `create`, `unlock`, `save`, `change_password`. `write_atomic()`: `.tmp` → `sync_all()` → `.bak` → rename. `Debug` manual nunca imprime clave ni datos. |
| `crypto.rs` | Formato binario del archivo. `KdfParams` (64 MiB, 3 iter, 1 carril por defecto). `derive_key()` con Argon2id. `encode_header`/`parse_header` (valida firma, versión, parámetros dentro de límites seguros antes de derivar, para que un archivo manipulado no pida RAM absurda). `seal`/`open` con XChaCha20-Poly1305 y la cabecera como AAD. Nonce nuevo en cada guardado. |
| `model.rs` | `VaultData { version, tree, connections, known_hosts }` — JSON cifrado dentro del vault. `tree` y `connections` son `serde_json::Value` hasta M4. |
| `error.rs` | `VaultError` con `code()`: reduce todos los fallos a códigos estables (`wrong_password`, `not_a_vault`, `invalid_params`…). Contraseña incorrecta y archivo alterado devuelven el mismo código a propósito. |
| `tests.rs` | 15 pruebas con parámetros KDF reducidos para ir rápido: round-trip, contraseña errónea, ciphertext y cabecera alterados, parámetros absurdos, truncado, no es vault, archivo ausente, no sobrescribir, contraseña vacía, cambio de contraseña, copia `.bak`, nonce nuevo, ausencia de texto plano. |

### Módulo `store/`

| Archivo | Qué hace |
| --- | --- |
| `mod.rs` | `Connection { id, name, host, port, user, password, keepalive_secs }`. `ConnectionSummary` (sin contraseña, para la barra lateral). `conn_list` (ordenado por nombre), `conn_get`, `conn_upsert`, `conn_delete`. `normalize()` valida y completa los campos. `find()` deserializa un `Value` del vault. 3 pruebas: defaults y strip de contraseña vacía, rechazo de conexiones incompletas, `ConnectionSummary` nunca tiene contraseña. |

### Módulo `transport/`

| Archivo | Qué hace |
| --- | --- |
| `mod.rs` | Trait `Transport` (open/write/resize/close/output). `SshConfig` (program, askpass_program, known_hosts, host, port, user, keepalive_secs). `PtySize { cols, rows }`. `TransportError { code, message }`. |
| `openssh.rs` | `OpenSshTransport`. `build_command()`: construye el comando `ssh.exe` con todos los flags (verificable sin conexión real). `open()`: abre ConPTY, lanza `ssh.exe`, hilo lector (8 KB por iteración, envía chunks por `UnboundedSender`), hilo watchdog (100 ms polling; al detectar salida espera 150 ms, luego hasta 2 s por el hilo lector, luego envía chunk vacío como EOF sentinel). `write()`, `resize()`, `close()`. Implementa `Drop` para matar el proceso huérfano. 5 pruebas: flags SSH correctos, política de host key con askpass activo, contraseña nunca en la línea de comandos, tamaño de PTY, (manual, ignorado) salida de ConPTY. |

### Módulo `askpass/`

| Archivo | Qué hace |
| --- | --- |
| `mod.rs` | `AskpassServer`: crea un pipe con nombre aleatorio, arranca un hilo que espera una conexión, valida el token de un solo uso, devuelve la contraseña almacenada. Constantes `PIPE_ENV` y `TOKEN_ENV`. `run_helper()`: el exe en modo askpass conecta al pipe, envía el token, recibe la contraseña e imprime a stdout. |
| `acl.rs` | Restringe el pipe al usuario actual mediante DACL de Windows (solo en Windows). |

### Módulo `session/`

| Archivo | Qué hace |
| --- | --- |
| `mod.rs` | `SessionState { transports: HashMap<id, Transport>, loggers: HashMap<id, Logger> }`. `session_open`: lee credenciales del vault, crea `AskpassServer`, abre el transport, crea el logger, lanza la tarea async que reenvía chunks como `session:data` y llama a `logger.write_raw()` en modo Raw. `session_write`, `session_resize`, `session_close`, `session_close_all`. `session_log_append` / `session_log_close`: comandos llamados por `LogCapture.ts` en el frontend para escribir texto de pantalla y cerrar el log. `log_line()`: diagnóstico a `logs/sshterm.log`, nunca escribe contenido del terminal ni credenciales. |

### Módulo `logger/`

| Archivo | Qué hace |
| --- | --- |
| `mod.rs` | `SessionLogger { file, mode: LogMode }`. `open()` / `open_in()` (versión de test con directorio configurable). `write_text()`: solo en modo Screen, escribe texto recibido del frontend. `write_raw()`: solo en modo Raw, escribe bytes del PTY. `finish()`: escribe línea de cierre con timestamp RFC 3339 y suelta el archivo. `create_file()`: nombre `sanitize(nombre)_stamp(now).log`; si ya existe añade `_2`, `_3`… (`create_new`). `sanitize()`: reemplaza caracteres ilegales en Windows, recorta punto y espacio finales, trunca a 80 caracteres. `stamp()`: formato `YYYYMMDD-HHMMSS`. 6 pruebas: sanitize, nombre de archivo con timestamp legible, modo Screen vs Raw, `log_output=false` no crea archivo, dos sesiones simultáneas no comparten archivo. |

## 5. Ejemplo: cómo viaja una llamada

### Abrir una sesión SSH

1. El usuario hace clic en una conexión en `ConnectionSidebar`.
2. `Workspace.svelte` llama a `invoke("session_open", { conn_id, cols, rows })`.
3. `session::session_open` (Rust) lee las credenciales del vault, crea un `AskpassServer` (si hay contraseña), abre el `OpenSshTransport` (que lanza `ssh.exe` sobre ConPTY), crea el `SessionLogger` y devuelve un `session_id`.
4. `Workspace` recibe el `session_id`, crea una nueva pestaña y monta un `TerminalPane`.
5. `TerminalPane` se suscribe a `session:data` y `session:closed` via `sessionBus`. Cuando llega un chunk base64, `sessionBus` lo decodifica y lo pasa a `term.write()`.
6. `TerminalPane` instancia `LogCapture`, que registra listeners en xterm para `onWriteParsed` y los handlers de borrado de pantalla.
7. A medida que el usuario ve texto en el terminal, `LogCapture` detecta líneas completas (el cursor avanzó más allá de ellas) y las envía por lotes a `session_log_append`. El backend escribe el texto en el archivo de log.
8. Al cerrar la pestaña (clic en `×` o `session:closed`): `TerminalPane` llama a `LogCapture.finish()` (que envía las últimas líneas), luego a `session_log_close` (que escribe la línea de cierre y cierra el archivo), y finalmente a `session_close`.

### Desbloquear el vault

1. `LockScreen.svelte` llama a `invoke("vault_status")` al arrancar.
2. Al enviar el formulario: `invoke("vault_unlock", { password })`.
3. Tauri ejecuta `commands::vault_unlock`: lanza `Vault::unlock` en `spawn_blocking` (Argon2 con 64 MiB no puede correr en el hilo async del runtime de Tauri).
4. `crypto::parse_header` valida la cabecera, `derive_key` obtiene la clave, `open` descifra y autentica, `serde_json` reconstruye `VaultData`.
5. Si falla: `VaultError::code()` produce, por ejemplo, `wrong_password`. El frontend lo convierte a la clave `error.wrong_password` y `t()` lo traduce.
6. Si tiene éxito: el `Vault` queda en `VaultState`. La pantalla cambia al workspace.
7. **Bloquear** (`vault_lock`): pone `None` en `VaultState`. El `Vault` se destruye y `Zeroizing` borra la clave de RAM.

## 6. El vault por dentro

Disposición del archivo `sshterm.vault`:

| Bytes | Contenido |
| --- | --- |
| 0–3 | Firma `SSHV` |
| 4 | Versión del formato (1) |
| 5–8 | Argon2: memoria en KiB (u32 LE) |
| 9–12 | Argon2: iteraciones (u32 LE) |
| 13–16 | Argon2: carriles (u32 LE) |
| 17–32 | Sal aleatoria (16 bytes) |
| 33–56 | Nonce aleatorio (24 bytes), nuevo en cada guardado |
| 57… | JSON cifrado con XChaCha20-Poly1305 + etiqueta 16 bytes |

Los primeros 33 bytes (firma + versión + parámetros KDF) son **AAD**: modificar la sal o los parámetros hace que el descifrado falle. Los parámetros se validan dentro de límites seguros antes de usarlos para evitar que un archivo manipulado consuma RAM absurda.

## 7. Los logs de sesión por dentro

**Por qué no se pueden loguear los bytes del PTY directamente:** ConPTY no reenvía los bytes brutos del servidor remoto. Mantiene su propia copia de la pantalla y envía redraws: repaints completos al redimensionar o hacer scroll, actualizaciones parciales con secuencias de posicionamiento de cursor. Si se quitan los códigos de escape de ese flujo se pierde el contexto de movimiento de cursor que explica los redraws, produciendo líneas duplicadas (el MOTD repetido en el log del bug que motivó el fix de M3), fragmentos de texto y números sueltos de los parámetros de las secuencias de escape.

**La solución:** xterm.js aplica correctamente todos los redraws; su buffer siempre tiene cada línea exactamente una vez. `LogCapture.ts` lee ese buffer.

```
Bytes del PTY (redraws) ──▶ xterm.js ──▶ buffer (cada línea una vez)
                                              │
                                        LogCapture.ts
                                        • IMarker apunta a la siguiente línea
                                          no escrita (sobrevive scroll/reflow)
                                        • al detectar ESC[J o RIS: vacía
                                          lo pendiente y recuerda el anchor
                                        • al perder el marker: salta las
                                          líneas que solapan con las recientes
                                        • pantalla alternativa: no loguea
                                        • líneas en blanco: máximo una seguida
                                        • batch de 250 ms
                                              │
                                    session_log_append (IPC)
                                              │
                                    SessionLogger.write_text()
                                    — solo en modo Screen
                                              │
                              logs/<conexión>_<YYYYMMDD-HHMMSS>.log
```

Cabecera del archivo:
```
sshterm session log
connection: media server
user: olfo
host: 192.168.1.2
started: 2026-11-15T01:38:19+00:00
----
```

Pie del archivo (escrito por `finish()` via `session_log_close`):
```
---- ended: 2026-11-15T02:10:44+00:00
```

## 8. Archivos que la aplicación crea junto al exe

| Archivo o carpeta | Contenido | Aparece en |
| --- | --- | --- |
| `webview-data/` | Caché de WebView2 | M0 |
| `sshterm.vault` | Conexiones, contraseñas: cifrados | M1 |
| `sshterm.vault.bak` | Copia del vault anterior (desde el 2.º guardado) | M1 |
| `sshterm.vault.tmp` | Temporal durante escritura atómica; desaparece al renombrar | M1 (transitorio) |
| `config.toml` | `language`, `log_output`, `log_mode` | M1 |
| `known_hosts` | Claves de host para `ssh.exe` | M2 |
| `logs/sshterm.log` | Diagnósticos de la app (nunca contenido del terminal) | M2 |
| `logs/<conexión>_<YYYYMMDD-HHMMSS>.log` | Log de sesión | M3 |
| `highlight/` | Reglas de resaltado de sintaxis (`*.toml`) | M6b |

## 9. Estructura prevista del backend (hitos futuros)

```
src-tauri/src/
  main.rs  lib.rs  paths.rs  commands.rs  config.rs   M0–M1 ✅
  vault/        M1  vault cifrado                              ✅
  store/        M2  CRUD de conexiones (lista plana → árbol M4) ✅
  transport/    M2  trait Transport + openssh.rs               ✅
  askpass/      M2  entrega de contraseñas a ssh.exe           ✅
  session/      M2–M3  sesiones + bridge de logs               ✅
  logger/       M3  archivos de log de sesión                  ✅
  broadcast/    M6  enviar comandos a varias sesiones          📋
  highlight/    M6b carga y validación de reglas               📋
  tunnel/       M7  túneles -L, -R, -D                        📋
  sftp/         M8  explorador y descargas con russh-sftp      📋
  hostkeys/     M9  confianza en claves de servidor            📋
  transport/russh.rs  M10 motor nativo russh                   📋
```

## 10. Componentes previstos del frontend

| Componente | Función | Estado |
| --- | --- | --- |
| `LockScreen.svelte` | Contraseña maestra, crear vault | ✅ M1 |
| `LanguageSelect.svelte` | Elegir idioma | ✅ M1 |
| `ConnectionSidebar.svelte` | Lista de conexiones, abrir sesión | ✅ M2 |
| `ConnectionEditor.svelte` | Crear/editar conexión | ✅ M2 |
| `Workspace.svelte` | Pestañas y montaje de panes | ✅ M2–M3 |
| `TerminalPane.svelte` | xterm.js + LogCapture | ✅ M2–M3 |
| Barra lateral con árbol | Carpetas, drag-and-drop, búsqueda, importación | 📋 M4 |
| Gestor de túneles | Estado de túneles activos | 📋 M7 |
| Panel SFTP | Lista remota, cola de descargas | 📋 M8 |
| Barra de broadcast | Comando para varias sesiones | 📋 M6 |
| Panel de prueba de resaltado | Pegar texto, ver clasificación y colores | 📋 M6b |

## 11. Comandos Tauri disponibles (tras M3)

| Comando | Módulo | Descripción |
| --- | --- | --- |
| `app_info` | `lib.rs` | Ruta del directorio de la app |
| `vault_status` | `commands` | `"missing"` / `"locked"` / `"unlocked"` |
| `vault_create` | `commands` | Crear vault con contraseña maestra |
| `vault_unlock` | `commands` | Desbloquear vault |
| `vault_lock` | `commands` | Bloquear vault (borra clave de RAM) |
| `vault_change_password` | `commands` | Cambiar contraseña (re-cifra todo) |
| `config_get_language` | `commands` | Leer idioma de `config.toml` |
| `config_set_language` | `commands` | Guardar idioma en `config.toml` |
| `conn_list` | `store` | Lista de conexiones (sin contraseñas) |
| `conn_get` | `store` | Leer una conexión (con contraseña) |
| `conn_upsert` | `store` | Crear o actualizar conexión |
| `conn_delete` | `store` | Eliminar conexión |
| `session_open` | `session` | Abrir sesión SSH, devuelve `session_id` |
| `session_write` | `session` | Enviar input al PTY |
| `session_resize` | `session` | Redimensionar el PTY |
| `session_close` | `session` | Cerrar una sesión |
| `session_close_all` | `session` | Cerrar todas las sesiones (al bloquear) |
| `session_log_append` | `session` | Escribir texto de pantalla en el log (desde `LogCapture`) |
| `session_log_close` | `session` | Escribir pie del log y cerrar el archivo |

## 12. Comandos de uso diario

| Comando | Para qué |
| --- | --- |
| `npm install` | Descargar librerías de Node |
| `npm run tauri dev` | Ejecutar en desarrollo con recarga automática |
| `npm run check` | Comprobar TypeScript y Svelte (debe dar 0 errores) |
| `cargo check` (en `src-tauri/`) | Comprobar errores de Rust rápidamente |
| `cargo test` (en `src-tauri/`) | 42 pruebas: vault (15), config (7), store (3), transport (5), logger (6), misc (6) |
| `npm run tauri build` | Compilar exe + instaladores MSI y NSIS |
| `git log --oneline` | Ver historial de hitos |

## 13. Detalles conocidos y pendientes

- `vault_save` no está expuesto como comando Tauri (hará falta en M4 para guardar conexiones editadas desde la UI de árbol).
- `vault_change_password` está implementado y probado pero la UI aún no ofrece botón para usarlo.
- El bloqueo automático por inactividad es de M9.
- `tree` y `connections` son `serde_json::Value` hasta M4.
- La barra lateral es una lista plana; los árboles de carpetas llegan en M4.
- `tauri-plugin-opener` está en `Cargo.toml` y `capabilities/default.json` pero no se usa en el código; se puede quitar al limpiar la plantilla.
- `docs/` está en `.gitignore`: son documentos de trabajo local. El historial de git comienza en `bd4d6b4` (m1).

## 14. Glosario

- **IPC:** comunicación entre procesos; aquí, entre la ventana y Rust.
- **WebView2:** motor de Windows (Edge) que dibuja la interfaz.
- **ConPTY:** pseudo-terminal de Windows; permite ejecutar `ssh.exe` de forma interactiva manteniendo el control del terminal.
- **Redraw / repaint:** ConPTY envía actualizaciones de pantalla, no bytes brutos del servidor. xterm.js los interpreta; el log los lee del buffer resultante.
- **IMarker:** referencia a una línea del buffer de xterm.js que sobrevive al scroll y al reflow.
- **PTY:** pseudo-terminal.
- **LogCapture:** clase TypeScript que monitoriza el buffer de xterm para construir el log sin duplicados.
- **Jump host:** servidor intermedio por el que se pasa para llegar a otro.
- **SFTP / SCP:** transferencia de archivos sobre SSH.
- **KDF:** función que deriva una clave desde una contraseña (aquí, Argon2id).
- **AEAD / AAD:** cifrado autenticado; los datos asociados (cabecera del vault) no se cifran pero sí se verifican.
- **TOFU:** confiar en la clave de un servidor la primera vez y avisar si cambia.
- **Runa:** primitiva reactiva de Svelte 5 (`$state`, `$props`, `$effect`).
- **Escritura atómica:** `.tmp` → `sync_all()` → rename. Protege el vault si hay un corte de luz.

## 15. Qué sigue

El hito **M4** añade el árbol de sesiones con carpetas, búsqueda, importación desde `~/.ssh/config`, PuTTY y WinSSHTerm, el editor de conexión completo (con todos los campos del modelo del vault), y la barra de trabajo multi-pestaña formal. También extiende el vault con los tipos completos de `Connection` (colores, `protected`, cadena de saltos, túneles, SFTP). Esta guía se ampliará con cada hito.
