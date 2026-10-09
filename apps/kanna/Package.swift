// swift-tools-version: 6.0
import PackageDescription

let rustLibraries = Context.packageDirectory + "/../../target/release"

let package = Package(
    name: "kanna",
    platforms: [.macOS(.v15)],
    products: [.executable(name: "Kanna", targets: ["Kanna"])],
    targets: [
        .binaryTarget(name: "GhosttyKit", path: "../../target/ghosttykit/GhosttyKit.xcframework"),
        .systemLibrary(name: "KannaLayoutC", path: "Sources/KannaLayoutC"),
        .executableTarget(
            name: "Kanna",
            dependencies: ["GhosttyKit", "KannaLayoutC"],
            linkerSettings: [.linkedLibrary("c++"), .unsafeFlags(["-L", rustLibraries])]
                + ["AppKit", "Carbon", "CoreText", "IOSurface", "Metal", "QuartzCore", "UniformTypeIdentifiers"].map { .linkedFramework($0) }
        ),
    ]
)
