# Surco

Surco busca álbumes en YouTube Music y organiza sus pistas en una biblioteca local de Windows. Cada archivo conserva el orden de la edición elegida, sus etiquetas, la portada y la letra cuando está disponible.

![Pantalla de búsqueda de Surco](docs/surco.png)

## Instalación en Windows

Instala [Rust estable](https://rust-lang.org/tools/install/) con las herramientas de compilación C++ de Visual Studio, [Python 3.14](https://www.python.org/downloads/), [Node.js 24](https://nodejs.org/) y [FFmpeg](https://ffmpeg.org/download.html). Comprueba que `cargo`, `python`, `node` y `ffmpeg` estén en `PATH`. Surco también admite copias locales de `node.exe` y `ffmpeg.exe` en `runtime/`.

Desde la carpeta del proyecto, ejecuta en PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\Preparar.ps1
powershell -ExecutionPolicy Bypass -File .\Compilar.ps1
```

Abre **Iniciar Surco.cmd**. El motor escucha en `127.0.0.1:8737` y abre una ventana de Edge o el navegador predeterminado. **Cerrar motor** pausa el trabajo activo. Si solo cierras la ventana, la cola sigue funcionando.

## Descargar una edición

Busca un artista y abre **Canciones**, **Álbumes** o **Sencillos y EP**. Puedes marcar varias canciones y publicaciones en una sola selección. Marca **Completo** para guardar todas las pistas disponibles de una publicación o ábrela para elegir pistas concretas. Pulsa **Descargar selección** para añadirlas a la cola.

También puedes buscar por título o pegar el enlace de un álbum. Si el catálogo agrupa varias publicaciones, abre **Otras ediciones** en su ficha.

Surco toma el identificador de audio oficial de la lista de la edición elegida. Comprueba posición, título y artista. Contrasta la duración disponible con la ficha del álbum y verifica el archivo antes de guardarlo. Si la fuente resulta ambigua o no está disponible, la pista queda pendiente con un motivo. La verificación usa metadatos del catálogo y del archivo.

**MP3 a 320 kbps** recodifica el audio de origen, normalmente AAC u Opus. La conversión no mejora su calidad. **M4A** conserva el audio AAC de origen sin recodificar cuando está disponible. Puedes cambiar el formato y el destino en **Ajustes**. La carpeta de música predeterminada es `%USERPROFILE%\Music\Surco`.

Surco reutiliza archivos del historial cuyos datos de fuente y duración coinciden.

Si el catálogo funciona y las descargas empiezan a fallar, ejecuta `powershell -ExecutionPolicy Bypass -File .\Actualizar.ps1` para actualizar yt-dlp y reinicia Surco.

- Usar las cookies de tu navegador con yt-dlp puede provocar restricciones en la cuenta de YouTube. [Documentación de yt-dlp](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies)
- Descargar de YouTube Music puede infringir los términos del servicio. Comprueba que tienes permiso para descargar el contenido. El uso de Surco es responsabilidad de quien lo ejecuta. [Términos de YouTube](https://www.youtube.com/t/terms)

## Código y pruebas

- `src/` contiene el servidor local, la cola y la publicación de archivos en Rust.
- `bridge/` conecta el catálogo, la descarga y las etiquetas con ytmusicapi, yt-dlp y Mutagen.
- `web/` contiene la interfaz integrada en el ejecutable.
- `tests/` contiene pruebas y muestras reducidas de catálogo. `Compilar.ps1` ejecuta las pruebas de Rust, Python y JavaScript antes de compilar.

Cada usuario de Windows tiene su propia cola, historial y ajustes en `%LOCALAPPDATA%\Surco`. Si esa variable falta, Surco usa `%APPDATA%\Surco` o, en último término, `data/` junto al programa. Copia la carpeta del perfil con Surco cerrado para respaldarla. El cambio de ubicación empieza con un perfil nuevo. Los archivos de música se guardan por separado.

`SURCO_DATA` tiene prioridad y permite usar un perfil de pruebas. El puerto local 8737 admite un motor a la vez. Si lo ocupa otro perfil de Windows, ciérralo desde la sesión que lo inició. Los errores de arranque se guardan como `surco-error.log` dentro del perfil.

La refracción de la interfaz usa [Glasswork v1.1.0](https://github.com/kubai087/glasswork/tree/v1.1.0). Su [licencia MIT](web/vendor/glasswork/LICENSE) se conserva junto al módulo.
