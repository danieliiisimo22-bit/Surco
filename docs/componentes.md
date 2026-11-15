# Componentes y licencias

La licencia del código propio de Surco está por definir. Esta página describe las licencias de los componentes de terceros que usa la versión portable para Windows.

## Componentes principales

| Componente | Función en Surco | Licencia | Fuente y aviso |
| --- | --- | --- | --- |
| Python 3.14.7 | Ejecuta el adaptador de catálogo y audio. | Python Software Foundation License v2, con avisos adicionales. | [Distribución oficial](https://www.python.org/ftp/python/3.14.7/) · [`LICENSE`](https://github.com/python/cpython/blob/v3.14.7/LICENSE) |
| Node.js 24.22.1 | Ejecuta el JavaScript que utiliza yt-dlp para resolver desafíos de YouTube. | MIT, con avisos adicionales. | [Distribución oficial](https://nodejs.org/dist/v24.22.1/) · [`LICENSE`](https://github.com/nodejs/node/blob/v24.22.1/LICENSE) |
| FFmpeg 9.0.2 Essentials Build | Convierte y comprueba los archivos de audio como proceso independiente. | GPLv3 para esta compilación. | [Binario de Gyan](https://github.com/GyanD/codexffmpeg/releases/tag/9.0.2) · [Fuente correspondiente de FFmpeg](https://github.com/FFmpeg/FFmpeg/archive/946fcce07b.tar.gz) · [Bibliotecas de la compilación](https://www.gyan.dev/ffmpeg/builds/#libraries) |
| Glasswork 1.1.0 | Aplica el efecto de vidrio en la interfaz. | MIT | [Código](https://github.com/kubai087/glasswork/tree/v1.1.0) · [Licencia incluida](../web/vendor/glasswork/LICENSE) |
| Axum y Tokio | Servidor local y ejecución asíncrona en Rust. | MIT | [Axum](https://github.com/tokio-rs/axum) · [Tokio](https://github.com/tokio-rs/tokio) |
| Serde | Lee y escribe los datos JSON del motor. | MIT o Apache-2.0 | [Serde](https://github.com/serde-rs/serde) |

## Dependencias de Python incluidas

| Paquete | Licencia declarada por el paquete |
| --- | --- |
| ytmusicapi, Brotli, charset-normalizer, truststore, urllib3 | MIT |
| yt-dlp | Unlicense |
| yt-dlp-ejs | Unlicense, MIT e ISC |
| Mutagen | GPL-2.0-or-later |
| Pillow | MIT-CMU |
| Requests | Apache-2.0 |
| certifi | MPL-2.0 |
| idna, websockets | BSD-3-Clause |
| pycryptodomex | BSD y dominio público |

Las versiones exactas de todas las dependencias se fijan en [`requirements.txt`](../requirements.txt) y [`Cargo.lock`](../Cargo.lock).

## Dónde encontrar los textos completos

- `Licencias/` reúne los avisos de Node.js, FFmpeg y Glasswork en el ZIP. `FFmpeg-fuentes-y-compilacion.txt` incluye la configuración y el enlace al código fuente de la compilación distribuida.
- `Licencias/Rust/` reúne los avisos de los paquetes de Rust incluidos en la compilación.
- `runtime/python/LICENSE.txt` contiene la licencia y los avisos de Python. Los avisos de sus paquetes están en `runtime/python/Lib/site-packages/`, dentro de sus carpetas `*.dist-info`.
- Las capturas muestran portadas y nombres del catálogo de YouTube Music para ilustrar el flujo de uso.
