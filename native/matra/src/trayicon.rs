//! The tray icon, and the provider marks the settings window shows.
//!
//! The brand mark, activity badges and compact opt-in Windows tray readings.

/// The application's own icon, at tray size.
///
/// Approved DYDXFX Logo-18, packaged by scripts/build-brand-icons.ps1.
/// The tray PNG, settings image and ICO all derive from the same unmodified master.
/// Windows does not tint tray icons; retain the supplied red and white artwork.
pub fn app_mark() -> Option<tauri::image::Image<'static>> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/tray-color.png")).ok()
}

/// The same icon as a `data:` URL, so the settings window shows what the taskbar shows.
pub fn app_mark_data_url() -> Option<String> {
    png_data_url(include_bytes!("../icons/tray-color.png"))
}

pub fn activity_mark(badge: &str, phase: u8, style: &str) -> Option<tauri::image::Image<'static>> {
    let image = app_mark()?;
    if badge == "idle" { return Some(image); }
    let (width, height) = (image.width(), image.height());
    let mut data = image.rgba().to_vec();
    let (cx, cy) = (width as i32 * 3 / 4, height as i32 * 3 / 4);
    let radius = (width as i32 / 6).max(3) + if badge == "busy" { phase as i32 } else { 0 };
    let color = match badge { "waiting" => [255, 183, 58, 255], "done" => [64, 210, 131, 255], _ => [121, 186, 255, 255] };
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let distance = (x - cx).pow(2) + (y - cy).pow(2);
            if distance <= (radius + 2).pow(2) {
                let offset = ((y as u32 * width + x as u32) * 4) as usize;
                let fill = if distance <= radius.pow(2) { color } else { [24, 22, 21, 255] };
                data[offset..offset + 4].copy_from_slice(&fill);
            }
        }
    }
    if badge == "busy" && style == "wave" {
        // Three visible bars distinguish Wave from Pulse, even when paused.
        for bar in -1i32..=1 {
            let half = if (bar == 0) == (phase == 0) { radius - 1 } else { (radius / 2).max(1) };
            for y in (cy - half)..=(cy + half) {
                let x = cx + bar * (radius / 2).max(2);
                if x >= 0 && y >= 0 && x < width as i32 && y < height as i32 {
                    let offset = ((y as u32 * width + x as u32) * 4) as usize;
                    data[offset..offset + 4].copy_from_slice(&[15, 36, 58, 255]);
                }
            }
        }
    }
    Some(tauri::image::Image::new_owned(data, width, height))
}

fn compact_reset(at: Option<u64>, now: u64) -> String {
    let Some(at) = at else { return "--".into() };
    let left = at.saturating_sub(now);
    if left == 0 { "0m".into() }
    else if left < 60_000 { "<1m".into() }
    else if left < 3_600_000 { format!("{}m", left.div_ceil(60_000)) }
    else if left < 86_400_000 { format!("{}h", left.div_ceil(3_600_000)) }
    else { format!("{}d", left.div_ceil(86_400_000).min(999)) }
}

fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [7,5,5,5,7], '1' => [2,6,2,2,7], '2' => [7,1,7,4,7],
        '3' => [7,1,7,1,7], '4' => [5,5,7,1,1], '5' => [7,4,7,1,7],
        '6' => [7,4,7,5,7], '7' => [7,1,1,1,1], '8' => [7,5,7,5,7],
        '9' => [7,5,7,1,7], '%' => [5,1,2,4,5], '<' => [1,2,4,2,1], '>' => [4,2,1,2,4],
        'h' => [4,4,6,5,5], 'm' => [0,0,7,7,5], 'd' => [1,1,3,5,3],
        _ => [0,0,7,0,0],
    }
}

fn draw_text(data: &mut [u8], text: &str, top: usize, color: [u8; 4]) {
    let chars: Vec<char> = text.chars().take(4).collect();
    let scale = 2;
    let width = chars.len() * 8 - 2;
    let left = (32 - width) / 2;
    for (index, c) in chars.iter().enumerate() {
        for (row, bits) in glyph(*c).into_iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) == 0 { continue; }
                for dy in 0..scale { for dx in 0..scale {
                    let x = left + index * 8 + column * scale + dx;
                    let y = top + row * scale + dy;
                    if x < 32 && y < 32 { data[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4].copy_from_slice(&color); }
                }}
            }
        }
    }
}

fn compact_usage(used: Option<f64>, remaining: bool) -> String {
    let Some(used) = used.filter(|v| v.is_finite() && *v >= 0.0) else { return "--".into() };
    let value = if remaining { 1.0 - used.clamp(0.0, 1.0) } else { used.clamp(0.0, 1.0) };
    if value > 0.0 && value < 0.01 { "<1%".into() }
    else if value < 1.0 && value > 0.99 { ">99%".into() }
    else { format!("{}%", (value * 100.0).round() as u32) }
}

