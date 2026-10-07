import Foundation

public enum Fraction {
    /// Parses "1/3", "25%", "0.25" or a JSON number.
    public static func parse(_ value: JSON) -> Double? {
        if let number = value.number { return number }
        guard let text = value.string?.trimmingCharacters(in: .whitespaces) else { return nil }
        if text.hasSuffix("%") { return Double(text.dropLast()).map { $0 / 100 } }
        let parts = text.split(separator: "/", omittingEmptySubsequences: false)
        if parts.count == 2, let n = Double(parts[0]), let d = Double(parts[1]), d != 0 { return n / d }
        return parts.count == 1 ? Double(text) : nil
    }

    /// The simplest fraction within 0.4% (denominators up to 8), else a percentage.
    public static func format(_ value: Double) -> String {
        if abs(value - 1) < 1e-6 { return "1" }
        for d in 2...8 {
            let n = (value * Double(d)).rounded()
            if n > 0, abs(value - n / Double(d)) < 0.004 { return "\(Int(n))/\(d)" }
        }
        return "\(Int((value * 100).rounded()))%"
    }
}
