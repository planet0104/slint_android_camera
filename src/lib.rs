#[cfg(target_os = "android")]
mod app;

mod camera;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    slint::android::init(app.clone()).unwrap();
    app::run(app).unwrap();
}
