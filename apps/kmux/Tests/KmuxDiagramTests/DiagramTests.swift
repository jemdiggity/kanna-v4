import AppKit
import Testing
@testable import KmuxDiagram

@Suite struct DiagramTests {
    @Test func flowchartHasEveryLabelAndColour() throws {
        let layout = try Diagram.layout(source: """
        flowchart LR
            A[Start here] -->|go| B{Decide}
            B --> C[(Store)]
            classDef hot fill:#f96
            class C hot
        """)
        #expect(layout.type.hasPrefix("flowchart"))
        let scene = try #require(layout.scene)
        #expect(Set(scene.texts).isSuperset(of: ["Start here", "go", "Decide", "Store"]))
        #expect(scene.size.width > scene.size.height) // left to right
        let orange = scene.items.contains { item in
            guard case .shape(let shape) = item, case .color(let color)? = shape.fill else { return false }
            return abs((color.components?[0] ?? 0) - 1) < 0.01 && abs((color.components?[1] ?? 0) - 0.6) < 0.01
        }
        #expect(orange, "classDef fill should colour the node")
    }

    @Test func sequenceAndStateDiagramsLayOut() throws {
        let sequence = try #require(try Diagram.layout(source: "sequenceDiagram\n    Alice->>Bob: Hello\n    Bob-->>Alice: Hi").scene)
        #expect(Set(sequence.texts).isSuperset(of: ["Alice", "Bob", "Hello", "Hi"]))
        let state = try #require(try Diagram.layout(source: "stateDiagram-v2\n    [*] --> Idle\n    Idle --> Busy: work\n    Busy --> [*]").scene)
        #expect(Set(state.texts).isSuperset(of: ["Idle", "Busy", "work"]))
    }

    @Test func otherTypesAreUnsupportedNotErrors() throws {
        let layout = try Diagram.layout(source: "pie title Pets\n    \"Dogs\" : 3\n    \"Cats\" : 2")
        #expect(layout.scene == nil)
        #expect(layout.type.lowercased().contains("pie"))
    }

    @Test func invalidSourceThrows() {
        #expect(throws: DiagramError.self) { try Diagram.layout(source: "flowchart LR\n    A -->") }
    }

    /// Labels are measured with Core Text, so drawing never has to shrink one
    /// to fit its box, in light or dark, and the scene contains everything drawn.
    @Test func labelsFitTheirBoxes() throws {
        let scene = try #require(try Diagram.layout(source: """
        flowchart TD
            A[A fairly long label that has to wrap onto more than one line] --> B([Stadium])
            B --> C{{Hexagon label}}
        """).scene)
        let context = try #require(CGContext(data: nil, width: Int(scene.size.width), height: Int(scene.size.height), bitsPerComponent: 8,
                                             bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                             bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        #expect(scene.draw(in: context, dark: false) == 0)
        #expect(scene.png(scale: 2, dark: true) != nil)
        for item in scene.items {
            if case .text(let label) = item {
                #expect(CGRect(origin: .zero, size: scene.size).contains(label.rect.integral.insetBy(dx: 1, dy: 1)))
            }
        }
    }
}
