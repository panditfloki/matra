use serde::{Deserialize, Serialize};

/// Fractional usage, independent of app accent and notification delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuotaColors {
    pub watch: f64,
    pub critical: f64,
    pub mode: String,
}

impl Default for QuotaColors {
    fn default() -> Self {
        Self {
            watch: 0.5,
            critical: 0.7,
            mode: "step".into(),
        }
    }
}

impl QuotaColors {
    pub fn validate(&self) -> Result<(), String> {
        if !self.watch.is_finite()
            || !self.critical.is_finite()
            || self.watch < 0.01
            || self.watch > 0.88
            || self.critical < self.watch + 0.009999
            || self.critical > 0.89
        {
            return Err(
                "Watch must be 1-88%, with Critical at least 1% higher and below 90%.".into(),
            );
        }
        if !matches!(self.mode.as_str(), "step" | "ramp") {
            return Err("Unknown quota colour mode.".into());
        }
        Ok(())
    }

    pub fn sanitized(self) -> Self {
        if self.validate().is_ok() {
            self
        } else {
            Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backward_compatible_and_round_trip() {
        assert_eq!(
            serde_json::from_str::<QuotaColors>("{}").unwrap(),
            QuotaColors::default()
        );
        let custom = QuotaColors {
            watch: 0.4,
            critical: 0.8,
            mode: "ramp".into(),
        };
        assert!(custom.validate().is_ok());
        assert_eq!(
            serde_json::from_value::<QuotaColors>(serde_json::to_value(&custom).unwrap()).unwrap(),
            custom
        );
    }
    #[test]
    fn rejects_inverted_nonfinite_and_red_zone_thresholds() {
        for (watch, critical) in [
            (0.7, 0.5),
            (0.5, 0.5),
            (0.0, 0.7),
            (0.5, 0.9),
            (f64::NAN, 0.7),
        ] {
            let value = QuotaColors {
                watch,
                critical,
                mode: "step".into(),
            };
            assert!(value.validate().is_err());
            assert_eq!(value.sanitized(), QuotaColors::default());
        }
        assert!(QuotaColors {
            mode: "accent".into(),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
