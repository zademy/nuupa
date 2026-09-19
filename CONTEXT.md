# Nuupa

Contexto de una app de escritorio (Tauri) para ver y actualizar los paquetes
globales de los gestores instalados (npm, pnpm, bun, brew), sin escribir comandos.

## Language

**Gestor**:
Programa que administra paquetes globales: npm, pnpm, bun o brew.
_Avoid_: manager, instalador, runtime

**Espacio global**:
El conjunto de paquetes globales de un gestor; cada gestor tiene el suyo e independiente.
_Avoid_: lista global (singular), store

**Paquete global**:
Paquete perteneciente al espacio global de un gestor: en los gestores de node se instala con `-g`; en brew son fórmulas hoja y casks.
_Avoid_: módulo, dependencia, librería

**Fórmula**:
Paquete de brew que instala herramientas o librerías de línea de comandos.
_Avoid_: paquete cli, librería

**Cask**:
Paquete de brew (solo macOS) que instala aplicaciones de usuario.
_Avoid_: app, aplicación

**Fórmula hoja**:
Fórmula que el usuario instaló a propósito (no llegó como dependencia de otra): las únicas fórmulas del espacio global de brew.
_Avoid_: top-level, principal

**Pineada**:
Fórmula marcada en brew para no actualizarse; Nuupa la muestra como Excluida de solo lectura.
_Avoid_: bloqueada, congelada, fija

**Paquete del gestor**:
Paquete global cuyo nombre es el de un gestor (npm, pnpm, bun; brew tampoco se lista a sí mismo): se actualiza fuera de Nuupa (npm viene con node; pnpm y bun los instala su instalador oficial) y por eso nunca aparece en el espacio global de ningún gestor.
_Avoid_: excluido (la actualización individual sigue disponible), bloqueado, especial

**Versión activa**:
La versión de node seleccionada por nvm; define qué paquetes globales existen.
_Avoid_: node actual, node por defecto

**Desactualizado**:
Paquete global cuya última versión publicada difiere de la instalada.
_Avoid_: viejo, obsoleto, pendiente

**Actualizar**:
Instalar la última versión publicada de un paquete global (`npm i -g pkg@latest`, `brew upgrade --formula pkg`).
_Avoid_: upgradear, refrescar

**Actualizar todo**:
Cola secuencial que actualiza, de a uno, los paquetes desactualizados no excluidos.
_Avoid_: actualizar en lote, bulk

**Detenido**:
Paquete cuya actualización fue cortada a pedido del usuario (Detener): no es un fallo; la fila vuelve a la normalidad y el resumen lo cuenta aparte de los fallidos.
_Avoid_: cancelado, abortado, fallido

**Excluido**:
Paquete marcado para que "Actualizar todo" lo salte en SU gestor; la actualización individual sigue disponible. Persiste entre sesiones, por (gestor, paquete).
_Avoid_: bloqueado, ignorado, pausado

**Refrescar**:
Volver a consultar la lista de paquetes y sus últimas versiones.
_Avoid_: sincronizar, actualizar (reservado para versiones)

**Plazo**:
Tiempo máximo de espera de un comando de gestor. Al vencer, Nuupa finaliza su proceso de forma escalonada (cortés, tregua, forzosa) y muestra un error visible; nunca queda esperando para siempre. 60 s para consultas, 300 s para actualizaciones.
_Avoid_: timeout, límite, expiración

**Descubrimiento**:
Hallar el binario de un gestor en esta máquina: el PATH y las ubicaciones habituales por sistema. Si no aparece, el gestor no está: su pestaña no existe y el error muestra dónde se buscó. Nunca exige al usuario configurar nada.
_Avoid_: validación de instalación, chequeo de entorno

**Tema**:
Conjunto de colores con nombre que el usuario elige y que persiste entre sesiones; el claro es el predeterminado.
_Avoid_: skin, modo

**Paleta**:
Los valores concretos (rol → color) que componen un tema.
_Avoid_: esquema, colores sueltos

**Rol**:
La función semántica de un color (fondo, superficie, borde, texto tenue…): igual en todos los temas; cambia el color, no el rol.
_Avoid_: variable, token
