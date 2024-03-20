use std::{sync::mpsc::channel, time::{Duration, Instant}};

use anyhow::{anyhow, Result};
use slint::{Image, Timer, TimerMode};

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

            Rectangle {
                padding: 0px;
                width: 100%;
                height: 100%;
                HorizontalLayout {
                    padding: 0px;
                    alignment: center;
                    camera-texture := Image {
                        source: @image-url("assets/rust.png");
                    }
                }
                Rectangle {
                    x: 0px;
                    y: 0px;
                    width: 100px;
                    height: 40px;
                    Text { text: "相机"; }
                }
                Rectangle {
                    height: 40px;
                    width: 100px;
                    x: (parent.width/2 - self.width/2);
                    y: (parent.height - self.height);
                    HorizontalBox {
                        padding: 0px;
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
    }

    let app = MainWindow::new()?;
    
    let (image_sender, image_receiver) = channel();

    let mut camera = Camera::new(#[cfg(target_os = "android")]android_app, image_sender)?;

    let app_clone = app.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, std::time::Duration::from_millis(10), move || {
        if let (Ok(buffer), Some(app)) = (image_receiver.try_recv(), app_clone.upgrade()){
            app.set_camera_texture(Image::from_rgba8(buffer));
        }
    });

    app.on_open_camera(move |open|{
        if open{
            let res = camera.start_preview(0, 1280, 720);
            println!("相机启动:{:?}", res);
        }else{
            let res = camera.stop_preview();
            println!("相机结束:{:?}", res);
        }
    });

    app.run()?;
    Ok(())
}
