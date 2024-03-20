use std::{sync::mpsc::channel, time::{Duration, Instant}};

use anyhow::{anyhow, Result};
use slint::{Timer, TimerMode};

use crate::camera::Camera;

pub fn run(
    #[cfg(target_os = "android")]
    android_app: slint::android::AndroidApp,
) -> Result<()> {
    slint::slint! {
        import { Button, VerticalBox, HorizontalBox } from "std-widgets.slint";
        export component MainWindow inherits Window {
            in-out property <image> camera-texture <=> camera-texture.source;
            callback open-camera(bool);

            VerticalLayout {
                VerticalBox { Text { text: "相机"; } }
                HorizontalBox {
                    alignment: center;
                    camera-texture := Image {
                        source: @image-url("assets/rust.png");
                    }
                }
                HorizontalBox {
                    Button {
                        text: "打开相机";
                        clicked => {
                            open-camera(true);
                        }
                    }
                    Button {
                        text: "关闭相机";
                        clicked => {
                            open-camera(false);
                        }
                    }
                }
            }
        }
    }

    let app = MainWindow::new()?;
    
    let (image_sender, image_receiver) = channel();

    let mut camera = Camera::new(#[cfg(target_os = "android")]android_app, image_sender)?;

    let app_clone = app.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, std::time::Duration::from_millis(10), move || {
        if let (Ok(img), Some(app)) = (image_receiver.try_recv(), app_clone.upgrade()){
            app.set_camera_texture(img);
        }
    });

    app.on_open_camera(move |open|{
        if open{
            let res = camera.start_preview(2, 1280, 720);
            println!("相机启动:{:?}", res);
        }else{
            let res = camera.stop_preview();
            println!("相机结束:{:?}", res);
        }
    });

    app.run()?;
    Ok(())
}
