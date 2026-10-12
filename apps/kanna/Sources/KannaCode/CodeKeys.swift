import AppKit

public enum CodeMove: Equatable {
    case lines(Int)
    case pages(Double)
    case top
    case bottom
    case blank(Int)
    case scope(Int)
    case find(forward: Bool)
    case match(forward: Bool)
    case stopFinding
}

public struct CodeKeys {
    private var count = ""
    private var pending: Character?

    public init() {}

    public mutating func press(_ characters: String, control: Bool) -> CodeMove? {
        guard let key = characters.first, characters.count == 1 else { reset(); return nil }
        if control {
            defer { reset() }
            let times = Double(Int(count) ?? 1)
            switch key {
            case "e": return .lines(Int(times))
            case "y": return .lines(-Int(times))
            case "d": return .pages(0.5 * times)
            case "u": return .pages(-0.5 * times)
            case "f": return .pages(times)
            case "b": return .pages(-times)
            default: return nil
            }
        }
        if let first = pending {
            pending = nil
            defer { reset() }
            switch (first, key) {
            case ("g", "g"): return .top
            case ("]", "]"): return .scope(times)
            case ("[", "["): return .scope(-times)
            default: return nil
            }
        }
        if key.isNumber, key != "0" || !count.isEmpty {
            count.append(key)
            return nil
        }
        switch key {
        case "g", "]", "[":
            if key == "g", Int(count) == nil { pending = "g"; return nil }
            pending = key
            return nil
        default: break
        }
        defer { reset() }
        switch key {
        case "j", "\u{F701}": return .lines(times)
        case "k", "\u{F700}": return .lines(-times)
        case "d": return .pages(0.5 * Double(times))
        case "u": return .pages(-0.5 * Double(times))
        case "f", " ": return .pages(Double(times))
        case "b": return .pages(-Double(times))
        case "G": return .bottom
        case "}": return .blank(times)
        case "{": return .blank(-times)
        case "/": return .find(forward: true)
        case "?": return .find(forward: false)
        case "n": return .match(forward: true)
        case "N": return .match(forward: false)
        case "\u{1b}": return .stopFinding
        default: return nil
        }
    }

    private var times: Int { max(1, Int(count) ?? 1) }

    private mutating func reset() {
        count = ""
        pending = nil
    }
}
