# BExplorer

BExplorer es un explorador de archivos avanzado y liviano escrito en Rust. Su
objetivo es mejorar la gestion diaria de archivos sin intentar reemplazar todo
el shell de Windows.

La prioridad actual es Windows, pero el proyecto esta organizado para que la
logica especifica de cada sistema operativo quede separada. La idea es poder
trabajar Linux y macOS mas adelante sin romper la base de la aplicacion.

## Estado

BExplorer esta en fase beta.

La version actual ya es usable para pruebas internas en Windows, especialmente
para gestion de archivos, comprimidos, vista dividida, red, dispositivos MTP y
operaciones comunes como copiar, mover, eliminar y renombrar.

Antes de una beta publica conviene seguir probando:

- instalaciones limpias de Windows;
- redes con diferentes permisos y credenciales;
- dispositivos USB, discos externos y celulares MTP;
- carpetas que requieren permisos de administrador;
- archivos comprimidos grandes o protegidos con contrasena;
- escenarios de arrastrar y soltar dentro y fuera de la aplicacion.

## Funciones Principales

- Navegacion por pestanas con historial independiente.
- Pantalla dividida con vistas independientes.
- Barra lateral redimensionable y reordenable.
- Barra de acciones y barra de marcadores opcionales.
- Vistas de detalles, lista, iconos, iconos grandes, iconos extra grandes y
  mosaicos.
- Soporte para unidades locales, extraibles, ISO montadas, rutas UNC, red y
  dispositivos portatiles MTP.
- Descubrimiento progresivo de red con cache.
- Copiar, cortar y pegar compatible con el portapapeles de Windows.
- Arrastrar y soltar dentro de BExplorer y entre BExplorer y Windows.
- Cola de transferencias con progreso, pausa, cancelacion y manejo de
  conflictos.
- Reintento elevado con UAC cuando Windows deniega permisos.
- Busqueda rapida y busqueda completa, incluyendo archivos dentro de
  comprimidos soportados.
- Panel de vista previa para imagenes, texto, SVG y PDF.
- Integracion con Windows Defender.
- Personalizacion de tema, color, bordes de iconos, efectos de ventana, atajos
  y distribucion de la interfaz.

## Comprimidos

BExplorer usa un motor 7-Zip integrado para abrir, listar y extraer muchos
formatos de comprimidos.

Flujos soportados:

- crear archivos ZIP;
- crear archivos 7z;
- crear ZIP y 7z protegidos con contrasena;
- extraer ZIP, 7z, RAR, ISO, TAR y otros formatos compatibles con 7-Zip;
- pedir contrasena cuando el comprimido la requiere;
- buscar dentro de comprimidos durante la busqueda completa.

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
