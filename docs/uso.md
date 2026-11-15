# Guía de Surco

## Desde cero

1. En **Releases** de Surco en GitHub, abre **Assets** y descarga `Surco-1.0.0-windows-x64.zip`. Si ya extrajiste ese ZIP, abre `surco.exe` en esta carpeta.
2. En Windows 10 u 11 de 64 bits, haz clic derecho sobre el ZIP y elige **Extraer todo**. Abre `surco.exe` en la carpeta que creó Windows.
3. En la ventana de Surco, busca un artista o el título de un álbum. También puedes pegar el enlace de un álbum de YouTube Music.
4. Abre el artista o una publicación y marca hasta dos canciones en total. Para cambiar la selección, desmarca alguna de las canciones elegidas.
5. Pulsa **Descargar selección**. La sección **Descargas** muestra el estado de cada canción. Pulsa **Abrir carpeta** para acceder a tus archivos.

La primera vez que abras Surco en tu perfil de Windows, el buscador y el historial estarán vacíos. Las versiones extraídas en carpetas distintas comparten el historial y los ajustes del mismo perfil de Windows.

## Formato y archivos

- **MP3 a 320 kbps** convierte el audio al formato MP3. **M4A** conserva el AAC de origen cuando el catálogo lo ofrece.
- **Ajustes** permite cambiar el formato y la carpeta de destino. La ubicación inicial es `%USERPROFILE%\Music\Surco`.
- Surco usa el audio oficial que corresponde a la edición elegida. Si no puede confirmar la fuente o la disponibilidad de una pista, muestra el motivo junto a ella.
- Al cerrar la ventana, el motor local sigue procesando la cola. Usa **Cerrar motor** para salir y pausar el trabajo activo. Puedes reanudarlo desde **Descargas** al abrir Surco de nuevo.

## Si algo falla

- Si faltan componentes, vuelve a extraer el ZIP completo y abre el ejecutable en esa carpeta. `surco.exe`, `bridge` y `runtime` deben estar juntos.
- Si la búsqueda o la ficha de un artista falla, comprueba tu conexión y vuelve a intentar la consulta.
- Si una pista queda pendiente, abre **Resultados por canción** en **Descargas** para leer el motivo. Si YouTube pide iniciar sesión, **Ajustes** permite elegir un navegador compatible para usar sus cookies. Usa esta opción solo cuando decidas compartir esa sesión con el descargador. [Documentación de yt-dlp sobre cookies](https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp)
- Para actualizar, usa la carpeta extraída de una nueva versión. Surco guarda su configuración e historial en `%LOCALAPPDATA%\Surco`, por separado de los archivos descargados.

Descarga contenido que tengas permiso de guardar. [Términos de YouTube](https://www.youtube.com/t/terms)
