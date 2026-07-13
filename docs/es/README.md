# BExplorer

BExplorer 1.0 es un explorador de archivos estable y liviano para Windows y
Linux, escrito en Rust. Su objetivo es mejorar la gestion diaria de archivos
sin intentar reemplazar todo el shell del sistema.

El proyecto esta organizado para que la logica especifica de cada sistema
operativo quede separada. Windows y Linux son plataformas soportadas con
integraciones nativas propias; macOS sigue siendo un objetivo experimental.

## Estado

BExplorer 1.0 es la primera version estable para Windows y Linux. La interfaz,
el motor de operaciones, los formatos de configuracion y sesion, los flujos de
comprimidos y las integraciones de plataforma forman la base compatible de la
serie 1.x.

Windows incluye integracion con WPD/MTP, Microsoft Defender, recursos nativos,
red, portapapeles, montaje de imagenes y UAC. Linux incluye navegacion y
operaciones completas, montajes desde `/proc/self/mountinfo`, unidades USB,
red, dispositivos GVfs/FUSE, UDisks2, Polkit, iconos y miniaturas XDG,
portapapeles nativo y soporte Wayland/X11 mediante `iced`/`winit`.

La interfaz ya se ha migrado de `egui` a `iced` y se ha eliminado la
implementacion visual sustituida. Cubre navegacion local, pestanas, panel
dividido, vistas, filtrado, agrupacion y ordenacion por columnas, renombrado,
eliminacion en segundo plano y transferencias en cola. Los cambios de sesion se
guardan al producirse y las carpetas grandes se renderizan incrementalmente en
lotes de 500 elementos, sin ocultar permanentemente los elementos restantes.
La busqueda completa, vistas previas, comprimidos, Defender, MTP, montaje de
imagenes, red y arrastrar y soltar nativo estan conectados a la interfaz.

La interfaz `iced` esta organizada por responsabilidad en `src/iced_ui`:
`mod.rs` coordina la aplicacion, `state.rs` contiene mensajes y estado, y
`update.rs` conserva el despacho exhaustivo de eventos. `interaction/` separa
contexto, seleccion, arrastre y geometria; `view/` separa paneles, menus,
dialogos, tablas y cuadriculas; `file_actions.rs` gestiona operaciones y
transferencias; `advanced.rs` conecta Defender, MTP y unidades; y `helpers/`
agrupa componentes compartidos.

En KDE Plasma/Wayland el difuminado usa el protocolo nativo opcional de KWin.
GNOME/Mutter no publica el efecto interno de GNOME Shell como protocolo para
clientes Wayland, por lo que BExplorer se integra con la extension opcional
Blur My Shell. Al activar Difuminado se registra el identificador `bexplorer`
en la lista de aplicaciones de la extension y al desactivarlo se retira. Si la
extension no esta instalada o habilitada, se conserva un fondo opaco legible.
BExplorer tambien desactiva automaticamente la opacidad dinamica de la
extension para que la ventana enfocada siga realmente difuminada.

La compatibilidad continua se valida especialmente en:

- instalaciones limpias de Windows;
- redes con diferentes permisos y credenciales;
- dispositivos USB, discos externos y celulares MTP;
- carpetas que requieren permisos de administrador;
- archivos comprimidos grandes o protegidos con contrasena;
- escenarios de arrastrar y soltar dentro y fuera de la aplicacion;
- distribuciones Linux con Wayland/X11, portales, montajes de red, USB y
  diferentes implementaciones de portapapeles.

## Funciones Principales

- Navegacion por pestanas con historial independiente.
- Pantalla dividida con vistas independientes.
- Barra lateral redimensionable y reordenable.
- Barra de acciones y barra de marcadores opcionales.
- Vistas de detalles, lista, iconos, iconos grandes, iconos extra grandes y
  mosaicos.
- Soporte para unidades locales, extraibles, ISO montadas, rutas UNC, red,
  montajes Linux, WPD/MTP en Windows y dispositivos GVfs/FUSE en Linux.
- Descubrimiento progresivo de red con cache.
- Copiar, cortar y pegar compatible con el portapapeles de Windows; en Linux se
  usan helpers MIME nativos cuando existen, con fallback de texto.
- Arrastrar y soltar dentro de BExplorer y hacia otras aplicaciones compatibles
  en Windows y Linux.
- Cola de transferencias con progreso, pausa, cancelacion y manejo de
  conflictos.
- Reemplazos locales preparados: primero se copia y sincroniza el archivo o
  directorio completo junto al destino y solo entonces se sustituye el
  elemento anterior. Si la preparacion falla, el destino previo permanece
  intacto.
- Compresiones concurrentes con ventana propia de progreso, que vuelve a primer
  plano al iniciar una transferencia o compresion nueva.
- Acciones elevadas de remediacion y exclusiones de Microsoft Defender.
- Busqueda rapida y busqueda completa, incluyendo archivos dentro de
  comprimidos soportados.
- Panel de vista previa para imagenes, texto, SVG y PDF.
- Integracion con Windows Defender en Windows.
- Personalizacion de tema, color, bordes de iconos, efectos de ventana, atajos
  y distribucion de la interfaz.

