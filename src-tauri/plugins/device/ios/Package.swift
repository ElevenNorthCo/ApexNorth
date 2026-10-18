// swift-tools-version:5.5

import PackageDescription

let package = Package(
    name: "tauri-plugin-apexnorth-device",
    platforms: [
        .macOS(.v10_13),
        .iOS(.v14),
    ],
    products: [
        .library(
            name: "tauri-plugin-apexnorth-device",
            type: .static,
            targets: ["tauri-plugin-apexnorth-device"])
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "tauri-plugin-apexnorth-device",
            dependencies: [
                .byName(name: "Tauri")
            ],
            path: "Sources")
    ]
)
