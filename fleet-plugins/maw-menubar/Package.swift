// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MawMenubar",
    platforms: [.macOS(.v13)],
    products: [.executable(name: "maw-menubar", targets: ["MawMenubar"])],
    targets: [
        .executableTarget(name: "MawMenubar", path: "native"),
        .testTarget(name: "MawMenubarTests", dependencies: ["MawMenubar"], path: "native-tests"),
    ]
)
