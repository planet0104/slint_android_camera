use std::{sync::{mpsc::Sender, Arc, Mutex}, time::Instant};

use anyhow::Result;
use escapi::Device;
use image::RgbaImage;

pub struct ESCamera{
    camera_handle: Option<Arc<Mutex<bool>>>,
    camera_task: Option<std::thread::JoinHandle<Result<()>>>,
    image_sender: Sender<slint::Image>,
}

impl ESCamera{
    pub fn new(image_sender: Sender<slint::Image>) -> Self{
        Self { camera_handle:None, camera_task: None, image_sender }
    }

    pub fn num_devices() -> usize{
        escapi::num_devices()
    }

    pub fn start_preview(&mut self, index: usize, width: u32, height: u32) -> Result<()>{
        self.stop_preview();
        let camera_handle = Arc::new(Mutex::new(true));
        self.camera_handle = Some(camera_handle.clone());
        self.camera_task = Some(std::thread::spawn(move ||{
            let camera = escapi::init(index, width, height, 30)?;
            let mut count = 0;
            let mut t = Instant::now();
            loop {
                if let Ok(opened) = camera_handle.lock(){
                    if !*opened{
                        break;
                    }
                }
                let pixels = camera.capture().unwrap_or(&[]);
                // let img = RgbaImage::from_raw(camera.capture_width(), camera.capture_height(), pixels.to_vec());
                // println!("img:{}", img.is_some());
                // println!("pixels:{} {}x{}", pixels.len(), camera.capture_width(), camera.capture_height());
                if count == 30{
                    let time = t.elapsed().as_millis();
                    println!("30帧耗时:{}ms 平均帧时间:{}ms len:{} 帧率:{}FPS", time, time/30, pixels.len(), 1000./(time as f32/30.));
                    count = 0;
                    t = Instant::now();
                }
                count += 1;
            }
            Ok(())
        }));
        Ok(())
    }

    pub fn stop_preview(&mut self){
        let mut need_close = false;
        if let Some(handle) = self.camera_handle.as_ref(){
            if let Ok(mut handle) = handle.lock(){
                *handle = false;
                need_close = true;
            }
        }

        if need_close{
            println!("stop preview..");
            if let Some(handle) = self.camera_task.take(){
                let res = handle.join();
                println!("stop preview: {:?}", res);
            }
        }
    }
}