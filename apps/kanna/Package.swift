// swift-tools-version: 6.0
import PackageDescription

// Kanna.app: Kanna's own mux app. Its terminal views start from kmux's
// (apps/kmux at kmux.rev) and draw with GhosttyKit from the Ghostty fork
// (external I/O), which scripts/build-ghosttykit.sh builds into target/.
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
