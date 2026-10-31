//! A strict reader for the Prometheus text exposed by llama.cpp's and vLLM's `/metrics`. Anything it
//! cannot read with certainty is unavailable, never a guessed number.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct LlamaReading {
    pub total_tokens: i64,
    pub tokens_per_second: Option<f64>,
    pub active: Option<i64>,
    pub queued: Option<i64>,
}

fn closing_brace(text: &str) -> Option<usize> {
    let (mut quoted, mut escaped) = (false, false);
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '}' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

/// Sorted key/value pairs, so the same series in another label order still counts as a duplicate.
fn parse_labels(text: &str) -> Option<Vec<(String, String)>> {
    let chars: Vec<char> = text.chars().collect();
    let mut labels = Vec::new();
    let mut keys = BTreeSet::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        let first = *chars.get(start)?;
        if start == i || !(first.is_alphabetic() || first == '_') || chars.get(i) != Some(&'=') {
            return None;
        }
        let key: String = chars[start..i].iter().collect();
        if !keys.insert(key.clone()) {
            return None;
        }
        i += 1;
        if chars.get(i) != Some(&'"') {
            return None;
        }
        i += 1;
        let mut value = String::new();
        let mut closed = false;
        while i < chars.len() {
            let c = chars[i];
            i += 1;
            match c {
                '"' => {
                    closed = true;
                    break;
                }
                '\\' => {
                    match chars.get(i)? {
                        'n' => value.push('\n'),
                        '\\' => value.push('\\'),
                        '"' => value.push('"'),
                        _ => return None,
                    }
                    i += 1;
                }
                other => value.push(other),
            }
        }
        if !closed {
            return None;
        }
        labels.push((key, value));
        if i == chars.len() {
            break;
        }
        if chars[i] != ',' {
            return None;
        }
        i += 1;
        if i == chars.len() {
            return None;
        }
    }
    labels.sort();
    Some(labels)
}

fn samples(text: &str, names: &[&str]) -> Option<BTreeMap<String, Vec<String>>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut series: BTreeMap<String, BTreeSet<Vec<(String, String)>>> = BTreeMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(boundary) = line.find(|c: char| c == '{' || c.is_whitespace()) else {
            if names.contains(&line) {
                return None;
            }
            continue;
        };
        let name = &line[..boundary];
        if !names.contains(&name) {
            continue;
        }
        let mut rest = &line[boundary..];
        let mut labels = Vec::new();
        if let Some(inner) = rest.strip_prefix('{') {
            let close = closing_brace(inner)?;
            labels = parse_labels(&inner[..close])?;
            rest = &inner[close + 1..];
        }
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let timestamp_ok =
            fields.len() == 1 || (fields.len() == 2 && fields[1].parse::<f64>().is_ok_and(f64::is_finite));
        if !timestamp_ok || !series.entry(name.to_string()).or_default().insert(labels) {
            return None;
        }
        out.entry(name.to_string()).or_default().push(fields[0].to_string());
    }
    Some(out)
}

fn integer_sample(sample: &str) -> Option<i64> {
    if let Ok(value) = sample.parse::<i64>() {
        return (value >= 0).then_some(value);
    }
    let value: f64 = sample.parse().ok()?;
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value < 9e15).then_some(value as i64)
}

/// The sum of two counter families. Both must be present, and an unreadable sample voids the total.
pub fn counters(text: &str, prompt: &str, completion: &str) -> Option<i64> {
    let found = samples(text, &[prompt, completion])?;
    let total = |name: &str| -> Option<i64> {
        found
            .get(name)?
            .iter()
            .try_fold(0i64, |sum, sample| sum.checked_add(integer_sample(sample)?))
    };
    total(prompt)?.checked_add(total(completion)?)
}

