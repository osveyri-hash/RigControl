//! Служба подсветки Armoury Crate (LightingService), как в MainWindow.Argb.cs: пока она работает,
//! Armoury Crate перехватывает подсветку платы и перебивает цвета программы. Остановить — sc stop
//! (и запуск «вручную», чтобы не вернулась после перезагрузки), вернуть — sc start + «автоматически».
//! Нужны права администратора (окно RigControl их и так просит).

/// Имя службы и её процесса.
pub const SERVICE: &str = "LightingService";

#[cfg(windows)]
fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
}

/// Работает ли служба: есть процесс LightingService.exe.
pub fn running() -> bool {
    #[cfg(windows)]
    {
        let out = hidden(std::process::Command::new("tasklist").args(["/FI", &format!("IMAGENAME eq {SERVICE}.exe"), "/NH"]))
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains(&SERVICE.to_ascii_lowercase()),
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn sc(args: &[&str]) -> Result<(), String> {
    let out = hidden(std::process::Command::new("sc.exe").args(args)).output().map_err(|e| format!("sc.exe: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Err(if text.is_empty() { format!("sc.exe вернул {}", out.status) } else { text })
    }
}

/// Остановить службу и не запускать её при старте Windows.
pub fn stop() -> Result<(), String> {
    #[cfg(windows)]
    {
        sc(&["config", SERVICE, "start=", "demand"])?;
        // Уже остановлена — sc stop вернёт ошибку 1062, это не страшно.
        let _ = sc(&["stop", SERVICE]);
        std::thread::sleep(std::time::Duration::from_millis(1500));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("только в Windows".into())
    }
}

/// Вернуть службу: запуск «автоматически» и сразу запустить.
pub fn start() -> Result<(), String> {
    #[cfg(windows)]
    {
        sc(&["config", SERVICE, "start=", "auto"])?;
        let _ = sc(&["start", SERVICE]);
        std::thread::sleep(std::time::Duration::from_millis(1500));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("только в Windows".into())
    }
}
