# BExplorer

BExplorer es un explorador de archivos avanzado y liviano escrito en Rust. Su
objetivo es mejorar la gestion diaria de archivos sin intentar reemplazar todo
el shell del sistema.

La prioridad historica ha sido Windows, pero el proyecto esta organizado para
que la logica especifica de cada sistema operativo quede separada. Linux ya
compila y tiene una primera base neutral para escritorio; macOS sigue como
trabajo futuro.

## Estado

BExplorer esta en fase beta.

La version actual ya es usable para pruebas internas en Windows, especialmente
para gestion de archivos, comprimidos, vista dividida, red, dispositivos MTP y
operaciones comunes como copiar, mover, eliminar y renombrar. En Linux ya hay
soporte inicial para compilar, navegar archivos locales, detectar montajes desde
`/proc/self/mountinfo`, clasificar unidades USB/red/opticas cuando el sistema
lo expone, usar terminales comunes y registrar la app como gestor de carpetas
con un archivo `.desktop`. La ventana la maneja `iced`/`winit`, asi que la app
puede ejecutarse tanto en Wayland como en X11 cuando las librerias de runtime
estan disponibles.

La interfaz ya se ha migrado de `egui` a `iced` y se ha eliminado la
implementacion visual sustituida. Cubre navegacion local, pestanas, panel
dividido, vistas, filtrado, agrupacion y ordenacion por columnas, renombrado,
eliminacion en segundo plano y transferencias en cola. Los cambios de sesion se
guardan al producirse y las carpetas grandes se renderizan incrementalmente en
lotes de 500 elementos, sin ocultar permanentemente los elementos restantes.
La busqueda completa, vistas previas, comprimidos, Defender, MTP, montaje de
imagenes, red y arrastrar y soltar nativo estan conectados a la interfaz.

La interfaz `iced` esta organizada por responsabilidad en `src/iced_ui`:
`mod.rs` contiene el estado y los mensajes; `update.rs` procesa eventos;
`interaction.rs`, `navigation.rs` y `search_state.rs` gestionan entrada,
navegacion y busqueda; `view.rs` y `view/` contienen la composicion visual;
`file_actions.rs` las operaciones y transferencias; `advanced.rs` conecta
Defender, MTP y unidades; y `helpers/` agrupa presentacion y persistencia.

Antes de una beta publica conviene seguir probando:

- instalaciones limpias de Windows;
- redes con diferentes permisos y credenciales;
- dispositivos USB, discos externos y celulares MTP;
- carpetas que requieren permisos de administrador;
- archivos comprimidos grandes o protegidos con contrasena;
- escenarios de arrastrar y soltar dentro y fuera de la aplicacion.
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
  montajes Linux y dispositivos portatiles MTP en Windows.
- Descubrimiento progresivo de red con cache.
- Copiar, cortar y pegar compatible con el portapapeles de Windows; en Linux se
  usan helpers MIME nativos cuando existen, con fallback de texto.
- Arrastrar y soltar dentro de BExplorer y entre BExplorer y Windows.
- Cola de transferencias con progreso, pausa, cancelacion y manejo de
  conflictos.
- Compresiones concurrentes con ventana propia de progreso, que vuelve a primer
  plano al iniciar una transferencia o compresion nueva.
- Acciones elevadas de remediacion y exclusiones de Microsoft Defender.
- Busqueda rapida y busqueda completa, incluyendo archivos dentro de
  comprimidos soportados.
- Panel de vista previa para imagenes, texto, SVG y PDF.
- Integracion con Windows Defender.
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

Limitaciones actuales en Linux:

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
cargo test
cargo run
```

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

## Beta Interna

La primera beta interna esta marcada como:

```text
v0.1.0-beta.1
```

Esta version sirve para pruebas controladas. Todavia no esta firmada, por lo que
Windows puede mostrar advertencias al ejecutar el archivo.
