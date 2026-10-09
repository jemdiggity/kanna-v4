// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "bm-cli",
    platforms: [.macOS(.v14)],
    dependencies: [.package(url: "https://github.com/lukilabs/beautiful-mermaid-swift", exact: "1.0.4")],
    targets: [.executableTarget(name: "bm-cli", dependencies: [.product(name: "BeautifulMermaid", package: "beautiful-mermaid-swift")])]
)