fn reading_color(used: Option<f64>, colors: &crate::quota_colors::QuotaColors) -> [u8; 4] {
    let Some(value) = used else { return [189, 187, 182, 255] };
    let stops = [0.0, colors.watch, colors.critical, 0.90];
    let palette = [[85,213,143,255], [255,202,73,255], [255,154,69,255], [255,95,87,255]];
    let index = if value >= 0.90 { 3 } else if value >= colors.critical { 2 } else if value >= colors.watch { 1 } else { 0 };
    if colors.mode != "ramp" || index == 3 { return palette[index]; }
    let mix = ((value - stops[index]) / (stops[index+1] - stops[index])).clamp(0.0, 1.0);
    let mut result = [0,0,0,255];
    for channel in 0..3 {
        result[channel] = (palette[index][channel] as f64 * (1.0 - mix) + palette[index+1][channel] as f64 * mix).round() as u8;
    }
    result
}

pub fn reading(used: Option<f64>, reset: Option<u64>, now: u64,
               prefs: &crate::parity::ParitySettings, colors: &crate::quota_colors::QuotaColors) -> tauri::image::Image<'static> {
    let used = used.filter(|v| v.is_finite() && *v >= 0.0).map(|v| v.clamp(0.0, 1.0));
    let value = compact_usage(used, prefs.shows_remaining);
    let reset = compact_reset(reset, now);
    let color = reading_color(used, colors);
    let mut data = vec![0; 32 * 32 * 4];
    for y in 1..31 { for x in 1..31 {
        data[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4].copy_from_slice(&[24, 22, 21, 245]);
    }}
    match prefs.menu_bar_reading.as_str() {
        "usage" => draw_text(&mut data, &value, 11, color),
        "reset" => draw_text(&mut data, &reset, 11, [248, 244, 237, 255]),
        _ => { draw_text(&mut data, &value, 3, color); draw_text(&mut data, &reset, 19, [248, 244, 237, 255]); }
    }
    tauri::image::Image::new_owned(data, 32, 32)
}

/// Wraps PNG bytes that are already encoded (the bundled tray mark, or a provider's own icon) as a
/// `data:` URL.
pub fn png_data_url(png: &[u8]) -> Option<String> {
    if png.is_empty() {
        return None;
    }
    Some(format!("data:image/png;base64,{}", b64_encode(png)))
}

/// Base64 without pulling in a crate for it — the only encoder the app needs.
fn b64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unreadable_is_not_zero_and_nearly_spent_is_not_empty() {
        assert_eq!(compact_usage(None, false), "--");
        assert_eq!(compact_usage(Some(f64::NAN), false), "--");
        assert_eq!(compact_usage(Some(0.0), false), "0%");
        assert_eq!(compact_usage(Some(0.998), true), "<1%");
        assert_eq!(compact_usage(Some(0.998), false), ">99%");
        assert_eq!(compact_usage(Some(0.25), true), "75%");
    }
    #[test]
    fn reset_rounding_and_unknown_are_distinct() {
        assert_eq!(compact_reset(None, 0), "--");
        assert_eq!(compact_reset(Some(1), 0), "<1m");
        assert_eq!(compact_reset(Some(60_001), 0), "2m");
        assert_eq!(compact_reset(Some(0), 1), "0m");
    }
    #[test]
    fn glyphs_fit_and_badges_preserve_the_brand_outside_badge() {
        let base = app_mark().unwrap();
        let waiting = activity_mark("waiting",0,"pulse").unwrap();
        assert_eq!(base.width(), waiting.width());
        assert_eq!(&base.rgba()[..32], &waiting.rgba()[..32]);
        assert_ne!(base.rgba(), waiting.rgba());
        let pulse = activity_mark("busy",0,"pulse").unwrap();
        let wave = activity_mark("busy",0,"wave").unwrap();
        assert_ne!(pulse.rgba(), wave.rgba());
        let image = reading(Some(0.998),Some(3_600_000),0,&Default::default(),&Default::default());
        assert_eq!((image.width(), image.height(),image.rgba().len()),(32,32,4096));
    }
    #[test]
    fn ramp_interpolates_but_critical_color_does_not_follow_remaining_mode() {
        let colors = crate::quota_colors::QuotaColors { mode:"ramp".into(), ..Default::default() };
        assert_ne!(reading_color(Some(0.2), &colors),reading_color(Some(0.4), &colors));
        assert_eq!(reading_color(Some(1.0), &colors), [255,95,87,255]);
    }
}
