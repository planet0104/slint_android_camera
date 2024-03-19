use std::time::Instant;

use anyhow::{anyhow, Result};

pub fn run() -> Result<()> {
    slint::slint! {
        export component MainWindow inherits Window {
            in-out property <image> camera-texture <=> camera-texture.source;

            VerticalLayout {
                Text { text: "Hello World"; }
                camera-texture := Image {
                    source: @image-url("assets/rust.png");
                    width: 100px;
                }
            }
        }
    }

    let app = MainWindow::new()?;

    app.window()
        .set_rendering_notifier(move |state, graphics_api| {
            match state {
                slint::RenderingState::RenderingSetup => {}
                slint::RenderingState::BeforeRendering => {
                    // if let (Some(underlay), Some(app)) = (underlay.as_mut(), app_weak.upgrade()) {
                    //     app.set_camera_texture(slint::Image::from(texture));
                    //     app.window().request_redraw();
                    // }
                }
                slint::RenderingState::AfterRendering => {}
                slint::RenderingState::RenderingTeardown => {}
                _ => {}
            }
        })
        .map_err(|err| anyhow!("{:?}", err))?;

    app.run()?;
    Ok(())
}
