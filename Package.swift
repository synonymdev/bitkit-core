// swift-tools-version:5.5
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription
import Foundation

let tag = "v0.8.0-rc8"
let checksum = "ab603824ac3c9295b73d4719525ec88b4585a1befb28d911bdab146af613a0bc"
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
