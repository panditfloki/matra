use super::*;
use regex::Regex;
pub const URL: &str = "https://ampcode.com/api/internal";
fn captures(pattern: &str, text: &str) -> Option<Vec<String>> {
    let re = Regex::new(pattern).ok()?;
    let c = re.captures(text)?;
    Some(
        c.iter()
            .skip(1)
            .map(|m| m.map_or("", |m| m.as_str()).to_string())
            .collect(),
    )
}
fn valid_balance(left: &str, total: &str) -> bool {
    let n = |s: &str| s.replace(',', "").parse::<f64>().ok();
    match (n(left), n(total)) {
        (Some(a), Some(b)) => a.is_finite() && b.is_finite() && a >= 0. && b > 0. && a <= b,
        _ => false,
    }
}
pub fn read(url: &str, c: &Credential) -> Result<Reading, Failure> {
    parse(&request(
        "POST",
        url,
        vec![net::bearer(&c.token)],
        Some(r#"{"jsonrpc":"2.0","method":"userDisplayBalanceInfo","params":{},"id":1}"#),
        "Amp",
        true,
    )?)
}
pub fn parse(root: &Value) -> Result<Reading, Failure> {
    if root.get("ok").is_some_and(|v| v.as_bool() != Some(true))
        || root.get("error").is_some_and(|v| !v.is_null())
    {
        return Err(invalid("Amp"));
    }
    let payload = root.get("result").unwrap_or(root);
    let text = payload["displayText"]
        .as_str()
        .ok_or_else(|| invalid("Amp"))?
        .replace("**", "");
    let tier = captures(
        r"(?im)^\s*Amp[ \t]+([^\r\n:]+?)[ \t]+Tier:[ \t]+agent usage \$([0-9]+(?:,[0-9]{3})*(?:\.[0-9]+)?) of \$([0-9]+(?:,[0-9]{3})*(?:\.[0-9]+)?) remaining \(([0-9]+(?:\.[0-9]+)?)%\), orb usage ([0-9]+(?:,[0-9]{3})*(?:\.[0-9]+)?)h of ([0-9]+(?:,[0-9]{3})*(?:\.[0-9]+)?)h (?:[a-z0-9._-]+ )?orb hours remaining \(([0-9]+(?:\.[0-9]+)?)%\)",
        &text,
    );
    let fields = if let Some(f) = tier {
        if !valid_balance(&f[1], &f[2]) || !valid_balance(&f[4], &f[5]) {
            return Err(invalid("Amp"));
        }
        Some(vec![f[0].clone(), f[3].clone(), f[6].clone()])
    } else {
        captures(
            r"(?im)^\s*Amp[ \t]+([^\r\n:]+?)[ \t]+Subscription:[ \t]+([0-9]+(?:\.[0-9]+)?)%[ \t]+(?:other|agent)[ \t]+usage[ \t]+and[ \t]+([0-9]+(?:\.[0-9]+)?)%[ \t]+orb[ \t]+usage[ \t]+remaining\b",
            &text,
        )
    };
    let mut r = Reading::default();
    if let Some(f) = fields {
        let a = f[1].parse::<f64>().map_err(|_| invalid("Amp"))?;
        let b = f[2].parse::<f64>().map_err(|_| invalid("Amp"))?;
        if !(0.0..=100.0).contains(&a) || !(0.0..=100.0).contains(&b) {
            return Err(invalid("Amp"));
        }
        r.plan = Some(f[0].clone());
        r.windows = vec![
            fraction("agent", "Agent usage", (100. - a) / 100., None),
            fraction("orb", "Orb usage", (100. - b) / 100., None),
        ];
        if let Some(days) = captures(r"resets upon renewal in ([0-9]+) days?\b", &text) {
            textual(
                "renewal",
                "Renewal",
                format!("About {} days", days[0]),
                None,
                &mut r,
            );
        }
        return Ok(r);
    }
    let lower = text.to_lowercase();
    if lower.contains("subscription:") || lower.contains("tier:") {
        return Err(invalid("Amp"));
    }
    if let Some(f) = captures(
        r"(?im)^\s*Amp Free:[ \t]+\$([0-9]+(?:\.[0-9]+)?)/\$([0-9]+(?:\.[0-9]+)?)[ \t]+remaining[ \t]+\(replenishes[ \t]+\+\$([0-9]+(?:\.[0-9]+)?)/hour\)",
        &text,
    ) {
        if !valid_balance(&f[0], &f[1]) {
            return Err(invalid("Amp"));
        }
        let a: f64 = f[0].parse().unwrap();
        let b: f64 = f[1].parse().unwrap();
        let rate: f64 = f[2].parse().map_err(|_| invalid("Amp"))?;
        if !rate.is_finite() {
            return Err(invalid("Amp"));
        }
        r.plan = Some("Free".into());
        let mut w = fraction("free", "Free allowance", (b - a) / b, None);
        w.derived = true;
        r.windows.push(w);
        textual(
            "freeBalance",
            "Free balance",
            format!("${a:.2} of ${b:.2} left"),
            None,
            &mut r,
        );
        textual(
            "replenishment",
            "Replenishment",
            format!("${rate:.2}/hour"),
            None,
            &mut r,
        );
        return Ok(r);
    }
    Err(invalid("Amp"))
}
