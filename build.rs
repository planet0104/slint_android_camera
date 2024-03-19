fn main() {
    #[cfg(target_os = "android")]
    {
        println!("cargo:rustc-link-lib=dylib=camera2ndk");
        println!("cargo:rustc-link-lib=dylib=mediandk");
    }
}