/// An average cannot be summed across labelled series, so a gauge with several is unavailable.
pub fn gauge(text: &str, name: &str) -> Option<f64> {
    let found = samples(text, &[name])?;
    let values = found.get(name)?;
    if values.len() != 1 {
        return None;
    }
    let value: f64 = values[0].parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

pub fn llama(text: &str) -> Option<LlamaReading> {
    let exact = |v: f64| (v.fract() == 0.0).then_some(v as i64);
    Some(LlamaReading {
        total_tokens: counters(text, "llamacpp:prompt_tokens_total", "llamacpp:tokens_predicted_total")?,
        tokens_per_second: gauge(text, "llamacpp:predicted_tokens_seconds"),
        active: gauge(text, "llamacpp:requests_processing").and_then(exact),
        queued: gauge(text, "llamacpp:requests_deferred").and_then(exact),
    })
}

pub fn vllm(text: &str) -> Option<i64> {
    counters(text, "vllm:prompt_tokens_total", "vllm:generation_tokens_total")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LLAMA: &str = "# HELP llamacpp:prompt_tokens_total Number\n\
        # TYPE llamacpp:prompt_tokens_total counter\n\
        llamacpp:prompt_tokens_total 1000\n\
        llamacpp:prompt_seconds_total 3.5\n\
        llamacpp:tokens_predicted_total 234\n\
        llamacpp:predicted_tokens_seconds 41.25\n\
        llamacpp:requests_processing 1\n\
        llamacpp:requests_deferred 0\n";

    #[test]
    fn reads_llama_cpp_metrics() {
        let r = llama(LLAMA).unwrap();
        assert_eq!(r.total_tokens, 1234);
        assert_eq!(r.tokens_per_second, Some(41.25));
        assert_eq!(r.active, Some(1));
        assert_eq!(r.queued, Some(0));
    }

    #[test]
    fn a_missing_gauge_is_unavailable_while_the_total_survives() {
        let text = "llamacpp:prompt_tokens_total 1\nllamacpp:tokens_predicted_total 2\n";
        let r = llama(text).unwrap();
        assert_eq!(r.total_tokens, 3);
        assert_eq!(r.tokens_per_second, None);
        assert_eq!(r.active, None);
    }

    #[test]
    fn a_missing_counter_voids_the_reading() {
        assert!(llama("llamacpp:prompt_tokens_total 1\n").is_none());
        assert!(llama("<html>nope</html>").is_none());
    }

    #[test]
    fn labelled_series_sum_and_averages_do_not() {
        let text = "vllm:prompt_tokens_total{model=\"a\"} 10\nvllm:prompt_tokens_total{model=\"b\"} 5\n\
                    vllm:generation_tokens_total{model=\"a\"} 1 1700000000\n";
        assert_eq!(vllm(text), Some(16));
        let gauges = "g{a=\"1\"} 2\ng{a=\"2\"} 4\n";
        assert_eq!(gauge(gauges, "g"), None);
    }

    #[test]
    fn duplicates_bad_labels_and_negatives_are_refused() {
        assert_eq!(counters("a 1\na 2\nb 1\n", "a", "b"), None);
        assert_eq!(counters("a{x=\"1\",y=\"2\"} 1\na{y=\"2\",x=\"1\"} 2\nb 1\n", "a", "b"), None);
        assert_eq!(counters("a{x=1} 1\nb 1\n", "a", "b"), None);
        assert_eq!(counters("a{x=\"1\",x=\"2\"} 1\nb 1\n", "a", "b"), None);
        assert_eq!(counters("a -1\nb 1\n", "a", "b"), None);
        assert_eq!(counters("a 1.5\nb 1\n", "a", "b"), None);
        assert_eq!(counters("a 9223372036854775807\nb 1\n", "a", "b"), None);
        assert_eq!(gauge("g NaN\n", "g"), None);
    }

    #[test]
    fn escaped_label_values_do_not_end_early() {
        assert_eq!(counters("a{x=\"}\\\"\\n\"} 2\nb 1\n", "a", "b"), Some(3));
    }
}
