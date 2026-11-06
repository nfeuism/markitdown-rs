fn main() {
    println!("cargo:rerun-if-changed=src/vision_ocr.m");
    println!("cargo:rerun-if-changed=static/index.html");

    // IMPORTANT: cfg(target_os) inside build.rs describes the HOST that runs
    // this script, not the TARGET we are compiling for. On a Mac that means it
    // is always "macos" even when cross-building for Windows, which would make
    // cc try to compile an Objective-C file with a Windows toolchain.
    // CARGO_CFG_TARGET_OS is the correct target selector here.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        return;
    }

    // On macOS we compile a tiny Objective-C shim that talks to two frameworks
    // shipped with the OS: Vision (OCR) and PDFKit (page rendering).
    // No Python, no ONNX runtime, no downloaded model files.
    cc::Build::new()
        .file("src/vision_ocr.m")
        .flag("-fobjc-arc")
        .warnings(false)
        .compile("mdvision");

    for framework in ["Foundation", "Vision", "AppKit", "PDFKit", "QuartzCore"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
