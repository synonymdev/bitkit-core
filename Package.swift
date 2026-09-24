// swift-tools-version:5.5
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription
import Foundation

let tag = "v0.6.0"
let checksum = "eed89e4a6d060e064bc91f8a66115ffff90fff6edd659c42024077c49a8982b2"
let url = "https://github.com/synonymdev/bitkit-core/releases/download/\(tag)/BitkitCore.xcframework.zip"

let localBinary = ProcessInfo.processInfo.environment["BITKIT_CORE_LOCAL"] == "1"

let package = Package(
    name: "bitkitcore",
    platforms: [
        .iOS(.v15),
        .macOS(.v12),
    ],
    products: [
        // Products define the executables and libraries a package produces, and make them visible to other packages.
        .library(
            name: "BitkitCore",
            targets: ["BitkitCoreFFI", "BitkitCore"]),
    ],
    targets: [
        .target(
            name: "BitkitCore",
            dependencies: ["BitkitCoreFFI"],
            path: "./bindings/ios",
            sources: ["bitkitcore.swift"]
        ),
        localBinary
            ? .binaryTarget(
                name: "BitkitCoreFFI",
                path: "./dist/ios/BitkitCore.xcframework"
            )
            : .binaryTarget(
                name: "BitkitCoreFFI",
                url: url,
                checksum: checksum
            )
    ]
)
