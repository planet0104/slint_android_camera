use std::sync::mpsc::Sender;

#[cfg(target_os = "android")]
use self::camera2::AndroidCamera;
use anyhow::Result;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

#[cfg(target_os = "android")]
mod camera2;

#[cfg(target_os = "windows")]
mod pcam;

pub struct Camera{
    #[cfg(target_os = "android")]
    camera: AndroidCamera,
    #[cfg(target_os = "windows")]
    camera: pcam::Camera,
}

impl Camera{
    pub fn new(
        #[cfg(target_os = "android")]
        app: slint::android::AndroidApp,
        image_sender: Sender<SharedPixelBuffer<Rgba8Pixel>> 
    ) -> Result<Self>{
        #[cfg(target_os = "android")]
        let camera = AndroidCamera::new(app, image_sender);
        Ok(Camera{
            #[cfg(target_os = "android")]
            camera,
            #[cfg(target_os = "windows")]
            camera: pcam::Camera::new(image_sender)
        })
    }

    pub fn start_preview(&mut self, camera_index: usize, width: u32, height: u32) -> Result<()>{
        #[cfg(target_os = "android")]
        {
            self.camera.open(&format!("{camera_index}"))?;
            self.camera.start_preview(width, height)?;
        }
        #[cfg(target_os = "windows")]
        self.camera.start_preview(camera_index, width, height)?;
        Ok(())
    }

    pub fn stop_preview(&mut self) -> Result<()>{
        #[cfg(target_os = "android")]
        {
            self.camera.close();
        }
        #[cfg(target_os = "windows")]
        self.camera.stop_preview();
        Ok(())
    }
    
}