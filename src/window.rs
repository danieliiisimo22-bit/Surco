use crate::process;
use anyhow::{Context, Result};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(callback: unsafe extern "system" fn(isize, isize) -> i32, context: isize)
    -> i32;
    fn GetWindowThreadProcessId(window: isize, process: *mut u32) -> u32;
    fn IsWindowVisible(window: isize) -> i32;
}

fn has_visible_window(pid: u32) -> bool {
    struct Search {
        pid: u32,
        found: bool,
    }
    unsafe extern "system" fn check(window: isize, context: isize) -> i32 {
        // EnumWindows invokes the callback synchronously with the live Search pointer.
        let search = unsafe { &mut *(context as *mut Search) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == search.pid && unsafe { IsWindowVisible(window) } != 0 {
            search.found = true;
            return 0;
        }
        1
    }
    let mut search = Search { pid, found: false };
    unsafe { EnumWindows(check, (&mut search as *mut Search) as isize) };
    search.found
}

pub async fn run(data_dir: &Path, port: u16) -> Result<()> {
    let browser = [
        "Microsoft/Edge/Application/msedge.exe",
        "Google/Chrome/Application/chrome.exe",
    ]
    .into_iter()
    .flat_map(|browser| {
        ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(move |base| Path::new(&base).join(browser))
    })
    .find(|path| path.is_file())
    .context("Surco necesita Microsoft Edge o Google Chrome para abrir su ventana.")?;

    // A dedicated profile keeps this window in a process we can monitor and close.
    let mut browser_profile = std::ffi::OsString::from("--user-data-dir=");
    browser_profile.push(data_dir.join("window-profile"));
    let mut child = process::command(browser)
        .arg(browser_profile)
        .arg(format!("--app=http://127.0.0.1:{port}"))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-mode",
            "--window-size=1280,900",
        ])
        .spawn()
        .context("No se pudo abrir la ventana de Surco.")?;
    let pid = child
        .id()
        .context("No se pudo identificar la ventana de Surco.")?;
    let started = Instant::now();
    let mut window_opened = false;
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if child.try_wait()?.is_some() {
            anyhow::ensure!(
                window_opened,
                "El navegador se cerró antes de mostrar Surco."
            );
            return Ok(());
        }
        if has_visible_window(pid) {
            window_opened = true;
        } else if window_opened {
            // Give the browser time to finish closing before kill_on_drop takes effect.
            let _ = tokio::time::timeout(Duration::from_millis(500), child.wait()).await;
            return Ok(());
        } else {
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(30),
                "No se mostró la ventana de Surco."
            );
        }
    }
}
