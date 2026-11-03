// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "SYSUNetAuthMac",
    platforms: [.macOS(.v11)],
    products: [.executable(name: "SYSUNetAuth", targets: ["NetAuthApp"]), .executable(name: "SYSUNetAuthHelper", targets: ["NetAuthHelper"])],
    targets: [
        .target(name: "NetAuthCore"),
        .executableTarget(name: "NetAuthApp", dependencies: ["NetAuthCore"]),
        .executableTarget(name: "NetAuthHelper", dependencies: ["NetAuthCore"]),
        .testTarget(name: "NetAuthCoreTests", dependencies: ["NetAuthCore"])
    ]
)
