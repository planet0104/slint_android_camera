#[cfg(target_os = "android")]
mod app;

mod camera;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: slint::android::AndroidApp) {
    slint::android::init(app).unwrap();
    app::run().unwrap();
}
