# Guía de uso de Surco

## Abrir Surco por primera vez

1. En **Releases** de Surco en GitHub, abre **Assets** y descarga el ZIP que termina en `-windows-x64.zip`. El paquete es portable y funciona en Windows 10 y 11 de 64 bits.
2. Haz clic derecho en el ZIP y elige **Extraer todo**. Si ya lo extrajiste, continúa en la carpeta que creó Windows.
3. Abre `surco.exe`. La ventana de Surco usa Microsoft Edge o Google Chrome. Mantén `surco.exe`, `bridge` y `runtime` en la misma carpeta.

La primera vez que abres Surco con tu usuario de Windows, el buscador y el historial están vacíos. Las carpetas extraídas en el mismo perfil de Windows comparten el historial y los ajustes.

## Encontrar y guardar canciones

1. Escribe el nombre de un artista o álbum y pulsa **Buscar**. También puedes pegar un enlace de álbum de YouTube Music.
2. Si abres un artista, elige **Canciones**, **Álbumes** o **Sencillos y EP**. En la ficha de un álbum aparecen las pistas de esa edición. Usa **Otras ediciones** cuando el catálogo ofrezca variantes.
3. Marca las canciones que quieras, incluso de distintos álbumes. Usa **Seleccionar álbum completo** para marcar todas las pistas disponibles de la edición abierta. Puedes volver a los resultados y marcar otros álbumes antes de enviar la selección.
4. Pulsa **Descargar selección** para añadir las pistas a la cola. Surco las procesa una a la vez. En **Descargas**, abre **Resultados por canción** para ver el estado de cada pista. Con **Abrir carpeta** puedes encontrar los archivos que se guardaron.

Surco usa el audio oficial que corresponde a la edición elegida. Cuando el catálogo no permite confirmar ese audio o la pista está bloqueada, la interfaz muestra el motivo.

## Comprobar letras

Abre **Letras**, busca por canción o artista y pulsa **Comprobar letra** en el resultado que quieras. Si YouTube Music ofrece la letra, aparecerá en la misma pestaña.

## Elegir el formato y la carpeta

En **Ajustes**, elige el formato y pulsa **Guardar ajustes**. **MP3 a 320 kbps** convierte el audio a un formato compatible con muchos reproductores. **M4A** conserva el audio AAC de la fuente cuando está disponible. Configura el formato antes de añadir la selección a **Descargas**.

La carpeta inicial es `%USERPROFILE%\Music\Surco`. Puedes abrirla desde **Abrir carpeta** o cambiarla en **Ajustes**. Dentro de ella verás carpetas por artista y álbum, y cada archivo lleva el número de pista y el título.

## Controlar las descargas

- **Pausar** detiene temporalmente el trabajo. **Continuar** lo devuelve a la cola.
- **Reintentar pendientes** vuelve a intentar las pistas que quedaron sin guardar.
- La **X** de la ventana cierra Surco y deja en pausa el trabajo activo. Cuando vuelvas a abrirlo, puedes pulsar **Continuar** en **Descargas**.

## Resolver problemas

### Surco avisa que faltan archivos

Vuelve a extraer el ZIP completo y abre `surco.exe` desde la carpeta extraída. El ejecutable utiliza las carpetas `bridge` y `runtime` que vienen junto a él.

### La búsqueda o la ficha de un artista falla

Comprueba la conexión a internet y repite la consulta. Si ya tienes el enlace del álbum, puedes pegarlo en el buscador para abrir esa edición directamente.

### Una canción queda pendiente

En **Descargas**, abre **Resultados por canción** para leer el motivo. Si YouTube pide iniciar sesión, abre **Ajustes** → **Acceso a fuentes que requieren sesión**, selecciona el navegador donde tienes iniciada la sesión y pulsa **Guardar ajustes**. Regresa a **Descargas** y pulsa **Reintentar pendientes**. Usa esta opción cuando decidas compartir esa sesión con el descargador. [Documentación de yt-dlp sobre cookies](https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp)

### Quiero actualizar Surco

Cierra Surco y extrae el ZIP de la versión nueva en una carpeta. Al abrir el nuevo `surco.exe`, Surco lee el historial y los ajustes de `%LOCALAPPDATA%\Surco`. Tu música permanece en la carpeta que configuraste.

Descarga contenido que tengas permiso de guardar. [Términos de YouTube](https://www.youtube.com/t/terms)
