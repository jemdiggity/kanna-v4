import Foundation

@MainActor
final class EchoLink: TerminalLink {
    func opened(_ terminal: ExternalTerminal) {}

    func input(_ bytes: Data, from terminal: ExternalTerminal) {
        var echoed = Data(capacity: bytes.count)
        for byte in bytes {
            switch byte {
            case 0x0D: echoed.append(contentsOf: [0x0D, 0x0A])
            case 0x7F: echoed.append(contentsOf: [0x08, 0x20, 0x08])
            default: echoed.append(byte)
            }
        }
        terminal.output(echoed)
    }

    func resizeRequested(_ terminal: ExternalTerminal, cols: UInt16, rows: UInt16, width: UInt32, height: UInt32) {
        terminal.setGrid(cols: cols, rows: rows)
    }

    func closed(_ terminal: ExternalTerminal) {}
}
