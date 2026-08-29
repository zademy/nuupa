# Nuupa

Contexto de una app de escritorio (Tauri) para ver y actualizar los paquetes
globales de los gestores instalados (npm, pnpm, bun), sin escribir comandos.

## Language

**Gestor**:
Programa que administra paquetes globales: npm, pnpm o bun.
_Avoid_: manager, instalador, runtime

**Espacio global**:
El conjunto de paquetes globales de un gestor; cada gestor tiene el suyo e independiente.
_Avoid_: lista global (singular), store

**Paquete global**:
Paquete instalado con `-g`, perteneciente al espacio global de un gestor.
_Avoid_: módulo, dependencia, librería

**Paquete del gestor**:
Paquete global cuyo nombre es el de un gestor (npm, pnpm, bun): se actualiza fuera de Nuupa (npm viene con node; pnpm y bun los instala su instalador oficial) y por eso nunca aparece en el espacio global de ningún gestor.
_Avoid_: excluido (la actualización individual sigue disponible), bloqueado, especial

**Versión activa**:
La versión de node seleccionada por nvm; define qué paquetes globales existen.
_Avoid_: node actual, node por defecto

**Desactualizado**:
Paquete global cuya última versión publicada difiere de la instalada.
_Avoid_: viejo, obsoleto, pendiente

**Actualizar**:
Instalar la última versión publicada de un paquete global (`npm i -g pkg@latest`).
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

**Habilidad**:
Conjunto de instrucciones para agentes (una carpeta con SKILL.md) que Nuupa gestiona a nivel de usuario en la carpeta de habilidades.
_Avoid_: skill, plugin, extensión

**Carpeta de habilidades**:
La ruta `~/.agents/skills/`: nivel de usuario, única ruta que Nuupa toca; todos los agentes leen las habilidades de ahí.
_Avoid_: skills globales, directorio de skills

**Origen**:
De dónde vino una Habilidad gestionada: repositorio, ruta dentro de él y SHA del árbol al momento de instalarla o actualizarla.
_Avoid_: fuente, repositorio (a secas), procedencia

**Gestionada**:
Habilidad cuyo Origen registró Nuupa; única clase que Actualizar puede tocar.
_Avoid_: instalada, trackeada

**No gestionada**:
Habilidad presente en la carpeta de habilidades sin Origen conocido (la puso otra herramienta): visible, sin Actualizar.
_Avoid_: externa, huérfana, manual

**Validación**:
Comprobación de conformidad del contenido de una habilidad; se exige al agregarla y antes de aplicar una actualización. Sin Validación no se activa ni se actualiza.
_Avoid_: chequeo, lint, escaneo

**Conforme**:
Contenido que pasa la Validación: SKILL.md presente, con frontmatter válido (name en minúsculas-con-guiones y description presentes, sin etiquetas XML) y el resto de archivos dentro de la carpeta de la habilidad.
_Avoid_: válido (como estado de fila), correcto

**Inválida**:
Habilidad presente cuyo contenido ya no es Conforme: solo lectura, sin Actualizar hasta resolverse.
_Avoid_: rota, corrupta, dañada

**Actual**:
Habilidad Gestionada cuyo SHA guardado coincide con el actual en su Origen remoto.
_Avoid_: al día, sincronizada, vigente

**Actualización disponible**:
Habilidad Gestionada cuyo SHA guardado difiere del actual en su Origen remoto.
_Avoid_: desactualizada (reservado a paquetes), pendiente, atrás

**Sin verificar**:
Habilidad Gestionada cuyo SHA no se pudo consultar ahora (fallo de red en SU fila): nunca es un veredicto; las demás filas refrescan igual.
_Avoid_: error (a secas), fallida, desactualizada
