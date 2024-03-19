:: 编译报错：
:: error: linking with `D:\Android\ndk\25.2.9519653\toolchains\llvm\prebuilt\windows-x86_64\bin\clang.exe` failed: exit code: 1
:: "D:\Program Files\Rust\.rustup\toolchains\stable-x86_64-pc-windows-msvc\lib\rustlib\aarch64-linux-android\lib"
:: 复制
:: D:\Android\ndk\25.2.9519653\toolchains\llvm\prebuilt\windows-x86_64\sysroot\usr\lib\aarch64-linux-android\24\libcamera2ndk.so
:: D:\Android\ndk\25.2.9519653\toolchains\llvm\prebuilt\windows-x86_64\sysroot\usr\lib\aarch64-linux-android\24\libmediandk.so
::  到 对应的路径

cargo apk run --target aarch64-linux-android --lib