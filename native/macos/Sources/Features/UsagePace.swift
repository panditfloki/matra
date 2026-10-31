import Foundation

struct UsagePace {
    /// Used quota minus elapsed time, expressed in percentage points.
    ///
    /// Positive is a deficit (burning too fast, runs out early); negative is
    /// held in reserve. `burnPoints` flips the sign, so the reserve that is
    /// about to expire unused reads positive for the burn-deficit sort.
    let percentagePoints: Double

    var isDeficit: Bool { percentagePoints > 0 }

    /// The waste risk, in points: remaining quota minus remaining time.
    /// Positive is urgent (much left, little time), negative means ahead
    /// of burn (in deficit). Nil pace has no burn figure.
    var burnPoints: Double { -percentagePoints }

    var isBurnUrgent: Bool { burnPoints > 0 }

    var summary: String {
        let magnitude = abs(percentagePoints)
        let rounded = (magnitude * 10).rounded() / 10
        let value = rounded == 0 && magnitude > 0
            ? "<0.1"
            : String(format: "%.1f", locale: Locale(identifier: "en_US_POSIX"), rounded)
                .replacingOccurrences(of: ".0", with: "")
        return isDeficit
            ? L10n.t("\(value)% deficit")
            : L10n.t("\(value)% reserved")
    }
}

extension LimitWindow {
    /// Share of the window's cycle elapsed, from 0 at reset to just under 1 —
    /// where the fill *should* be. Nil when the window never says when it
    /// rolls, so there is no cycle to be through.
    func elapsedFraction(now: Date) -> Double? {
        guard let duration, duration.isFinite, duration > 0,
              let resetsAt else { return nil }
        let remainingTime = resetsAt.timeIntervalSince(now)
        guard remainingTime.isFinite, remainingTime > 0 else { return nil }
        // Provider and device clocks can put a fresh reset just beyond one full cycle.
        return 1 - min(remainingTime / duration, 1)
    }

    /// Where the card's pace tick sits on the window's bar, 0...1 from the
    /// leading edge: the elapsed share when the bar reads used, mirrored to
    /// the expected-remaining share when it reads remaining. Nil exactly when
    /// there is no cycle to mark.
    func paceMarkerFraction(now: Date, showingRemaining: Bool) -> Double? {
        guard let elapsed = elapsedFraction(now: now) else { return nil }
        return min(max(Percent.metered(elapsed, showingRemaining: showingRemaining), 0), 1)
    }

    func usagePace(now: Date) -> UsagePace? {
        guard let usedFraction, usedFraction.isFinite, usedFraction >= 0,
              let elapsedFraction = elapsedFraction(now: now) else { return nil }
        // Once the allowance is exhausted there is no negative quota to account for.
        let spentFraction = min(usedFraction, 1)
        return UsagePace(
            percentagePoints: (spentFraction - elapsedFraction) * 100
        )
    }

    /// The burn deficit for this window, in points: remaining quota minus
    /// remaining time. Positive means quota will expire unused unless burn
    /// accelerates (the 70%-left-with-23h-left case). Nil exactly when
    /// there is no cycle to measure against — the same windows pace skips.
    func burnDeficitPoints(now: Date) -> Double? {
        usagePace(now: now).map(\.burnPoints)
    }
}

extension ProviderSnapshot {
    /// The most urgent burn deficit across every timed window, in points.
    ///
    /// The max, not the headline: a weekly allowance expiring in 23h is
    /// urgent even where the 5-hour session beside it is comfortable, and
    /// sorting by the headline alone would bury it. Nil when no window
    /// carries a clock — nothing timed, nothing to burn down.
    func maxBurnDeficit(now: Date) -> Double? {
        windows.compactMap { $0.burnDeficitPoints(now: now) }.max()
    }
}

/// Where a window ends up if it keeps being used at the rate it has been so far.
struct UsageProjection {
    /// The share of the limit used by the reset, at the current rate. Above 1
    /// when the limit runs out first.
    let fractionAtReset: Double
    /// When the limit runs out at the current rate, if that is before the reset.
    let limitReachedAt: Date?

    var runsOut: Bool { limitReachedAt != nil }
}

extension LimitWindow {
    /// Straight-line: what has been used, over the share of the window gone.
    ///
    /// Nil for the first fifteenth of a window — two days of a month. A rate
    /// taken from a few hours is a rate taken from one busy morning, and a
    /// warning colour that flickers on with the first request of the month and
    /// off again by lunchtime trains people to ignore it.
    func usageProjection(now: Date) -> UsageProjection? {
        guard let usedFraction, usedFraction.isFinite, usedFraction >= 0,
              // Spent already: nothing left to forecast.
              usedFraction < 1,
              let duration, duration.isFinite, duration > 0,
              let resetsAt else { return nil }
        let remainingTime = resetsAt.timeIntervalSince(now)
        guard remainingTime.isFinite, remainingTime > 0 else { return nil }
        let elapsed = duration - min(remainingTime, duration)
        guard elapsed >= duration / 15 else { return nil }

        let ratePerSecond = usedFraction / elapsed
        let atReset = usedFraction + ratePerSecond * remainingTime
        let reachedAt = atReset > 1
            ? now.addingTimeInterval((1 - usedFraction) / ratePerSecond)
            : nil
        return UsageProjection(fractionAtReset: atReset, limitReachedAt: reachedAt)
    }
}
