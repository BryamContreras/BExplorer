# BExplorer

BExplorer 1.0.2 es un explorador de archivos estable y liviano para Windows y
Linux, escrito en Rust. Su objetivo es mejorar la gestion diaria de archivos
sin intentar reemplazar todo el shell del sistema.

El proyecto esta organizado para que la logica especifica de cada sistema
operativo quede separada. Windows y Linux son plataformas soportadas con
integraciones nativas propias; macOS sigue siendo un objetivo experimental.

## Estado

BExplorer 1.0.2 es la version estable actual para Windows y Linux. La interfaz,
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
imagenes, red, arrastrar y soltar nativo, enlaces simbolicos, propiedades y
seleccion de aplicaciones estan conectados a la interfaz.

En Linux se combinan GVfs, Samba y Avahi para descubrir recursos de red. En
KDE, los lugares guardados, montajes de KIOFuse y consultas acotadas mediante
`kioclient` enriquecen ese resultado cuando estan disponibles, sin sustituir
los mecanismos comunes ni convertir KDE en una dependencia obligatoria. El
selector completo de aplicaciones utiliza el portal XDG y el paquete `.deb`
declara como dependencias los servicios necesarios para las integraciones
generales de Linux.

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
- Integracion Freedesktop para iniciar `bexplorer %f`: abre la carpeta recibida
  o, si otra aplicacion entrega un archivo, navega a su carpeta contenedora.
- Vistas de detalles, lista, iconos, iconos grandes, iconos extra grandes y
  mosaicos.
- Soporte para unidades locales, extraibles, ISO montadas, rutas UNC, red,
  montajes Linux, WPD/MTP en Windows y dispositivos GVfs/FUSE en Linux.
- Formateo de discos que no sean del sistema con elevacion nativa en Windows y
  UDisks2/Polkit en Linux. Linux permite discos externos y discos locales
  secundarios, pero bloquea el disco fisico del sistema, firmware, imagenes
  loop, RAID y almacenamiento por capas antes de desmontar y formatear.
- Descubrimiento progresivo de red con cache.
- Copiar, cortar y pegar compatible con el portapapeles de Windows; en Linux se
  usan helpers MIME nativos cuando existen, con fallback de texto.
- Arrastrar y soltar dentro de BExplorer y hacia otras aplicaciones compatibles
  en Windows y Linux.
- Cola de transferencias con progreso, pausa, cancelacion y manejo de
  conflictos.
- Deshacer de un nivel para la ultima copia, movimiento o envio a la papelera
  completado.
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
- Enlaces simbolicos reconocidos por destino: los enlaces a carpetas se
  navegan como carpetas, los de archivos se abren como archivos y los rotos se
  mantienen visibles e identificables.
- Hoja de propiedades nativa de Windows y ventana compacta propia en Linux con
  pestanas General, Permisos y Detalles.
- Menus **Abrir con** con nombres e iconos de aplicaciones; en Linux, **Elegir
  otra aplicacion** usa el portal XDG.
- Navegacion por teclado en menus flotantes y selectores: al escribir una letra
  se selecciona la siguiente opcion que comienza por ella, igual que en la
  lista de archivos.
- Integracion con Windows Defender en Windows.
- Personalizacion de tema, color, bordes de iconos, efectos de ventana, atajos
  y distribucion de la interfaz.

## Enlaces simbolicos, propiedades y aplicaciones

En Linux, BExplorer clasifica cada enlace simbolico sin perder la identidad del
propio enlace. Un enlace valido a una carpeta se puede recorrer, uno dirigido a
un archivo se abre segun el tipo del destino y uno roto muestra un error
explicito. Propiedades presenta tanto el destino almacenado como el resuelto y
no aplica silenciosamente al destino cambios de permisos solicitados sobre el
enlace.

Windows conserva la hoja de propiedades nativa del Shell. En Linux, la ventana
de propiedades de BExplorer admite:

- uno o varios archivos y directorios locales;
- renombrado, tamano logico y en disco, fechas, tipo MIME, punto de montaje,
  sistema de archivos, dispositivo, espacio libre, UID/GID, inodo y cantidad
  de enlaces fisicos;
- calculo del tamano de carpetas en segundo plano;
- seleccion de propietario y grupo desde las identidades del sistema;
- permisos de lectura, escritura y ejecucion para propietario, grupo y otros;
- bits setuid, setgid y sticky, con aplicacion recursiva opcional;
- cambios elevados de permisos y propiedad mediante Polkit cuando sea
  necesario;
