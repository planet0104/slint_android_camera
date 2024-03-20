use std::{sync::{mpsc::Sender, Arc, Mutex}, time::{Duration, Instant}};
use anyhow::{ anyhow, Result};
use kamera::Camera as KCamera;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub struct Camera{
    camera_handle: Option<Arc<Mutex<bool>>>,
    camera_task: Option<std::thread::JoinHandle<Result<()>>>,
    image_sender: Sender<SharedPixelBuffer<Rgba8Pixel>>,
}

impl Camera{
    pub fn new(image_sender: Sender<SharedPixelBuffer<Rgba8Pixel>>) -> Self{
        Self { camera_handle:None, camera_task: None, image_sender }
    }

    pub fn start_preview(&mut self, index: usize, width: u32, height: u32) -> Result<()>{
        self.stop_preview();
        let camera_handle = Arc::new(Mutex::new(true));
        self.camera_handle = Some(camera_handle.clone());
        let image_sender_clone = self.image_sender.clone();
        self.camera_task = Some(std::thread::spawn(move ||{
            let camera = match KCamera::new_device(index){
                None => return Err(anyhow!("camera id not exist")),
                Some(v) => v
            };
            camera.start();
            let mut count = 0;
            let mut timer = Instant::now();
            let mut rgba_buffer = vec![];
            loop {
                if let Ok(opened) = camera_handle.lock(){
                    if !*opened{
                        break;
                    }
                }
                
                let frame = match camera.wait_for_frame(){
                    Some(f) => f,
                    None => {
                        println!("拍照失败!!");
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                
                let (width, height) = frame.size_u32();
                if rgba_buffer.len() as u32 != width*height*4{
                    rgba_buffer = vec![0; (width*height*4) as usize];
                }
                let frame_data = frame.data();
                let data_u8 = frame_data.data_u8();
                for (idx, bgra) in data_u8.chunks(4).enumerate(){
                    rgba_buffer[idx*4] = bgra[2];
                    rgba_buffer[idx*4+1] = bgra[1];
                    rgba_buffer[idx*4+2] = bgra[0];
                    rgba_buffer[idx*4+3] = bgra[3];
                }
                
                let buf = SharedPixelBuffer::clone_from_slice(&rgba_buffer, width, height);
                image_sender_clone.send(buf).map_err(|err| anyhow!("{:?}", err))?;

                if count == 30{
                    // let time = timer.elapsed().as_millis();
                    // println!("30帧耗时:{}ms 平均帧时间:{}ms {width}x{height} 帧率:{}FPS", time, time/30, 1000./(time as f32/30.));
                    count = 0;
                    timer = Instant::now();
                }
                count += 1;
            }
            camera.stop();
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