# Revisión de sshterm hasta M3 y causa de los logs duplicados

> **Informe histórico de revisión M3.** Las tablas de hallazgos y estados describen el estado de aquella revisión; no son una lista vigente de errores. Para el estado actual, consulta la especificación principal y `ROADMAP.md`.

## 1. Resumen

- **Causa de los duplicados:** el log se construye con los bytes que salen de ConPTY, y ConPTY no reenvía lo que escribe el servidor: reenvía **redibujados de su pantalla**. Al quitar los códigos de escape, cada redibujado queda como texto repetido o como fragmentos sueltos. No se puede arreglar limpiando mejor ese flujo.
- **Solución:** registrar lo que **muestra el terminal** (el buffer de xterm.js), no los bytes del PTY. Está en el parche `m3-logfix.patch`.
- **Qué revisé:** `logger`, `session`, `transport/openssh.rs`, `askpass/mod.rs`, `store`, `config`, `paths`, `lib.rs`, `TerminalPane`, `sessionBus` y el flujo de abrir y cerrar sesiones de `Workspace`.
- **Qué ejecuté:** las pruebas de Rust del repositorio en Linux (37 pasan, más 1 nueva del parche), `svelte-check` (0 errores) y un experimento comparativo (sección 3).
- **Qué no pude hacer:** probar nada en Windows ni con ConPTY real, y no revisé a fondo `askpass/acl.rs` (solo Windows), `ConnectionEditor`, `ConnectionSidebar` ni `LockScreen`.

## 2. Por qué el log tiene salida duplicada

### Evidencias en `server_1794688023.log`

| Observación | Qué significa |
| --- | --- |
| El banner completo sale una vez; después se repite unas 97 veces un bloque de unas 21 líneas que empieza a mitad del banner | 21 es la altura visible del terminal: cada bloque es un repintado de la pantalla |
| Fragmentos sueltos como `40`, `d.` o `.com/esm` en medio del texto | ConPTY actualiza solo las celdas que cambian, con saltos de cursor; sin los saltos queda el trozo suelto |
| Líneas con muchos espacios al final | Borrados de línea y relleno de ConPTY |
| El listado de `docker ps -a` aparece repetido al final | Repintado tras el desplazamiento de pantalla |
| El archivo no tiene ningún byte ESC ni CR | La limpieza ya se aplicó: el orden original no se puede reconstruir |

### Mecanismo

