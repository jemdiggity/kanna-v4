// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "kmux",
    platforms: [.macOS(.v15)],
    products: [.executable(name: "kmux", targets: ["Kmux"])],
    // Markdown parsing (GitHub-flavoured, on cmark-gfm), as kanna-v3 used.
    dependencies: [.package(url: "https://github.com/swiftlang/swift-markdown.git", exact: "0.7.3")],
    targets: [
        .binaryTarget(name: "GhosttyKit", path: "../../target/ghosttykit/GhosttyKit.xcframework"),
        .binaryTarget(name: "KmuxMerman", path: "../../target/merman/KmuxMerman.xcframework"),
        .target(name: "KmuxCore"),
        // Mermaid diagrams laid out by merman and drawn natively (no web view).
        .target(name: "KmuxDiagram", dependencies: ["KmuxMerman"], linkerSettings: ["AppKit", "CoreText"].map { .linkedFramework($0) }),
        // Markdown panes: files rendered natively with TextKit, diagrams drawn by KmuxDiagram.
        .target(name: "KmuxMarkdown", dependencies: ["KmuxDiagram", .product(name: "Markdown", package: "swift-markdown")],
                linkerSettings: ["AppKit"].map { .linkedFramework($0) }),
        // The iOS Simulator's screen and touches, through Xcode's private frameworks.
        .target(name: "SimBridge", linkerSettings: ["AppKit", "IOSurface", "QuartzCore"].map { .linkedFramework($0) }),
        .testTarget(name: "KmuxCoreTests", dependencies: ["KmuxCore"]),
        .testTarget(name: "KmuxDiagramTests", dependencies: ["KmuxDiagram"]),
        .testTarget(name: "KmuxMarkdownTests", dependencies: ["KmuxMarkdown", .product(name: "Markdown", package: "swift-markdown")]),
        // Renders Mermaid files to PNG with KmuxDiagram, for checking against mermaid.js.
        .executableTarget(name: "kmux-diagram", dependencies: ["KmuxDiagram"]),
        .executableTarget(
            name: "Kmux",
            dependencies: ["KmuxCore", "KmuxDiagram", "KmuxMarkdown", "GhosttyKit", "SimBridge"],
            linkerSettings: [.linkedLibrary("c++")] + ["AppKit", "Carbon", "CoreText", "IOSurface", "Metal", "QuartzCore", "UniformTypeIdentifiers"].map { .linkedFramework($0) }
        ),
    ]
)
