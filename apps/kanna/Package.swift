// swift-tools-version: 6.0
import PackageDescription

let rustLibraries = Context.packageDirectory + "/../../target/release"

let package = Package(
    name: "kanna",
    platforms: [.macOS(.v15)],
    products: [.executable(name: "Kanna", targets: ["Kanna"])],
    dependencies: [.package(url: "https://github.com/swiftlang/swift-markdown.git", exact: "0.7.3")],
    targets: [
        .binaryTarget(name: "GhosttyKit", path: "../../target/ghosttykit/GhosttyKit.xcframework"),
        .binaryTarget(name: "KannaMerman", path: "../../target/merman/KannaMerman.xcframework"),
        .systemLibrary(name: "KannaLayoutC", path: "Sources/KannaLayoutC"),
        .target(name: "KannaDiagram", dependencies: ["KannaMerman"], linkerSettings: ["AppKit", "CoreText"].map { .linkedFramework($0) }),
        .target(name: "KannaDocs", linkerSettings: ["AppKit", "WebKit"].map { .linkedFramework($0) }),
        .testTarget(name: "KannaDocsTests", dependencies: ["KannaDocs"]),
        .target(name: "KannaMarkdown", dependencies: ["KannaDocs", "KannaDiagram", .product(name: "Markdown", package: "swift-markdown")],
                linkerSettings: ["AppKit"].map { .linkedFramework($0) }),
        .testTarget(name: "KannaMarkdownTests", dependencies: ["KannaMarkdown"]),
        .target(name: "KannaCode", linkerSettings: ["AppKit"].map { .linkedFramework($0) }),
        .testTarget(name: "KannaCodeTests", dependencies: ["KannaCode"]),
        .target(name: "KannaLinks"),
        .target(name: "KannaDrop"),
        .target(name: "KannaStatus"),
        .testTarget(name: "KannaStatusTests", dependencies: ["KannaStatus"]),
        .testTarget(name: "KannaDropTests", dependencies: ["KannaDrop"]),
        .testTarget(name: "KannaLinksTests", dependencies: ["KannaLinks"]),
        .target(name: "SimBridge", linkerSettings: ["AppKit", "IOSurface", "QuartzCore"].map { .linkedFramework($0) }),
        .executableTarget(
            name: "Kanna",
            dependencies: ["GhosttyKit", "KannaLayoutC", "KannaDiagram", "KannaDocs", "KannaMarkdown", "KannaCode", "KannaLinks", "KannaDrop", "KannaStatus", "SimBridge"],
            linkerSettings: [.linkedLibrary("c++"), .unsafeFlags(["-L", rustLibraries])]
                + ["AppKit", "Carbon", "CoreText", "IOSurface", "Metal", "QuartzCore", "UniformTypeIdentifiers", "WebKit"].map { .linkedFramework($0) }
        ),
    ]
)
