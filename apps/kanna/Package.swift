// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "kanna",
    platforms: [.macOS(.v15)],
    products: [.executable(name: "Kanna", targets: ["Kanna"])],
    targets: [
        .binaryTarget(name: "GhosttyKit", path: "../../target/ghosttykit/GhosttyKit.xcframework"),
        .executableTarget(
            name: "Kanna",
            dependencies: ["GhosttyKit"],
            linkerSettings: [.linkedLibrary("c++")] + ["AppKit", "Carbon", "CoreText", "IOSurface", "Metal", "QuartzCore", "UniformTypeIdentifiers"].map { .linkedFramework($0) }
        ),
    ]
)
