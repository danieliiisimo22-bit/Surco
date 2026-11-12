@echo off
cd /d "%~dp0"
if not exist "surco.exe" (
  echo No se encontro surco.exe. Consulta README.md para compilarlo.
  pause
  exit /b 1
)
start "" "%~dp0surco.exe"
