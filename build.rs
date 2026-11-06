fn main() {
    println!("cargo:rerun-if-changed=src/vision_ocr.m");
    println!("cargo:rerun-if-changed=static/index.html");

    // On macOS we compile a tiny Objective-C shim that talks to two frameworks
    // shipped with the OS: Vision (OCR) and PDFKit (page rendering).
    // No Python, no ONNX runtime, no downloaded model files.
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/vision_ocr.m")
            .flag("-fobjc-arc")
            .warnings(false)
            .compile("mdvision");

        for framework in ["Foundation", "Vision", "AppKit", "PDFKit", "QuartzCore"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
