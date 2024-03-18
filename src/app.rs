use std::time::Instant;

use anyhow::Result;
use nokhwa::{pixel_format::RgbAFormat, utils::{CameraIndex, RequestedFormat, RequestedFormatType}, Camera};

pub fn run() -> Result<()>{

    let index = CameraIndex::Index(0);
    let requested = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera = Camera::new(index, requested)?;
    // camera.set_frame_format(nokhwa::utils::FrameFormat::NV12);
    println!("frame rate={:?}", camera.frame_rate());
    println!("camera format={:?}", camera.camera_format());
    println!("frame format={:?}", camera.frame_format());
    let mut t = Instant::now();
    let mut frame_count = 0;
    for _ in 0..300{
        if t.elapsed().as_millis() >= 1000{
            println!("fps:{frame_count}");
            frame_count = 0;
            t = Instant::now();
        }
        let frame = camera.frame()?;
        println!("bytes:{}", frame.buffer().len());
        frame_count += 1;
    }

    slint::slint!{
        export component MainWindow inherits Window {
            Text { text: "Hello World"; }
        }
    }
    MainWindow::new().unwrap().run().unwrap();
    Ok(())
}