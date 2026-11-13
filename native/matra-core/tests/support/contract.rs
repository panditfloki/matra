use super::*;

fn rows(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect())
}
fn optional(text: &str) -> Option<f64> {
    if text == "-" || text == "_" {
        None
    } else {
        Some(text.parse().unwrap())
    }
}
fn close(actual: f64, expected: f64, name: &str) {
    assert!(
        (actual - expected).abs() <= 1e-10 * expected.abs().max(1.0),
        "{name}: {actual} != {expected}"
    );
}

#[test]
fn shared_pricing_vectors() {
    for r in rows(include_str!("../fixtures/pricing.tsv")) {
        let tokens = Tokens {
            input: r[3].parse().unwrap(),
            output: r[4].parse().unwrap(),
            cache_read: r[5].parse().unwrap(),
            cache_write: r[6].parse().unwrap(),
        };
        let matches = models_match(r[1], r[2]);
        assert_eq!(matches, r[14] == "1", "{}", r[0]);
        let rates = matches.then_some(Rates {
            input: r[7].parse().unwrap(),
            output: r[8].parse().unwrap(),
            cache_read: optional(r[9]),
            cache_write: optional(r[10]),
        });
        let result = local_cost(tokens, rates, r[11].parse().unwrap());
        match (result, optional(r[12])) {
            (Some(actual), Some(expected)) => close(actual, expected, r[0]),
            (None, None) => (),
            _ => panic!("{}: wrong known/unknown result", r[0]),
        }
        close(tokens.weight(), r[13].parse().unwrap(), r[0]);
    }
}

#[test]
fn shared_coverage_vectors() {
    for r in rows(include_str!("../fixtures/coverage.tsv")) {
        let mut sum = CostAccumulator::default();
        if r[1] != "-" {
            for value in r[1].split(',') {
                sum.add(optional(value));
            }
        }
        close(sum.amount, r[2].parse().unwrap(), r[0]);
        assert_eq!(sum.priced, r[3].parse::<u64>().unwrap(), "{}", r[0]);
        assert_eq!(sum.unknown, r[4].parse::<u64>().unwrap(), "{}", r[0]);
        assert_eq!(
            sum.coverage() as u32,
            r[5].parse::<u32>().unwrap(),
            "{}",
            r[0]
        );
    }
}

#[test]
fn shared_quota_vectors() {
    for r in rows(include_str!("../fixtures/quota.tsv")) {
        let windows: Vec<_> = if r[1] == "-" {
            vec![]
        } else {
            r[1].split('|')
                .map(|w| w.split_once(':').unwrap())
                .collect()
        };
        let headline = windows
            .iter()
            .map(|(id, balance)| (claude_headline_rank(id, *balance == "1"), *id))
            .filter(|(rank, _)| *rank != u32::MAX)
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, id)| id)
            .unwrap_or("-");
        assert_eq!(headline, r[2], "{}", r[0]);
        let mut order: Vec<_> = windows.iter().map(|(id, _)| *id).collect();
        order.sort_by(|a, b| claude_display_order(a, b));
        assert_eq!(
            if order.is_empty() {
                "-".into()
            } else {
                order.join("|")
            },
            r[3],
            "{}",
            r[0]
        );
    }
}

#[test]
fn ffi_rejects_invalid_utf8_and_null_nonempty_buffers() {
    let invalid = [0xff_u8];
    let good = b"model";
    unsafe {
        assert_eq!(
            matra_core_models_match(invalid.as_ptr(), 1, good.as_ptr(), good.len()),
            0
        );
        assert_eq!(
            matra_core_models_match(std::ptr::null(), 1, good.as_ptr(), good.len()),
            0
        );
        assert_eq!(
            matra_core_claude_headline_rank(std::ptr::null(), 1, 0),
            u32::MAX
        );
        assert_eq!(
            matra_core_models_match(std::ptr::null(), 0, std::ptr::null(), 0),
            1
        );
    }
}

#[test]
fn aggregation_merge_keeps_unknown_and_zero_distinct() {
    let mut left = CostAccumulator::default();
    left.add(Some(0.0));
    let mut right = CostAccumulator::default();
    right.add(None);
    right.add(Some(12.5));
    let combined = left.merge(right);
    assert_eq!(
        combined,
        CostAccumulator {
            amount: 12.5,
            priced: 2,
            unknown: 1
        }
    );
    assert_eq!(combined.coverage(), Coverage::AtLeast);
    let huge = CostAccumulator {
        amount: 1e308,
        priced: 1,
        unknown: 0,
    };
    assert_eq!(huge.merge(huge).coverage(), Coverage::AtLeast);
}

#[test]
fn native_mac_reference_pricing_and_weight_differential() {
    // Frozen pre-extraction formulas over deterministic synthetic inputs, separate from the core.
    for input in [0_u64, 1, 10_001, 1_000_000] {
        for output in [0_u64, 1, 203, 500_000] {
            for cached in [0_u64, 1, 71_117] {
                let tokens = Tokens {
                    input,
                    output,
                    cache_read: cached,
                    cache_write: cached / 2,
                };
                let rates = Rates {
                    input: 3.0,
                    output: 15.0,
                    cache_read: Some(0.3),
                    cache_write: Some(3.75),
                };
                let mut usd = input as f64 * 3.0 + output as f64 * 15.0;
                if cached != 0 {
                    usd += cached as f64 * 0.3;
                }
                if cached / 2 != 0 {
                    usd += (cached / 2) as f64 * 3.75;
                }
                let old_weight = input as f64
                    + output as f64 * 5.0
                    + cached as f64 * 0.1
                    + (cached / 2) as f64 * 1.25;
                assert_eq!(
                    local_cost(tokens, Some(rates), 83.4),
                    Some(usd / 1_000_000.0 * 83.4)
                );
                assert_eq!(tokens.weight(), old_weight);
            }
        }
    }
}
