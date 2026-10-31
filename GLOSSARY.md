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
Fórmula que brew lista como hoja: no es dependencia de runtime de otra fórmula ni de un cask instalado. Es lo que brew llama _leaf_. Que una fórmula esté instalada a propósito no la hace hoja: `cairo` y `pango` se instalaron a propósito y a la vez son dependencias, así que brew no las considera hojas.
_Avoid_: top-level, principal

**Pineada**:
Fórmula marcada en brew para no actualizarse; Nuupa la muestra como Excluida y nunca la actualiza. Desinstalarla quita también el pineo.
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
Cola secuencial que actualiza, de a uno, el objetivo menos las Excluidas. Las Pineadas no llegan al objetivo (brew se niega a actualizarlas), así que la cola nunca las ve.
_Avoid_: actualizar en lote, bulk

**Objetivo de actualización**:
Los paquetes que un gestor declara actualizables ahora mismo. Para los gestores de node es el subconjunto desactualizado del Espacio global; para brew es todo lo que `brew outdated` declara, dependencias incluidas (las dependencias no son filas, pero sí se pueden actualizar), menos las Pineadas: brew se niega a actualizarlas, así que su fila las muestra como Excluida y quitarles el pino en brew las devuelve al objetivo. El botón de Actualizar todo, el contador y la cola leen el objetivo: nunca pueden discrepar entre sí.
_Avoid_: lista de actualización, pendientes, desactualizables

**Desinstalar**:
Quitar un paquete global de su espacio global (`npm rm -g pkg`, `brew uninstall pkg`): destructivo, pide confirmación y borra el registro de Excluido si lo había. Aplica en toda fila, incluso Excluida o Pineada.
_Avoid_: remover, eliminar, borrar

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
Hallar el binario de un gestor en esta máquina: el PATH, las ubicaciones habituales por sistema y la configuración donde el usuario define su PATH (shell en macOS/Linux, registro en Windows). Si no aparece, el gestor no está: su pestaña no existe y el error muestra dónde se buscó. Nunca exige al usuario configurar nada.
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
