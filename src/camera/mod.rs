use std::sync::mpsc::Sender;

#[cfg(target_os = "android")]
use self::camera2::AndroidCamera;
use anyhow::Result;
use slint::Image;

#[cfg(target_os = "android")]
mod camera2;

pub struct Camera{
    #[cfg(target_os = "android")]
    camera: AndroidCamera,

}

impl Camera{
    pub fn new(
        #[cfg(target_os = "android")]
        app: slint::android::AndroidApp,
        image_sender: Sender<Image> 
    ) -> Result<Self>{
        #[cfg(target_os = "android")]
        let camera = AndroidCamera::new(app, image_sender);
        Ok(Camera{
            #[cfg(target_os = "android")]
            camera
        })
    }

    pub fn start_preview(&mut self, camera_id: &str, width: u32, height: u32) -> Result<()>{
        #[cfg(target_os = "android")]
        {
            self.camera.open(camera_id)?;
            self.camera.start_preview(width, height)?;
        }
        Ok(())
    }

    pub fn stop_preview(&mut self) -> Result<()>{
        #[cfg(target_os = "android")]
        {
            self.camera.close();
        }
        Ok(())
    }
    
}