1. **ConPTY mantiene su propia copia de la pantalla** y envía redibujados: repintados completos al cambiar de tamaño o desplazar, y actualizaciones parciales con saltos de cursor.
2. `strip_ansi_sequences` elimina justo los códigos que dicen "esto sobrescribe lo anterior". Lo que queda es texto repetido.
3. El limpiador tiene dos fallos más:
   - No guarda estado entre lecturas (de hasta 8 KB). Una secuencia partida entre dos lecturas deja restos o se pierde.
   - Las secuencias OSC solo terminan con BEL. El comentario dice que también con ST (`ESC \`), pero el código no lo hace: con ST descarta texto hasta el siguiente BEL.
4. **Hay muchos repintados** por dos motivos en `TerminalPane`:
   - `resize()` llama a `session_resize` en cada evento del `ResizeObserver`, aunque columnas y filas no hayan cambiado.
   - `Workspace` abre cada sesión a 80x24 y el panel la redimensiona al montarse.
5. El commit `M3 fix: collapse runs of blank lines` solo oculta el síntoma.

No puedo saber sin una traza cuántos de los repintados vienen del redimensionado y cuántos del desplazamiento. El parche trata ambos: evita repintados innecesarios y hace que los que queden no lleguen al log.

## 3. Prueba comparativa

Generé un flujo **sintético** con el estilo de ConPTY (banner de 22 líneas, 12 repintados, una actualización parcial de celda, un comando con 30 líneas de salida y 6 repintados más), lo troceé al azar entre 200 y 8192 bytes y comparé ambos métodos con lo que realmente imprimió el servidor.

|  | Limpiador actual (código del repositorio) | Nuevo (buffer de xterm) |
| --- | --- | --- |
| Líneas del log | 577 | 56 |
| Líneas distintas que se repiten | 48 | 0 |
| Máximo de repeticiones de una línea | 17 | 1 |
| Igual a la salida del servidor | No | Sí |

Además pasan 11 casos límite: línea larga que se parte en filas, barra de progreso con `\r`, `vim` (pantalla alternativa), `clear`, repintado completo con borrado de pantalla, cambio de tamaño con reflujo, banner más largo que la pantalla en una sola escritura, desbordamiento del scrollback, UTF-8 y línea final sin terminar.

**Limitación:** no es una captura real de ConPTY. Por eso conservo el modo `raw` (sección 6) para diagnosticar.

## 4. Qué cambia el parche

| Archivo | Cambio |
| --- | --- |
| `src/lib/logCapture.ts` (nuevo) | Genera el log a partir del buffer de xterm. Escribe una línea solo cuando el cursor ya bajó de ella; une las líneas partidas; ignora `vim`, `top` y similares; sobrevive a `clear` y a los borrados de pantalla; no escribe líneas en blanco consecutivas |
| `src/lib/TerminalPane.svelte` | Crea el `LogCapture`; solo avisa del tamaño al PTY cuando cambia (con espera de 80 ms); cierra el log al terminar la sesión y al destruir el panel |
| `src-tauri/src/logger/mod.rs` | Nombre `<conexión>_<AAAAMMDD-HHMMSS>.log` en hora local (antes: segundos Unix); `create_new` con sufijo `_2`, `_3` si dos sesiones empiezan en el mismo segundo; respeta `log_output`; modo `raw` sin limpiador; pie `---- ended: ...`; elimina el limpiador de ANSI |
| `src-tauri/src/config.rs` | Nuevo `log_mode` (`screen` por defecto, o `raw`); se eliminan `log_input` y `strip_special`; los archivos antiguos con esos campos se siguen leyendo |
| `src-tauri/src/session/mod.rs` | Comandos `session_log_append` y `session_log_close`; el logger se crea solo después de que ssh arranque; ya no se registra la entrada tecleada |
| `src-tauri/src/transport/openssh.rs` | Corrige la carrera del fin de sesión (ver sección 5, punto 2) |
| `src-tauri/Cargo.toml` | Añade `chrono` para la hora local |

## 5. Otros hallazgos de la revisión

| # | Prioridad | Hallazgo | Estado |
| --- | --- | --- | --- |
| 1 | Alta | Causa de los logs duplicados (secciones 2 y 3) | Corregido en el parche |
| 2 | Alta | **Se puede perder el final de la salida.** Al terminar ssh, el vigía suelta el PTY y envía el marcador de fin de inmediato, sin esperar a que el lector termine. El receptor se detiene en el marcador y descarta lo que quede (por ejemplo `logout` o un mensaje de error) | Corregido: espera 150 ms, suelta el PTY, espera al lector (máximo 2 s) y entonces envía el marcador |
| 3 | Alta | **La pestaña desaparece al terminar ssh** (`subscribeClosed` llama a `onclosed`, que ejecuta `dropTab`). Mensajes como `Permission denied` o el aviso de clave de servidor cambiada se pierden al instante | Pendiente: dejar la pestaña con un aviso "sesión terminada" |
| 4 | Alta | La sesión se abre a 80x24 antes de que exista el panel y se redimensiona al montarlo: repintado inmediato y banner mal ajustado. Es también la razón de ser de `sessionBus` y su búfer | Pendiente: abrir la sesión desde `TerminalPane` después de `fit()` |
| 5 | Media | La opción `log_output` no tenía efecto: el logger se creaba siempre | Corregido |
| 6 | Media | `log_input` escribía secuencias ANSI en un log "limpio" y podía guardar contraseñas tecleadas en un prompt. Es redundante: la orden ya aparece en la línea del prompt | Corregido: se elimina |
| 7 | Media | Si ssh no arrancaba, quedaba un archivo de log vacío y una entrada en `loggers` | Corregido |
| 8 | Media | **Inyección de opciones:** `normalize` solo comprueba que host y usuario no estén vacíos y `build_command` pasa `usuario@host` sin `--`. Un valor que empiece por `-` (por ejemplo desde una importación futura) se interpretaría como opción de ssh, como `-oProxyCommand=...` | Pendiente: validar caracteres y usar `--` |
| 9 | Media | `normalize` convierte `keepalive_secs = 0` en 30, así que no se puede desactivar el keep alive (la especificación dice 0 = desactivado) | Pendiente |
| 10 | Media | `UserKnownHostsFile=` usa `display()`: una carpeta con espacios o `\` rompe la opción. Usar barras normales y comillas cuando haya espacios | Pendiente |
| 11 | Media | El servidor askpass **ignora el texto de la pregunta**. El ayudante solo descarta las preguntas de clave de servidor, así que cualquier otra pregunta (código 2FA, passphrase) recibe la contraseña y consume el token de un solo uso | Pendiente (el campo `prompt` ya está preparado para M5) |
| 12 | Media | Los `invoke("session_write")` son independientes: al teclear rápido o pegar pueden llegar desordenados. Conviene encadenarlos por sesión | Pendiente |
| 13 | Baja | `req.token == token` no es de tiempo constante. Riesgo bajo porque la tubería tiene ACL de usuario | Pendiente |
| 14 | Baja | `Cargo.toml.dependencias.txt` quedó en el repositorio | Pendiente: borrarlo |
| 15 | Baja | Solo hay autenticación por contraseña; la clave y el agente de la especificación no están | Aplazado |

Lo que está bien: el nombre de la tubería con ACL solo para el usuario actual y el token de un solo uso, `StrictHostKeyChecking=accept-new` con la explicación del fallo con askpass forzado, las pruebas de regresión de los argumentos de ssh, el búfer de `sessionBus` para la salida previa al montaje, y `.gitignore` (vault, logs, `known_hosts`, `config.toml`).

## 6. Cómo aplicar y verificar

1. Desde la raíz del proyecto, en una rama nueva: `git switch -c m3-logfix` y `git apply m3-logfix.patch`. Se aplica sin conflictos sobre `main`.
2. `npm run check` y, dentro de `src-tauri`, `cargo test`.
3. `npm run tauri dev`. Conéctate a un servidor y prueba:
   - Redimensiona la ventana arrastrando el borde.
   - Ejecuta un comando con mucha salida (`docker ps -a`, `ls -la /usr/bin`).
   - Ejecuta `clear`.
   - Abre y cierra `vim` o `top`.
   - Escribe `exit`.
4. Abre `logs/<conexión>_<fecha>.log`. Cada línea debe aparecer una sola vez y debe estar la línea final `logout`.
5. **Si aun así ves problemas:** pon `log_mode = "raw"` en `config.toml`, repite la prueba y envíame ese log. Con los bytes reales puedo ajustar el algoritmo.

## 7. Limitaciones del nuevo log

- Si el WebView se cierra de golpe, se pierde lo ocurrido en los últimos 250 ms.
- Si una sola escritura supera el scrollback (5000 líneas), se pierden las líneas desplazadas fuera antes de registrarlas. Una lectura del PTY es de 8 KB como máximo, así que no debería ocurrir.
- Los programas de pantalla completa (`vim`, `top`, `less`) no se registran.
- Si una línea larga aparece partida en varias líneas del log, prueba a añadir `windowsPty: { backend: "conpty" }` a las opciones de `Terminal`. No lo he podido probar.
- Una barra de progreso con `\r` queda registrada solo con su estado final.

## 8. Siguientes pasos recomendados

1. Puntos 3 y 4 de la sección 5: conservar la pestaña al terminar y abrir la sesión con el tamaño real.
2. Punto 8: validar host y usuario y usar `--`, antes de implementar la importación del M4.
3. Punto 11: que el servidor askpass use el texto de la pregunta (como en el diseño original del M5).


## 8. Seguimiento frente al código actual (2026-11-17)

Esta sección actualiza solo los hallazgos que siguen siendo relevantes; no altera los resultados históricos de las pruebas anteriores.

| Hallazgo de la revisión M3 | Estado actual |
| --- | --- |
| La pestaña desaparecía al terminar SSH | Se añadió la opción `keep_disconnected_sessions` en Settings. Si se activa, la pestaña conserva la pantalla final, queda marcada como desconectada y no acepta entrada ni broadcast. El valor predeterminado sigue siendo desactivado. |
| Apertura provisional a 80×24 antes del montaje del terminal | Se conserva como tamaño provisional. `TerminalPane` ajusta el PTY al tamaño real al montarse y solo envía un resize cuando cambian filas o columnas; el `sessionBus` retiene la salida temprana. |
| `keepalive_secs = 0` se normaliza a 30 | Sigue siendo el comportamiento actual: cero significa usar el valor predeterminado de 30 segundos, no desactivar keepalive. La UI acepta valores de 1 a 3600 segundos. |
| Entrega de credenciales askpass | El ayudante asigna secretos a prompts identificables de usuario/host o ruta de clave y falla de forma cerrada ante prompts genéricos/ambiguos. No se debe asumir compatibilidad con cualquier prompt 2FA o keyboard-interactive. |
| Orden de escrituras y broadcast a varias sesiones | El broadcast de M6 envía secuencialmente a los PTY y no reintenta. Esto no convierte los envíos en una transacción ni confirma ejecución remota. |
