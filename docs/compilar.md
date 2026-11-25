# Compilar Surco para Windows

Usa Windows x64 con Rust estable para `x86_64-pc-windows-msvc`, Visual Studio Build Tools con desarrollo de escritorio en C++, Python 3.14 x64 y Git. Ejecuta estos comandos en PowerShell desde la carpeta que contiene `Cargo.toml`.

```powershell
powershell -ExecutionPolicy Bypass -File .\Empaquetar.ps1
```

El script descarga y verifica las distribuciones de Python, Node.js y FFmpeg. Instala las dependencias de catálogo, ejecuta las pruebas, compila `surco.exe` y prueba el paquete con solo las rutas del sistema en `PATH`.

El resultado queda en `dist/Surco-1.1.0-windows-x64.zip` con su archivo `.sha256`. Para verificar la descarga, compara el primer campo de ese archivo con el resultado de PowerShell.

```powershell
Get-FileHash .\Surco-1.1.0-windows-x64.zip -Algorithm SHA256
```

Si ya tienes FFmpeg y Node.js instalados, puedes usar `Preparar.ps1` y `Compilar.ps1` para trabajar sobre el código sin generar el ZIP. `Actualizar.ps1` actualiza yt-dlp en el entorno de desarrollo. Para incorporarlo a un ZIP nuevo, actualiza también las versiones de `yt-dlp` y `yt-dlp-ejs` en `requirements.txt`.

## Publicar en GitHub

Sube el código a un repositorio público. Crea y sube la etiqueta `v1.1.0` apuntando al commit de la versión. El flujo **Publicar Surco** compila, verifica y crea la publicación con el ZIP portable y su suma SHA-256. La etiqueta debe coincidir con la versión de `Cargo.toml`. GitHub añade los archivos del código fuente a la publicación. [Documentación de GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)
