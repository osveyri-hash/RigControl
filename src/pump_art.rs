//! Встроенные картинки тем экрана помпы — те же, что Assets/*.jpg|gif в C#-версии.
//! В settings.json C# хранит их как «pack://application:,,,/Assets/AnimeHina.gif» (CoolerBackground),
//! поэтому такие пути понимают обе версии.

/// Префикс встроенной картинки (LcdRender.IsResource в C#).
pub const PACK: &str = "pack://application:,,,/Assets/";

/// Встроенная картинка по имени файла (AsciiArt.jpg, AnimeHina.gif, …).
pub fn bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "AsciiArt.jpg" => include_bytes!("../assets/themes/AsciiArt.jpg"),
        "AsciiArtDark.jpg" => include_bytes!("../assets/themes/AsciiArtDark.jpg"),
        "ArtMangaDark.jpg" => include_bytes!("../assets/themes/ArtMangaDark.jpg"),
        "ArtDotsDark.jpg" => include_bytes!("../assets/themes/ArtDotsDark.jpg"),
        "AnimeHina.gif" => include_bytes!("../assets/themes/AnimeHina.gif"),
        "AnimeLace.gif" => include_bytes!("../assets/themes/AnimeLace.gif"),
        "AnimeEyes.gif" => include_bytes!("../assets/themes/AnimeEyes.gif"),
        "AnimeEars.gif" => include_bytes!("../assets/themes/AnimeEars.gif"),
        "AnimeMaid.gif" => include_bytes!("../assets/themes/AnimeMaid.gif"),
        "AnimeWarm.jpg" => include_bytes!("../assets/themes/AnimeWarm.jpg"),
        _ => return None,
    })
}

/// Путь к встроенной картинке: «pack://…/Assets/Имя» → «Имя».
pub fn resource_name(path: &str) -> Option<&str> {
    let p = path.trim();
    if !p.to_ascii_lowercase().starts_with("pack://") {
        return None;
    }
    p.rsplit('/').next()
}

/// Путь для settings.json (как пишет C#).
pub fn pack_uri(name: &str) -> String {
    format!("{PACK}{name}")
}

/// Встроенная ли это картинка (pack://…).
pub fn is_resource(path: &str) -> bool {
    resource_name(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_paths() {
        let p = pack_uri("AnimeHina.gif");
        assert_eq!(resource_name(&p), Some("AnimeHina.gif"));
        assert!(bytes("AnimeHina.gif").is_some());
        assert_eq!(resource_name("C:\\pic.gif"), None);
        for name in ["AsciiArt.jpg", "AsciiArtDark.jpg", "ArtMangaDark.jpg", "ArtDotsDark.jpg", "AnimeLace.gif",
            "AnimeEyes.gif", "AnimeEars.gif", "AnimeMaid.gif", "AnimeWarm.jpg"] {
            assert!(bytes(name).is_some(), "{name}");
        }
    }
}
