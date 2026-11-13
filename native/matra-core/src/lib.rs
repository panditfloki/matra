//! Platform-independent business rules. No storage, credentials, network, clock or UI.
//! Rust callers use this crate directly; native Swift calls the same functions through the C ABI.
use std::cmp::Ordering;

pub const ABI_VERSION: u32 = 1;
pub const WEIGHT_OUTPUT: f64 = 5.0;
pub const WEIGHT_CACHE_READ: f64 = 0.1;
pub const WEIGHT_CACHE_WRITE: f64 = 1.25;

/// Exact model identity only. A context/version suffix must never inherit another model's price.
pub fn normalize_model(model: &str) -> String {
    let bare = model
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(model);
    bare.to_lowercase().replace('.', "-")
}

pub fn models_match(left: &str, right: &str) -> bool {
    normalize_model(left) == normalize_model(right)
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Tokens {
    pub fn is_empty(self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_read == 0 && self.cache_write == 0
    }
    pub fn weight(self) -> f64 {
        self.input as f64
            + self.output as f64 * WEIGHT_OUTPUT
            + self.cache_read as f64 * WEIGHT_CACHE_READ
            + self.cache_write as f64 * WEIGHT_CACHE_WRITE
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

pub fn valid_amount(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

/// INR at API list price. Missing cache rates matter only when that cache lane was used.
/// Zero tokens are a real zero even when the model or exchange rate is unknown.
pub fn local_cost(tokens: Tokens, rates: Option<Rates>, usd_to_inr: f64) -> Option<f64> {
    if tokens.is_empty() {
        return Some(0.0);
    }
    if !usd_to_inr.is_finite() || usd_to_inr <= 0.0 {
        return None;
    }
    let rates = rates?;
    if !valid_amount(rates.input) || !valid_amount(rates.output) {
        return None;
    }
    let mut usd = tokens.input as f64 * rates.input + tokens.output as f64 * rates.output;
    if tokens.cache_read != 0 {
        let rate = rates.cache_read.filter(|r| valid_amount(*r))?;
        usd += tokens.cache_read as f64 * rate;
    }
    if tokens.cache_write != 0 {
        let rate = rates.cache_write.filter(|r| valid_amount(*r))?;
        usd += tokens.cache_write as f64 * rate;
    }
    let amount = usd / 1_000_000.0 * usd_to_inr;
    valid_amount(amount).then_some(amount)
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    None = 0,
    Complete = 1,
    AtLeast = 2,
    Unavailable = 3,
}

pub fn coverage(priced: u64, unknown: u64) -> Coverage {
    match (priced, unknown) {
        (0, 0) => Coverage::None,
        (_, 0) => Coverage::Complete,
        (0, _) => Coverage::Unavailable,
        _ => Coverage::AtLeast,
    }
}

/// The same aggregation transition is used by both production accounting adapters.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CostAccumulator {
    pub amount: f64,
    pub priced: u64,
    pub unknown: u64,
}

impl CostAccumulator {
    pub fn add(&mut self, cost: Option<f64>) {
        match cost.filter(|c| valid_amount(*c) && valid_amount(self.amount + *c)) {
            Some(cost) => {
                self.amount += cost;
                self.priced = self.priced.saturating_add(1);
            }
            None => self.unknown = self.unknown.saturating_add(1),
        }
    }
    pub fn merge(self, other: Self) -> Self {
        if valid_amount(self.amount + other.amount) {
            Self {
                amount: self.amount + other.amount,
                priced: self.priced.saturating_add(other.priced),
                unknown: self.unknown.saturating_add(other.unknown),
            }
        } else {
            // Keep the representable lower bound and mark the other turns unknown.
            Self {
                amount: self.amount,
                priced: self.priced,
                unknown: self
                    .unknown
                    .saturating_add(other.unknown)
                    .saturating_add(other.priced),
            }
        }
    }
    pub fn coverage(self) -> Coverage {
        coverage(self.priced, self.unknown)
    }
}

/// Account-wide quota wins over a model-specific cap. The aliases are normalized at this seam,
/// before Windows adds account suffixes; never remove an account suffix here.
/// An unrecognized/model-scoped window is not a substitute for an absent account allowance.
pub fn claude_headline_rank(id: &str, is_balance: bool) -> u32 {
    match id {
        "session" => 0,
        "five_hour" => 1,
        "seven_day" => 2,
        "weekly_all" => 3,
        "weekly" => 4,
        _ if is_balance => 5,
        _ => u32::MAX,
    }
}

pub fn claude_display_rank(id: &str) -> u32 {
    match id {
        "session" | "five_hour" => 0,
        "weekly_all" | "seven_day" | "weekly" => 1,
        "spend" => 3,
        _ => 2,
    }
}

pub fn claude_display_order(left: &str, right: &str) -> Ordering {
    claude_display_rank(left)
        .cmp(&claude_display_rank(right))
        .then_with(|| left.cmp(right))
}

// C ABI: fixed-width numeric fields and borrowed UTF-8 buffers only. No allocation crosses FFI.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OptionalAmount {
    pub value: f64,
    pub known: u32,
}
impl From<Option<f64>> for OptionalAmount {
    fn from(value: Option<f64>) -> Self {
        Self {
            value: value.unwrap_or(0.0),
            known: u32::from(value.is_some()),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PriceRates {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub has_cache_read: u32,
    pub has_cache_write: u32,
}

// The caller owns all buffers for the duration of a call. Null is allowed only for an empty buffer.
unsafe fn text<'a>(data: *const u8, len: usize) -> Option<&'a str> {
    if len == 0 {
        return Some("");
    }
    if data.is_null() || len > isize::MAX as usize {
        return None;
    }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(data, len) }).ok()
}

#[no_mangle]
pub extern "C" fn matra_core_abi_version() -> u32 {
    ABI_VERSION
}
#[no_mangle]
pub extern "C" fn matra_core_token_weight(tokens: Tokens) -> f64 {
    tokens.weight()
}
#[no_mangle]
pub extern "C" fn matra_core_weight_output() -> f64 {
    WEIGHT_OUTPUT
}
#[no_mangle]
pub extern "C" fn matra_core_weight_cache_read() -> f64 {
    WEIGHT_CACHE_READ
}
#[no_mangle]
pub extern "C" fn matra_core_weight_cache_write() -> f64 {
    WEIGHT_CACHE_WRITE
}

/// # Safety
/// Each non-null pointer must reference at least its supplied length of readable bytes.
#[no_mangle]
pub unsafe extern "C" fn matra_core_models_match(
    left: *const u8,
    left_len: usize,
    right: *const u8,
    right_len: usize,
) -> u32 {
    match (unsafe { text(left, left_len) }, unsafe {
        text(right, right_len)
    }) {
        (Some(left), Some(right)) => u32::from(models_match(left, right)),
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn matra_core_local_cost(
    tokens: Tokens,
    rates: PriceRates,
    has_price: u32,
    usd_to_inr: f64,
) -> OptionalAmount {
    let rates = (has_price != 0).then_some(Rates {
        input: rates.input,
        output: rates.output,
        cache_read: (rates.has_cache_read != 0).then_some(rates.cache_read),
        cache_write: (rates.has_cache_write != 0).then_some(rates.cache_write),
    });
    local_cost(tokens, rates, usd_to_inr).into()
}

#[no_mangle]
pub extern "C" fn matra_core_cost_add(
    mut accumulator: CostAccumulator,
    cost: OptionalAmount,
) -> CostAccumulator {
    accumulator.add((cost.known != 0).then_some(cost.value));
    accumulator
}
#[no_mangle]
pub extern "C" fn matra_core_cost_merge(
    left: CostAccumulator,
    right: CostAccumulator,
) -> CostAccumulator {
    left.merge(right)
}
#[no_mangle]
pub extern "C" fn matra_core_coverage(priced: u64, unknown: u64) -> u32 {
    coverage(priced, unknown) as u32
}

/// # Safety
/// `id` must reference `len` readable bytes (or be null with len == 0).
#[no_mangle]
pub unsafe extern "C" fn matra_core_claude_headline_rank(
    id: *const u8,
    len: usize,
    is_balance: u32,
) -> u32 {
    unsafe { text(id, len) }
        .map(|id| claude_headline_rank(id, is_balance != 0))
        .unwrap_or(u32::MAX)
}
/// # Safety
/// `id` must reference `len` readable bytes (or be null with len == 0).
#[no_mangle]
pub unsafe extern "C" fn matra_core_claude_display_rank(id: *const u8, len: usize) -> u32 {
    unsafe { text(id, len) }
        .map(claude_display_rank)
        .unwrap_or(2)
}

#[cfg(test)]
#[path = "../tests/support/contract.rs"]
mod contract_tests;
