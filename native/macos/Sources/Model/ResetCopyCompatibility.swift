import Foundation

extension ResetCopy {
    /// Mātrā's owner-selected Reset date/Time remaining preference remains the
    /// authority. Shared rows must not append a second timestamp to that copy.
    static func text(for resetsAt: Date, now: Date = Date(), calendar: Calendar = .current,
                     format: ResetTimeFormat = .automatic, withAbsoluteStamp: Bool,
                     locale: Locale = L10n.locale) -> String {
        text(for: resetsAt, now: now, calendar: calendar, format: format, locale: locale)
    }

    static func formatter(template: String, calendar: Calendar = .current,
                          locale: Locale = L10n.locale) -> DateFormatter {
        let result = formatter(for: calendar)
        result.locale = locale
        result.setLocalizedDateFormatFromTemplate(template)
        return result
    }
}