- aplicaciones instaladas con su nombre e icono, y cambio de la aplicacion
  predeterminada para un tipo MIME mediante `xdg-mime`.

La accion **Elegir otra aplicacion** de Linux llama a `OpenFile` del portal XDG
con un descriptor del archivo y `ask=true`. Si el portal no puede atenderla,
usa el selector real `mimeopen --ask` cuando esta instalado; no abre
silenciosamente la aplicacion predeterminada simulando que mostro un selector.
El submenu contextual tambien permite iniciar directamente una aplicacion
compatible concreta.

## Compatibilidad de plataforma

| Funcion | Windows | Linux | macOS |
| --- | --- | --- | --- |
| Navegacion local, pestanas y panel dividido | Compatible | Compatible | Experimental |
| Transferencias y comprimidos | Compatible | Compatible | Experimental |
| Iconos y miniaturas | Integracion nativa | Freedesktop/XDG | Experimental |
| Dispositivos portatiles | WPD/MTP | Dispositivos montados por GVfs/FUSE | No compatible |
| Descubrimiento de red | Proveedores nativos | GVfs/Samba/Avahi y enriquecimiento SMB opcional con KIO | Solo SMB montado |
| Montaje y expulsion de ISO | Compatible | UDisks2 | Experimental |
| Formateo de discos no pertenecientes al sistema | Format-Volume | UDisks2/Polkit | Experimental |
| Difuminado | Efectos nativos | KWin o Blur My Shell opcional | Experimental |
| Microsoft Defender | Compatible | No aplica | No aplica |

Compatibilidad del paquete Linux 1.0.2 generado en la base actual:

| Distribucion o entorno | `.deb` actual | Nivel de validacion |
| --- | --- | --- |
| Debian 13 con GNOME o KDE Plasma | Compatible | Funciones principales probadas en ambos entornos |
| Ubuntu 26.04 con GNOME | Compatible | Flujo principal probado |
| Ubuntu 24.04 y derivadas con `libc6 >= 2.39` | Compatible por ABI y dependencias | No se han probado todas las derivadas ni todos sus escritorios |
| Debian 12 | No compatible con este `.deb` | La aplicacion se probo anteriormente con una compilacion compatible, pero el artefacto actual exige una GLIBC posterior |
| Ubuntu 22.04 | No compatible con este `.deb` | Requiere compilar en una base antigua compatible; no forma parte de la matriz validada actual |
| Otras derivadas de Debian/Ubuntu | Condicional | Requieren `libc6 >= 2.39`, las dependencias declaradas y pruebas en el entorno concreto |

El script de empaquetado detecta la version maxima de GLIBC utilizada por el
binario y la escribe como requisito de `libc6`, evitando que APT instale un
ejecutable que no puede arrancar. Compilar el paquete dentro de una base mas
antigua puede reducir ese requisito, pero esa compilacion se debe probar por
separado. La capa grafica mediante `iced`/`winit` admite sesiones Wayland y
X11; otros escritorios Freedesktop pueden usar las funciones base, pero no se
declaran probados solo por compartir las mismas bibliotecas.

## Linux

El objetivo en Linux es no depender de un unico entorno de escritorio. La base
actual usa piezas comunes del sistema:

- `/proc/self/mountinfo` para listar montajes reales;
- sysfs para detectar unidades removibles u opticas cuando esta disponible;
- tipos de filesystem como `cifs`, `smb3`, `nfs`, `sshfs`, `iso9660` y `udf`;
- Freedesktop Icon Theme y Shared MIME Info para iconos de archivos;
- cache XDG de thumbnails antes de generar miniaturas propias;
- portapapeles MIME con `wl-copy`/`wl-paste`, `xclip` o `xsel` cuando existen;
- UDisks2 mediante `udisksctl` para montar/expulsar ISO o unidades, y mediante
  su API D-Bus estable para formatear discos que no sean del sistema con
  autorizacion Polkit;
- Polkit mediante `pkexec` para reintentos elevados;
- `gio`, Samba y Avahi como descubrimiento de red de mejor esfuerzo, ampliado
  opcionalmente con lugares de KDE, KIOFuse y `kioclient`;
- portal XDG `OpenFile` para mostrar el selector completo de aplicaciones sin
  convertir una ruta local en una URI `file://` incompatible con ese portal;
