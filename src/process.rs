use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    // Redirected Python output uses the Windows code page unless explicitly
    // configured. Paths such as Cultura Profética must round-trip as UTF-8.
    cmd.env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8");
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd.kill_on_drop(true);
    cmd
}
pub fn python(root: &Path) -> PathBuf {
    root.join("runtime/python/Scripts/python.exe")
}

pub fn runtime_tool(root: &Path, name: &str) -> Result<PathBuf> {
    let local = root.join("runtime").join(name);
    if local.is_file() {
        return Ok(local);
    }
    if let Some(path) = std::env::var_os("PATH")
        && let Some(tool) = std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    {
        return Ok(tool);
    }
    bail!("Falta {name}. Añádelo a PATH o a la carpeta runtime.")
}
pub async fn run(mut cmd: Command, seconds: u64) -> Result<std::process::Output> {
    let program = cmd.as_std().get_program().to_string_lossy().into_owned();
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = tokio::time::timeout(Duration::from_secs(seconds), cmd.output())
        .await
        .context("El proceso tardó demasiado; se detuvo sin publicar un archivo incompleto.")?
        .with_context(|| format!("No se pudo ejecutar {program}"))?;
    if !out.status.success() {
        bail!(
            "{}",
            String::from_utf8_lossy(&out.stderr)
                .chars()
                .rev()
                .take(2500)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        );
    }
    Ok(out)
}
pub async fn bridge(root: &Path, value: Value) -> Result<Value> {
    let mut cmd = command(python(root));
    cmd.arg(root.join("bridge/catalog.py"))
        .current_dir(root)
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .context("No se pudo iniciar el adaptador del catálogo.")?;
    let mut input = child
        .stdin
        .take()
        .context("No se pudo abrir el adaptador.")?;
    input.write_all(&serde_json::to_vec(&value)?).await?;
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(100), child.wait_with_output())
        .await
        .context("El catálogo tardó demasiado. Puedes repetir esta consulta.")??;
    let reply: Value = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "El adaptador no respondió: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    if reply["ok"] != true {
        bail!(
            "{}",
            reply["error"].as_str().unwrap_or("La consulta falló.")
        );
    }
    Ok(reply["data"].clone())
}

#[cfg(windows)]
pub fn hide_directory(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileAttributesW(path: *const u16) -> u32;
        fn SetFileAttributesW(path: *const u16, attributes: u32) -> i32;
    }
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // The UTF-16 buffer is NUL-terminated and lives through both calls.
    unsafe {
        let attributes = GetFileAttributesW(path.as_ptr());
        if attributes != u32::MAX && attributes & 2 == 0 {
            SetFileAttributesW(path.as_ptr(), attributes | 2);
        }
    }
}

#[cfg(windows)]
pub fn show_error(message: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(window: isize, text: *const u16, title: *const u16, kind: u32) -> i32;
    }
    let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // The dialog receives live, NUL-terminated strings and no parent window.
    unsafe { MessageBoxW(0, wide(message).as_ptr(), wide("Surco").as_ptr(), 0x10) };
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn redirected_python_preserves_accented_windows_paths() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let expected = "C:\\Music\\Cultura Profética\\La Dulzura\\source.webm";
        let mut cmd = command(python(root));
        cmd.args(["-c", "import sys; print(sys.argv[1])", expected]);
        let output = run(cmd, 15).await.unwrap();
        assert_eq!(
            std::str::from_utf8(&output.stdout).unwrap().trim(),
            expected
        );
    }
}