## Linux

El objetivo en Linux es no depender de un unico entorno de escritorio. La base
actual usa piezas comunes del sistema:

- `/proc/self/mountinfo` para listar montajes reales;
- sysfs para detectar unidades removibles u opticas cuando esta disponible;
- tipos de filesystem como `cifs`, `smb3`, `nfs`, `sshfs`, `iso9660` y `udf`;
- Freedesktop Icon Theme y Shared MIME Info para iconos de archivos;
- cache XDG de thumbnails antes de generar miniaturas propias;
- portapapeles MIME con `wl-copy`/`wl-paste`, `xclip` o `xsel` cuando existen;
- UDisks2 mediante `udisksctl` para montar/expulsar ISO o unidades;
- Polkit mediante `pkexec` para reintentos elevados;
- `gio`, Samba y Avahi como descubrimiento de red de mejor esfuerzo;
- dispositivos MTP ya montados por GVfs/FUSE bajo `/run/user/.../gvfs`;
- `xdg-terminal-exec` y terminales comunes como fallback;
- `assets/linux/bexplorer.desktop` con `MimeType=inode/directory`.

Diferencias e integraciones opcionales en Linux:

- arrastrar archivos desde BExplorer hacia otros gestores usa helpers nativos
  compatibles con Wayland como `ripdrag`, `dragon-drag-and-drop`, `dragon` o
  `dragon-drop` cuando estan instalados;
- MTP sin montaje GVfs/FUSE todavia no tiene backend propio;
- descubrimiento de red depende de las herramientas disponibles en la distro;
- propiedades nativas siguen pendientes;
- la busqueda de iconos se hace en proceso y todavia puede no cubrir extensiones
  especificas de algunos temas de escritorio.

Integraciones opcionales recomendadas en Linux:

- `wl-clipboard`, `xclip` o `xsel`;
- `ripdrag`, `dragon-drag-and-drop`, `dragon` o `dragon-drop`;
- `udisks2`;
- `polkit`;
- `xdg-utils`, GLib/GVfs;
- `smbclient`, `smbtree` y opcionalmente Avahi;
- GVfs MTP/FUSE para telefonos y camaras.

Comandos recomendados:

```bash
cargo check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo run
```

Los paquetes Linux se generan con `scripts/linux/package.sh`; el tarball, el
paquete `.deb` y sus checksums incluyen los avisos y textos de licencia. El
`.deb` instala el ejecutable en `/usr/bin/bexplorer`, registra la aplicacion en
el menu del escritorio y puede instalarse con `scripts/linux/install-deb.sh`.

En Windows, `scripts/windows/package.ps1` crea un ZIP portable y un instalador
Inno Setup con checksum SHA-256. El instalador permite elegir espanol o ingles,
crea por defecto una entrada en el menu Inicio y ofrece casillas para crear un
acceso directo en el escritorio y agregar BExplorer al `PATH`. Al desinstalar
retira solamente su propia entrada del `PATH`. Los comandos antiguos de
`tools/` se conservan como wrappers compatibles.

La CI comprueba formato, Clippy, pruebas, builds optimizados y empaquetado en
ambas plataformas sin publicar releases automaticamente.

## Comprimidos

BExplorer usa un motor 7-Zip integrado para abrir, listar y extraer muchos
formatos de comprimidos.

Flujos soportados:

- crear archivos ZIP;
- crear archivos 7z;
- crear ZIP y 7z protegidos con contrasena;
- elegir nombre, formato ZIP/7z y nivel rapido, normal o alto desde la barra
  de acciones, o crear ZIP/7z normal con un clic desde el menu contextual;
- ejecutar varias compresiones a la vez, con progreso y cancelacion por tarea;
- extraer ZIP, 7z, RAR, ISO, TAR y otros formatos compatibles con 7-Zip;
- pedir contrasena cuando el comprimido la requiere;
- buscar dentro de comprimidos durante la busqueda completa.

En Windows y Linux el motor 7-Zip se compila desde `vendor/7zip-src` y se enlaza
con `vendor/7zip-ffi`, sin depender de un ejecutable externo `7zr`.

## Licencia

El codigo propio de BExplorer esta bajo licencia MIT.

El motor 7-Zip incluido en `vendor/7zip-src/` mantiene sus propias licencias.
Consulta `THIRD_PARTY_NOTICES.md` y los archivos originales en
`vendor/7zip-src/DOC/`.

## Fiabilidad de datos

La configuracion y la sesion se escriben en archivos temporales hermanos, se
sincronizan y se reemplazan atomicamente. Las copias con conflicto `Reemplazar`
tambien usan una copia preparada y sincronizada antes de modificar el destino.
La suite actual ejecuta 98 pruebas, incluyendo una regresion que fuerza
un fallo durante un reemplazo y verifica que el destino original se conserva.

Los paquetes portables de Windows incluyen checksum, pero pueden mostrar una
advertencia de SmartScreen hasta que exista un instalador firmado. Consulta
`CHANGELOG.md` para el historial de versiones y `SECURITY.md` para reportar
vulnerabilidades de forma privada.
