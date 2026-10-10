import Foundation

public enum DropText {
    static let special: Set<Character> = Set("\\ ()[]{}<>\"'`!#$&;|*?\t")

    public static func escape(_ path: String) -> String {
        if path.unicodeScalars.contains(where: isControl) { return quoted(path) }
        var out = ""
        for character in path {
            if special.contains(character) { out.append("\\") }
            out.append(character)
        }
        return out
    }

    static func isControl(_ scalar: Unicode.Scalar) -> Bool {
        scalar.value < 0x20 && scalar != "\t" || (0x7f...0x9f).contains(scalar.value)
    }

    static func quoted(_ path: String) -> String {
        var out = "$'"
        for scalar in path.unicodeScalars {
            switch scalar {
            case "\\": out += "\\\\"
            case "'": out += "\\'"
            case _ where isControl(scalar) || scalar == "\t":
                for byte in String(scalar).utf8 { out += String(format: "\\x%02x", byte) }
            default: out.unicodeScalars.append(scalar)
            }
        }
        return out + "'"
    }

    public static func paths(_ paths: [String]) -> String {
        paths.map(escape).joined(separator: " ")
    }

    public static func folder() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appending(path: "kanna-drops", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    public static func imageFile(in folder: URL, date: Date = Date()) -> URL {
        let format = DateFormatter()
        format.locale = Locale(identifier: "en_US_POSIX")
        format.dateFormat = "yyyy-MM-dd-HHmmss"
        return folder.appending(path: "image-\(format.string(from: date))-\(UUID().uuidString.prefix(8)).png")
    }
}
