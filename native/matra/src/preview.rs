//! Deterministic, labelled fixtures. No provider/profile reader is called from this module.
pub fn usage(id: &str) -> crate::usage::UsageSnapshot {
    use crate::usage::{LimitWindow, UsageSnapshot};
    // Include a genuine zero-used sample and an exhausted sample: zero is a reported reading,
    // while 100% used means spent. Neither is an absent/unknown quota.
    let used = match id { "claude" => 0.32, "codex" => 0.58, "cursor" => 0.0,
        "grok" => 1.0, "gemini" => 0.43, "glm" => 0.26, _ => 0.38 };
    let now = crate::now_ms();
    let shapes: &[(&str, &str)] = match id {
        "codex" => &[("primary", "Sample session"), ("secondary", "Sample week")],
        "cursor" => &[("included", "Sample included usage"), ("api", "Sample API usage")],
        "grok" | "higgsfield" => &[("credits", "Sample credits")],
        "glm" => &[("session", "Sample session"), ("weekly", "Sample week")],
        "gemini" => &[("gemini-5h", "Sample 5-hour session"), ("gemini-weekly", "Sample weekly quota")],
        "copilot" => &[("premium_interactions", "Sample premium interactions")],
        "opencode" | "kimi" => &[("rolling", "Sample rolling quota")],
        "commandcode" => &[("monthly", "Sample monthly quota")],
        "grokbot" => &[("weekly", "Sample weekly quota")],
        _ => &[("session", "Sample session"), ("seven_day", "Sample week")],
    };
    let windows = shapes.iter().enumerate().map(|(index, (window_id, label))| LimitWindow {
        id: (*window_id).into(), label: format!("{label} · synthetic"),
        used: if index == 0 { used } else if used == 0.0 { 0.07 } else { used * 0.7 },
        resets_at: Some(now + if index == 0 { 2 * 60 * 60 * 1000 } else { 4 * 24 * 60 * 60 * 1000 }),
        group: Some("SYNTHETIC PREVIEW".into()), ..Default::default()
    }).collect();
    UsageSnapshot {
        status: "ok".into(),
        note: "SYNTHETIC PREVIEW: sample readings only; no account or provider was accessed.".into(),
        fetched_at: now,
        windows, ..Default::default()
    }
}

pub fn or_else(id: &str, live: impl FnOnce() -> crate::usage::UsageSnapshot) -> crate::usage::UsageSnapshot {
    if crate::platform::preview() { usage(id) } else { live() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_fixture_identifies_itself_as_synthetic() {
        for id in crate::TRAY_PROVIDER_IDS {
            let sample = super::usage(id);
            assert!(sample.note.contains("SYNTHETIC PREVIEW"));
            assert!(sample.windows.iter().all(|w| w.label.contains("synthetic") && w.group.as_deref() == Some("SYNTHETIC PREVIEW")));
        }
    }

    #[test]
    fn every_sample_has_the_headline_window_the_actual_ring_selector_requires() {
        for id in crate::TRAY_PROVIDER_IDS {
            let sample = super::usage(id);
            let headline = crate::ring_window(id, &sample.windows, "automatic", "gemini")
                .unwrap_or_else(|| panic!("{id} sample has no selectable ring"));
            assert!(headline.used.is_finite() && (0.0..=1.0).contains(&headline.used));
            assert!(headline.count.is_none());
        }
        let cursor = super::usage("cursor");
        assert_eq!(crate::ring_window("cursor", &cursor.windows, "automatic", "gemini").unwrap().used, 0.0);
        let grok = super::usage("grok");
        assert_eq!(crate::ring_window("grok", &grok.windows, "automatic", "gemini").unwrap().used, 1.0);
    }
}
