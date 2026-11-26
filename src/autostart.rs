//! Порт Autostart.cs: автозапуск задачей планировщика «при входе в систему» с наивысшими правами
//! (без запроса UAC и без ограничения «остановить через 3 дня»). Только Windows.

#[allow(dead_code)]
const TASK_NAME: &str = "RigControl";

pub const UNSAFE_LOCATION_NOTE: &str = "Внимание: rigcontrol.exe или его папку могут менять программы без прав администратора. \
Для автозапуска безопаснее скопировать программу в C:\\Program Files\\RigControl и запускать оттуда.";

/// Упрощённая проверка IsProtectedLocation: exe лежит в Program Files / Windows.
pub fn is_protected_location(exe: &std::path::Path) -> bool {
    let p = exe.to_string_lossy().to_lowercase();
    ["PROGRAMFILES", "PROGRAMFILES(X86)", "WINDIR"].iter().any(|v| {
        std::env::var(v).map(|d| !d.is_empty() && p.starts_with(&d.to_lowercase())).unwrap_or(false)
    })
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// XML задачи: вход в систему, HighestAvailable, без лимита времени, работа от батареи.
pub fn task_xml(exe: &str, args: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>RigControl (Rust)</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><Delay>PT10S</Delay></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>"{}"</Command><Arguments>{}</Arguments></Exec></Actions>
</Task>
"#, xml_escape(exe), xml_escape(args))
}

#[cfg(windows)]
fn schtasks(args: &[&str]) -> i32 {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.code().unwrap_or(-1))
        .unwrap_or(-1)
}

/// Что запускать при входе в Windows: фоновую службу rigcontrol-service.exe рядом с программой,
/// а если её нет — саму программу.
pub fn target_exe() -> std::io::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    let service = exe.with_file_name(crate::service_link::SERVICE_EXE);
    Ok(if service.exists() { service } else { exe })
}

/// Задача «RigControl» общая с C#-версией (то же имя): включённый автозапуск мог остаться от неё.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Off,
    /// Задача запускает фоновую службу этой программы.
    This,
    /// Задача запускает само окно (так было до фоновой службы) — её надо пересоздать.
    Legacy,
    /// Задача есть, но запускает другую программу (например, RigControl на C#) — её команда.
    Other(String),
}

/// Команда из XML задачи (`<Command>`), без кавычек и с раскрытыми &amp; и т. п.
pub fn task_command(xml: &str) -> Option<String> {
    let start = xml.find("<Command>")? + "<Command>".len();
    let end = start + xml[start..].find("</Command>")?;
    let raw = xml[start..end].trim().trim_matches('"');
    Some(raw.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"))
}

/// Вывод schtasks /XML: UTF-16 LE (с BOM или без) или однобайтный.
#[cfg_attr(not(windows), allow(dead_code))]
fn decode(bytes: &[u8]) -> String {
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || (bytes.len() > 8 && bytes[1] == 0 && bytes[3] == 0);
    if utf16 {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).trim_start_matches('\u{FEFF}').to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(windows)]
pub fn status() -> Status {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME, "/XML"])
        .creation_flags(0x0800_0000)
        .stderr(std::process::Stdio::null())
        .output();
    let Ok(out) = out else { return Status::Off };
    if !out.status.success() {
        return Status::Off;
    }
    let Some(cmd) = task_command(&decode(&out.stdout)) else { return Status::Other("?".into()) };
    let lower = |p: std::io::Result<std::path::PathBuf>| p.map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (target, this) = (lower(target_exe()), lower(std::env::current_exe()));
    let cmd_l = cmd.to_lowercase();
    if !target.is_empty() && cmd_l == target {
        Status::This
    } else if !this.is_empty() && cmd_l == this {
        Status::Legacy
    } else {
        Status::Other(cmd)
    }
}

/// Автозапуск включён и запускает именно эту программу.
#[cfg(windows)]
pub fn is_enabled() -> bool {
    status() == Status::This
}

/// enable=true — создать/пересоздать задачу (args — аргументы запуска, например «profile apply Игры»).
#[cfg(windows)]
pub fn set(enable: bool, args: &str) -> Result<String, String> {
    if !enable {
        if status() == Status::Off { return Ok("Автозапуск и так выключен.".into()); }
        return match schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]) {
            0 => Ok("Автозапуск выключен.".into()),
            c => Err(format!("schtasks /Delete вернул {c} (нужны права администратора?)")),
        };
    }
    let exe = target_exe().map_err(|e| e.to_string())?;
    let xml = task_xml(&exe.to_string_lossy(), args);
    // schtasks /XML ждёт UTF-16 LE с BOM.
    let mut bytes = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() { bytes.extend_from_slice(&u.to_le_bytes()); }
    let tmp = std::env::temp_dir().join("rigcontrol-task.xml");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    let tmp_s = tmp.to_string_lossy().to_string();
    let code = schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &tmp_s, "/F"]);
    let _ = std::fs::remove_file(&tmp);
    if code != 0 {
        return Err(format!("schtasks /Create вернул {code}. Запустите консоль от имени администратора."));
    }
    let mut msg = "Автозапуск включён.".to_string();
    if !is_protected_location(&exe) {
        msg.push('\n');
        msg.push_str(UNSAFE_LOCATION_NOTE);
    }
    Ok(msg)
}

#[cfg(not(windows))]
pub fn status() -> Status { Status::Off }

#[cfg(not(windows))]
pub fn is_enabled() -> bool { false }

#[cfg(not(windows))]
pub fn set(_enable: bool, _args: &str) -> Result<String, String> {
    Err("Автозапуск через планировщик есть только в Windows.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn xml_escapes_path() {
        let x = super::task_xml("C:/A&B/rigcontrol.exe", "profile apply \"Игры\"");
        assert!(x.contains("A&amp;B"));
        assert!(x.contains("&quot;Игры&quot;"));
        assert!(x.contains("HighestAvailable"));
    }

    #[test]
    fn command_from_task_xml() {
        let x = super::task_xml("C:\\Program Files\\R&D\\rigcontrol-gui.exe", "--tray");
        assert_eq!(super::task_command(&x).as_deref(), Some("C:\\Program Files\\R&D\\rigcontrol-gui.exe"));
        let mut utf16 = vec![0xFF, 0xFE];
        for u in x.encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(super::task_command(&super::decode(&utf16)), super::task_command(&x));
        assert_eq!(super::task_command("<Task/>"), None);
    }
}
