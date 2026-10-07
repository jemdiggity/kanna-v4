import Foundation
import Testing

@testable import KmuxCore

/// Runs the protocol cases shared with the reference model
/// (tests/kmux-protocol/cases, also run by tests/kmux-protocol/run-model.mjs).
/// Cases that use a command or pane type kmux doesn't have yet are skipped
/// and listed, so the gap to the model stays visible.
@MainActor
@Suite struct ProtocolCasesTests {
    static let casesDirectory = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().appendingPathComponent("../../../../tests/kmux-protocol/cases").standardized

    @Test func sharedProtocolCases() async throws {
        let files = try FileManager.default.contentsOfDirectory(atPath: Self.casesDirectory.path).filter { $0.hasSuffix(".json") }.sorted()
        #expect(!files.isEmpty, "no cases in \(Self.casesDirectory.path)")
        let paneTypes = Set(await capabilities())
        var passed = 0
        var skipped: [String] = []
        for file in files {
            guard case .array(let cases) = try JSON.parse(Data(contentsOf: Self.casesDirectory.appendingPathComponent(file))) else {
                Issue.record("\(file) is not an array of cases")
                continue
            }
            for testCase in cases {
                let name = "\(file): \(testCase["name"]?.string ?? "?")"
                guard case .array(let steps) = testCase["steps"] ?? nil else { continue }
                let missing = steps.compactMap { step -> String? in
                    let send = step["send"] ?? nil
                    if let cmd = send["cmd"]?.string, !Core.commands.contains(cmd), cmd != "nope" { return cmd }
                    if send["cmd"] == "open", let type = send["args"]?["type"]?.string, ["term", "web", "ios"].contains(type), !paneTypes.contains(type) { return "\(type) panes" }
                    return nil
                }
                if !missing.isEmpty {
                    skipped.append("\(name) (needs \(Set(missing).sorted().joined(separator: ", ")))")
                    continue
                }
                let (core, host) = makeCore()
                for (index, step) in steps.enumerated() {
                    guard case .object(var request) = step["send"] ?? nil else { continue }
                    request["id"] = .number(Double(index + 1))
                    let reply = await core.handle(.object(request))
                    if let problem = Self.mismatch(step["expect"] ?? ["ok": true], reply) {
                        Issue.record("\(name)\n  step \(index + 1): \(problem)\n  reply: \(String(decoding: reply.encoded(), as: UTF8.self))")
                        break
                    }
                }
                passed += 1
                _ = host
            }
        }
        print("native: \(passed) cases run, \(skipped.count) skipped\n" + skipped.map { "  skipped \($0)" }.joined(separator: "\n"))
    }

    private func capabilities() async -> [String] {
        let reply = await Core().handle(["cmd": "capabilities"])
        guard case .array(let types) = reply["paneTypes"] ?? nil else { return [] }
        return types.compactMap(\.string)
    }

    /// `expected` matches when every field it names matches; objects may have
    /// extra fields, arrays must be the same length. Same rule as cases.mjs.
    static func mismatch(_ expected: JSON, _ actual: JSON?, path: String = "") -> String? {
        let actual = actual ?? .null
        switch expected {
        case .array(let items):
            guard case .array(let got) = actual else { return "\(path): expected an array, got \(actual)" }
            guard got.count == items.count else { return "\(path): expected \(items.count) items, got \(got.count)" }
            for (i, item) in items.enumerated() {
                if let problem = mismatch(item, got[i], path: "\(path)[\(i)]") { return problem }
            }
            return nil
        case .object(let fields):
            guard case .object(let got) = actual else { return "\(path): expected an object, got \(actual)" }
            for (key, value) in fields.sorted(by: { $0.key < $1.key }) {
                if let problem = mismatch(value, got[key], path: path.isEmpty ? key : "\(path).\(key)") { return problem }
            }
            return nil
        default:
            return expected == actual ? nil : "\(path): expected \(expected), got \(actual)"
        }
    }
}
