// swift-tools-version:5.5
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription
import Foundation

let tag = "v0.7.0"
let checksum = "1acafd28aadd7098274f80b402f230346c3265a77f21917583814403c05347cd"
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