- dispositivos MTP ya montados por GVfs/FUSE bajo `/run/user/.../gvfs`;
- `xdg-terminal-exec` y terminales comunes como fallback;
- `assets/linux/bexplorer.desktop` con `TryExec=bexplorer`,
  `Exec=bexplorer %f` y `MimeType=inode/directory`.

Diferencias e integraciones opcionales en Linux:

- el arrastre nativo hacia otras aplicaciones esta implementado para Wayland;
  en X11 se puede seleccionar un helper mediante `BEXPLORER_DRAG_HELPER`, y
  `BEXPLORER_DRAG_HELPER_FALLBACK=1` habilita como respaldo automatico
  `ripdrag`, `dragon-drag-and-drop`, `dragon` o `dragon-drop`;
- MTP sin montaje GVfs/FUSE todavia no tiene backend propio;
- el descubrimiento de red depende de los servicios disponibles, las
  credenciales y la configuracion de la red local;
- el enriquecimiento KIO no reemplaza GVfs, Samba o Avahi y solo se usa cuando
  KDE/KIO esta instalado;
- la busqueda de iconos se hace en proceso y todavia puede no cubrir extensiones
  especificas de algunos temas de escritorio.

El `.deb` conserva en `Depends` solamente las dependencias de arranque y las
integraciones base:

- las bibliotecas base de X11, Wayland, EGL y OpenGL utilizadas por
  `iced`/`winit`;
- GLib/GIO, `xdg-utils`, `xdg-desktop-portal` y un backend de portal;
- `udisks2` y elevacion mediante Polkit;
- Shared MIME Info y el tema `hicolor`.

`Recommends` incluye el complemento completo de X11, herramientas para dar
formato a ext, FAT, exFAT, NTFS, Btrfs y XFS, GVfs/FUSE, Samba/Avahi y las
utilidades de cache del escritorio, ademas de un helper compatible para MIME
del portapapeles. APT las instala normalmente cuando estan disponibles, pero
una fuente de paquetes opcional deshabilitada ya no impide instalar BExplorer.

Integraciones que siguen siendo opcionales:

- `libfile-mimeinfo-perl` como alternativa del selector de aplicaciones;
- `kde-cli-tools`, `kio-extras` y `kio-fuse` para enriquecer KDE sin instalar
  toda su pila en escritorios GNOME;
- Blur My Shell para el difuminado en GNOME;
- `ripdrag`, `dragon-drag-and-drop`, `dragon` o `dragon-drop` para arrastrar
  archivos en X11 o como respaldo de Wayland.

Para compilar desde el codigo fuente se requiere Rust 1.92 o posterior, ademas
de un toolchain C/C++ y las cabeceras de desarrollo de Wayland/X11/OpenGL que
requiera `iced`/`winit` en la distribucion elegida.

Comandos recomendados:

```bash
cargo check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo run
```

Los paquetes Linux se generan con `scripts/linux/package.sh`; el tarball, el
paquete `.deb` y sus checksums incluyen los avisos y textos de licencia. El
script valida automaticamente metadatos, arquitectura, ejecutable y
clasificacion de dependencias con `scripts/linux/validate-deb.sh`. El `.deb`
instala el ejecutable en `/usr/bin/bexplorer`, registra la aplicacion en el menu
del escritorio, instala sus iconos escalados en el tema `hicolor` y puede
instalarse con `scripts/linux/install-deb.sh`.

En Windows, `scripts/windows/package.ps1` crea un ZIP portable y un instalador
Inno Setup con checksum SHA-256. El instalador permite elegir espanol o ingles,
crea por defecto una entrada en el menu Inicio y ofrece casillas para crear un
acceso directo en el escritorio y agregar BExplorer al `PATH`. Al desinstalar
retira solamente su propia entrada del `PATH`. Los comandos antiguos de
`tools/` se conservan como wrappers compatibles.

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
La suite incluye una regresion que fuerza un fallo durante un reemplazo y
verifica que el destino original se conserva, ademas de protecciones para el
formateo de almacenamiento en Linux.

Los paquetes portables de Windows incluyen checksum, pero pueden mostrar una
advertencia de SmartScreen hasta que exista un instalador firmado. Consulta
`CHANGELOG.md` para el historial de versiones y `SECURITY.md` para reportar
vulnerabilidades de forma privada.
