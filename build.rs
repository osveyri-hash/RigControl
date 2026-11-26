//! rigcontrol-gui.exe просит права администратора, как app.manifest C#-версии (requireAdministrator):
//! без них LibreHardwareMonitor не читает температуры CPU и не управляет вентиляторами платы.
//! Только для release: отладочные сборки и тесты (cargo test) запускаются без запроса прав.
//! Автозапуск через планировщик (HighestAvailable) стартует программу с правами без вопроса.

//! Значок программы: assets/app.res (тот же app.ico, что у C#-версии, уже как ресурс Windows) —
//! link.exe принимает .res напрямую, поэтому ни rc.exe, ни лишних крейтов не нужно.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app.res");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    if windows && msvc {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        let res = std::path::Path::new(&dir).join("assets").join("app.res");
        if res.exists() {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
    }
    if windows && msvc && release {
        // Службе права нужны всегда (датчики, вентиляторы); автозапуск даёт их без вопроса UAC.
        for bin in ["rigcontrol-gui", "rigcontrol-service"] {
            println!("cargo:rustc-link-arg-bin={bin}=/MANIFEST:EMBED");
            println!("cargo:rustc-link-arg-bin={bin}=/MANIFESTUAC:level='requireAdministrator'");
        }
    }
}
