//! HSV-функции из ColorWheel.cs (сам круг — это WPF-контрол, его перенесём вместе с UI).

use crate::Rgb;

/// ColorWheel.FromHsv: без Clamp, Math.Round — банковское округление.
/// Каналы переводятся в байт так же, как `(byte)Math.Round(...)` в C# (см. [`dotnet_byte`]),
/// поэтому и при s, v вне 0…1 результат байт в байт как у оригинала.
pub fn from_hsv(h: f64, s: f64, v: f64) -> Rgb {
    let h = (h % 360.0 + 360.0) % 360.0;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        dotnet_byte((r + m) * 255.0),
        dotnet_byte((g + m) * 255.0),
        dotnet_byte((b + m) * 255.0),
    )
}

/// `(byte)Math.Round(x)` в .NET 8 на x64: банковское округление, затем перевод в int
/// (NaN и всё, что не помещается в int, даёт int.MinValue) и младшие 8 бит.
/// Для 0…255 это то же самое, что `x.round_ties_even() as u8`; разница только вне диапазона:
/// Rust `as u8` насыщает (−637.5 → 0), C# заворачивает (−637.5 → 130).
fn dotnet_byte(x: f64) -> u8 {
    let r = x.round_ties_even();
    let i = if (-2_147_483_648.0..=2_147_483_647.0).contains(&r) {
        r as i32
    } else {
        i32::MIN
    };
    i as u8
}

/// ColorWheel.ToHsv.
pub fn to_hsv(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let d = max - min;
    let mut h = 0.0;
    if d > 0.0 {
        h = if max == r {
            60.0 * ((g - b) / d % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
    }
    ((h + 360.0) % 360.0, if max <= 0.0 { 0.0 } else { d / max }, max)
}

/// ColorWheel.Hex: «#RRGGBB» заглавными.
pub fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_primaries() {
        for c in [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255), (0, 0, 0)] {
            let (h, s, v) = to_hsv(c);
            assert_eq!(from_hsv(h, s, v), c);
        }
        assert_eq!(hex((255, 126, 182)), "#FF7EB6");
    }

    /// Значения из записи C# (rust-anything record): s и v вне 0…1, NaN.
    #[test]
    fn out_of_range_like_dotnet() {
        assert_eq!(from_hsv(10.0, 3.0, 1_000_000.0), (192, 96, 128));
        assert_eq!(from_hsv(3.0, 2_147_483_648.0, -2.5), (130, 0, 0));
        assert_eq!(from_hsv(0.5, -0.0, -2.5), (130, 130, 130));
        assert_eq!(from_hsv(f64::NEG_INFINITY, -2.5, 1000.0), (24, 84, 0));
        assert_eq!(from_hsv(-1.0, -1.0, -2.5), (130, 5, 16));
    }
}
