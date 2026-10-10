import Foundation

public enum DropText {
    static let special: Set<Character> = Set("\\ ()[]{}<>\"'`!#$&;|*?\t")

    public static func escape(_ path: String) -> String {
        var out = ""
        for character in path {
            if special.contains(character) { out.append("\\") }
            out.append(character)
        }
        return out
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
