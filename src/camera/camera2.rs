use anyhow::{anyhow, Result};
use slint::{Image, SharedPixelBuffer};
use core::slice;
use image::{
    imageops::{rotate180, rotate270, rotate90},
    GrayImage, ImageBuffer, Rgb, RgbaImage,
};
use jni::{
    objects::{JObject, JString, JValueGen},
    sys::{JNIInvokeInterface_, _jobject, jint},
    JavaVM,
};
use log::{error, info};
use ndk_sys::{
    acamera_metadata_tag, camera_status_t, media_status_t, ACameraCaptureSession,
    ACameraCaptureSession_setRepeatingRequest, ACameraCaptureSession_stateCallbacks, ACameraDevice,
    ACameraDevice_StateCallbacks, ACameraDevice_close, ACameraDevice_createCaptureRequest,
    ACameraDevice_createCaptureSession, ACameraDevice_getId, ACameraDevice_request_template,
    ACameraIdList, ACameraManager_create, ACameraManager_delete, ACameraManager_deleteCameraIdList,
    ACameraManager_getCameraCharacteristics, ACameraManager_getCameraIdList,
    ACameraManager_openCamera, ACameraMetadata, ACameraMetadata_const_entry, ACameraMetadata_free,
    ACameraMetadata_getConstEntry, ACameraOutputTarget, ACameraOutputTarget_create,
    ACameraOutputTarget_free, ACaptureRequest, ACaptureRequest_addTarget, ACaptureRequest_free,
    ACaptureSessionOutput, ACaptureSessionOutputContainer, ACaptureSessionOutputContainer_add,
    ACaptureSessionOutputContainer_create, ACaptureSessionOutputContainer_free,
    ACaptureSessionOutput_create, ACaptureSessionOutput_free, AImageCropRect, AImageReader,
    AImageReader_ImageListener, AImageReader_acquireLatestImage, AImageReader_getFormat,
    AImageReader_getHeight, AImageReader_getWidth, AImageReader_getWindow, AImageReader_new,
    AImageReader_setImageListener, AImage_delete, AImage_getCropRect, AImage_getNumberOfPlanes,
    AImage_getPlaneData, AImage_getPlanePixelStride, AImage_getPlaneRowStride, AImage_getTimestamp,
    AImage_getWidth, ANativeWindow, AIMAGE_FORMATS,
};
use pollster::FutureExt;
use std::{
    borrow::Cow,
    ffi::{c_int, c_void, CStr},
    io::Write,
    mem::zeroed,
    ptr::null_mut,
    sync::mpsc::Sender,
    time::Instant,
};
use wgpu::{
    util::{BufferInitDescriptor, DeviceExt},
    BindGroup, Buffer, ComputePipeline, Device, Limits, Queue, ShaderModel, Texture, TextureView,
};

#[link(name = "camera2ndk")]
extern "C" {}

#[link(name = "mediandk")]
extern "C" {}

pub struct AndroidCamera {
    app: slint::android::AndroidApp,
    camera_device: *mut ACameraDevice,
    capture_request: *mut ACaptureRequest,
    camera_output_target: *mut ACameraOutputTarget,
    session_output: *mut ACaptureSessionOutput,
    capture_session_output_container: *mut ACaptureSessionOutputContainer,
    image_reader: *mut AImageReader,
    /// width,height,format
    image_formats: Vec<(i32, i32, i32)>,
    camera_id: Option<String>,
    image_listener: AImageReader_ImageListener,
    capture_session_state_callbacks: ACameraCaptureSession_stateCallbacks,
    device_state_callbacks: ACameraDevice_StateCallbacks,
    preview_width: u32,
    preview_height: u32,
    timer: Instant,
    frame_count: i32,
    decoder_gpu: Option<YuvGpuDecoder>,
    rgba_buffer: Vec<u8>,
    image_sender: Sender<slint::Image>,
    lens_facing: u8,
    sensor_orientation: i32,
    color_image: Option<slint::Image>,
}

impl AndroidCamera {
    pub fn new(app: slint::android::AndroidApp, image_sender: Sender<slint::Image>) -> Self {
        Self {
            app,
            camera_device: null_mut(),
            capture_request: null_mut(),
            camera_output_target: null_mut(),
            session_output: null_mut(),
            capture_session_output_container: null_mut(),
            image_reader: null_mut(),
            image_formats: vec![],
            camera_id: None,
            image_listener: AImageReader_ImageListener {
                context: null_mut(),
                onImageAvailable: None,
            },
            capture_session_state_callbacks: unsafe { zeroed() },
            device_state_callbacks: unsafe { zeroed() },
            preview_width: 0,
            preview_height: 0,
            timer: Instant::now(),
            frame_count: 0,
            decoder_gpu: None,
            rgba_buffer: vec![],
            image_sender,
            lens_facing: 0,
            sensor_orientation: 0,
            color_image: None,
        }
    }

    pub fn open(&mut self, camera_id: &str) -> Result<()> {
        let permission = "android.permission.CAMERA";
        if !check_self_permission(&self.app, permission)? {
            request_camera_permission(&self.app)?;
            return Err(anyhow!("没有相机权限"));
        }
        unsafe {
            let camera_manager = ACameraManager_create();
            let mut camera_id_list_raw = null_mut();
            let camera_status =
                ACameraManager_getCameraIdList(camera_manager, &mut camera_id_list_raw);
            if camera_status != camera_status_t::ACAMERA_OK {
                return Err(anyhow!(
                    "Failed to get camera id list (reason: {:?})",
                    camera_status
                ));
            }
            info!("camera_id_list_raw={:?}", camera_id_list_raw);

            if camera_id_list_raw.is_null() {
                return Err(anyhow!(
                    "Failed to get camera id list (reason: camera_id_list is null)"
                ));
            }

            let camera_id_list = &*camera_id_list_raw;

            info!("camera_id_list.cameraIds {:?}", camera_id_list.cameraIds);
            info!(
                "camera_id_list.cameraIds.is_null = {}",
                camera_id_list.cameraIds.is_null()
            );

            if camera_id_list.numCameras < 1 {
                return Err(anyhow!("No camera device detected."));
            }

            let camera_ids =
                slice::from_raw_parts(camera_id_list.cameraIds, camera_id_list.numCameras as usize);

            let camera_id_strings: Vec<String> = camera_ids
                .iter()
                .map(|v| CStr::from_ptr(*v).to_str().unwrap_or("").to_string())
                .collect();
            info!("camera_ids: {:?}", camera_id_strings);

            let mut selected_camera_id = None;
            let mut selected_camera_idx = -1;
            for (idx, cid) in camera_ids.iter().enumerate() {
                if CStr::from_ptr(*cid).to_str().unwrap_or("-1") == camera_id {
                    selected_camera_id = Some(cid);
                    selected_camera_idx = idx as i32;
                    break;
                }
            }

            if selected_camera_id.is_none() {
                return Err(anyhow!("Camera Id not found."));
            }
            let selected_camera_id = selected_camera_id.unwrap();

            info!(
                "Trying to open Camera2 (index: {:?}, num of camera : {})",
                selected_camera_idx,
                camera_id_strings.len()
            );

            let mut camera_metadata = null_mut();

            let camera_status = ACameraManager_getCameraCharacteristics(
                camera_manager,
                *selected_camera_id,
                &mut camera_metadata,
            );
            if camera_status != camera_status_t::ACAMERA_OK {
                return Err(anyhow!(
                    "Failed to get camera meta data of index:{:?}",
                    selected_camera_idx
                ));
            }

            info!("camera_metadata is null? {}", camera_metadata.is_null());

            let (lens_facing, sensor_orientation) =
                AndroidCamera::get_sensor_orientation(camera_metadata);
            info!("lens_facing: {lens_facing}");
            info!("sensor_orientation: {sensor_orientation}");
            self.lens_facing = lens_facing;
            self.sensor_orientation = sensor_orientation;

            // 获取相机支持的分辨率
            self.image_formats = AndroidCamera::get_video_size(camera_metadata)?;

            info!("image_formats: {:?}", self.image_formats);

            unsafe extern "C" fn on_disconnected(_data: *mut c_void, device: *mut ACameraDevice) {
                info!("Camera(id: {:?}) is disconnected.", get_cstr(ACameraDevice_getId(device)));
            }

            unsafe extern "C" fn on_error(
                _data: *mut c_void,
                device: *mut ACameraDevice,
                error: c_int,
            ) {
                error!("Error(code: {}) on Camera(id: {:?}).", error, get_cstr(ACameraDevice_getId(device)));
            }

            self.device_state_callbacks.onDisconnected = Some(on_disconnected);
            self.device_state_callbacks.onError = Some(on_error);

            let camera_status = ACameraManager_openCamera(
                camera_manager,
                *selected_camera_id,
                &mut self.device_state_callbacks,
                &mut self.camera_device,
            );

            if camera_status != camera_status_t::ACAMERA_OK {
                return Err(anyhow!(
                    "Failed to open camera device (index: {})",
                    selected_camera_idx
                ));
            }

            self.camera_id = Some(camera_id.to_string());

            ACameraMetadata_free(camera_metadata);
            ACameraManager_deleteCameraIdList(camera_id_list_raw);
            ACameraManager_delete(camera_manager);
        }
        Ok(())
    }

    fn get_sensor_orientation(camera_metadata: *mut ACameraMetadata) -> (u8, i32) {
        unsafe {
            let mut lens_facing: ACameraMetadata_const_entry = zeroed();
            let mut sensor_orientation: ACameraMetadata_const_entry = zeroed();

            ACameraMetadata_getConstEntry(
                camera_metadata,
                acamera_metadata_tag::ACAMERA_LENS_FACING.0,
                &mut lens_facing,
            );
            ACameraMetadata_getConstEntry(
                camera_metadata,
                acamera_metadata_tag::ACAMERA_SENSOR_ORIENTATION.0,
                &mut sensor_orientation,
            );

            let u8_arr = slice::from_raw_parts(lens_facing.data.u8_, lens_facing.count as usize);
            let i32_arr = slice::from_raw_parts(
                sensor_orientation.data.i32_,
                sensor_orientation.count as usize,
            );
            let lens_facing = u8_arr[0];
            let sensor_orientation = i32_arr[0];
            (lens_facing, sensor_orientation)
        }
    }

    // 获取相机支持的分辨率
    fn get_video_size(camera_metadata: *mut ACameraMetadata) -> Result<Vec<(i32, i32, i32)>> {
        unsafe {
            let mut available_configs: ACameraMetadata_const_entry = zeroed();
            let camera_status = ACameraMetadata_getConstEntry(
                camera_metadata,
                acamera_metadata_tag::ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS.0,
                &mut available_configs,
            );
            if camera_status != camera_status_t::ACAMERA_OK {
                return Err(anyhow!(
                    "Failed to get ACameraMetadata_const_entry res={:?}",
                    camera_status
                ));
            }

            // 数据格式: format, width, height, input?, type int32
            let mut formats: Vec<(i32, i32, i32)> = vec![];
            let data_i32_list: &[i32] = slice::from_raw_parts(
                available_configs.data.i32_,
                available_configs.count as usize,
            );
            for i in 0..available_configs.count as usize {
                let input_idx = i * 4 + 3;
                let format_idx = i * 4 + 0;
                if format_idx >= available_configs.count as usize {
                    break;
                }
                let input = data_i32_list[input_idx];
                let format = data_i32_list[format_idx];

                if input != 0 {
                    continue;
                }

                if format == AIMAGE_FORMATS::AIMAGE_FORMAT_YUV_420_888.0 as i32 {
                    let width = data_i32_list[i * 4 + 1];
                    let height = data_i32_list[i * 4 + 2];
                    info!("YUV_420: {width}x{height}");
                    formats.push((width, height, format));
                }
            }
            Ok(formats)
        }
    }

    pub fn close(&mut self) {
        unsafe {
            if !self.image_reader.is_null() {
                ACaptureRequest_free(self.capture_request);
                self.capture_request = null_mut();
            }

            if !self.camera_output_target.is_null() {
                ACameraOutputTarget_free(self.camera_output_target);
                self.camera_output_target = null_mut();
            }

            if !self.camera_device.is_null() {
                let camera_status = ACameraDevice_close(self.camera_device);

                if camera_status != camera_status_t::ACAMERA_OK {
                    error!("Failed to close CameraDevice.");
                }
                self.camera_device = null_mut();
            }

            if !self.session_output.is_null() {
                ACaptureSessionOutput_free(self.session_output);
                self.session_output = null_mut();
            }

            if !self.capture_session_output_container.is_null() {
                ACaptureSessionOutputContainer_free(self.capture_session_output_container);
                self.capture_session_output_container = null_mut();
            }
        }
        info!("Close Camera");
    }

    pub fn start_preview(&mut self, width: u32, height: u32) -> Result<()> {
        self.preview_width = width;
        self.preview_height = height;
        self.decoder_gpu = Some(YuvGpuDecoder::new(width, height)?);
        self.rgba_buffer = vec![0; (width * height * 4) as usize];
        self.create_image_reader(width, height, AIMAGE_FORMATS::AIMAGE_FORMAT_YUV_420_888)?;
        unsafe {
            let camera_status = ACameraDevice_createCaptureRequest(
                self.camera_device,
                ACameraDevice_request_template::TEMPLATE_PREVIEW,
                &mut self.capture_request,
            );

            if camera_status != camera_status_t::ACAMERA_OK {
                return Err(anyhow!(
                    "Failed to create preview capture request (id: {:?})",
                    self.camera_id
                ));
            }

            let mut native_window: *mut ANativeWindow = null_mut();
            let res = AImageReader_getWindow(self.image_reader, &mut native_window);

            if res != media_status_t::AMEDIA_OK {
                error!("AImageReader_getWindow error.");
            }

            ACameraOutputTarget_create(native_window, &mut self.camera_output_target);
            ACaptureRequest_addTarget(self.capture_request, self.camera_output_target);

            let mut session_output = null_mut();
            ACaptureSessionOutput_create(native_window, &mut session_output);

            let camera_status =
                ACaptureSessionOutputContainer_create(&mut self.capture_session_output_container);

            if camera_status != camera_status_t::ACAMERA_OK {
                return Err(anyhow!(
                    "Failed to create capture session output container (reason: {:?})",
                    camera_status
                ));
            }

            unsafe extern "C" fn capture_session_on_ready(
                context: *mut c_void,
                session: *mut ACameraCaptureSession,
            ) {
                info!("Session is ready. {:?}", session);
                let camera_ptr: *mut AndroidCamera = context as *mut _ as *mut AndroidCamera;
            }

            unsafe extern "C" fn capture_session_on_active(
                context: *mut c_void,
                session: *mut ACameraCaptureSession,
            ) {
                info!("Session is activated. {:?}", session);
                let camera_ptr: *mut AndroidCamera = context as *mut _ as *mut AndroidCamera;
            }

            unsafe extern "C" fn capture_session_on_closed(
                context: *mut c_void,
                session: *mut ACameraCaptureSession,
            ) {
                info!("Session is closed. {:?}", session);
                let camera_ptr: *mut AndroidCamera = context as *mut _ as *mut AndroidCamera;
            }

            self.capture_session_state_callbacks.onReady = Some(capture_session_on_ready);
            self.capture_session_state_callbacks.onActive = Some(capture_session_on_active);
            self.capture_session_state_callbacks.onClosed = Some(capture_session_on_closed);
            self.capture_session_state_callbacks.context = (self as *mut _) as *mut c_void;

            ACaptureSessionOutputContainer_add(
                self.capture_session_output_container,
                session_output,
            );

            let mut capture_session = null_mut();
            let camera_status = ACameraDevice_createCaptureSession(
                self.camera_device,
                self.capture_session_output_container,
                &self.capture_session_state_callbacks,
                &mut capture_session,
            );

            if camera_status != camera_status_t::ACAMERA_OK {
                return Err(anyhow!(
                    "Failed to create capture session (reason: {:?})",
                    camera_status
                ));
            }

            let camera_status = ACameraCaptureSession_setRepeatingRequest(
                capture_session,
                null_mut(),
                1,
                &mut self.capture_request,
                null_mut(),
            );

            if camera_status != camera_status_t::ACAMERA_OK {
                return Err(anyhow!(
                    "Failed to set repeating request (reason: {:?})",
                    camera_status
                ));
            }
        }
        Ok(())
    }

    fn on_image_available(&mut self) -> Result<()> {
        unsafe {
            let t = Instant::now();
            let mut image = null_mut();
            let media_status = AImageReader_acquireLatestImage(self.image_reader, &mut image);
            if media_status != media_status_t::AMEDIA_OK {
                let msg = if media_status == media_status_t::AMEDIA_IMGREADER_NO_BUFFER_AVAILABLE {
                    "An image reader frame was discarded".to_string()
                } else {
                    format!(
                        "Failed to acquire latest image from image reader, error: {:?}.",
                        media_status
                    )
                };
                return Err(anyhow!("{msg}"));
            }

            let mut format = 0;
            let res = AImageReader_getFormat(self.image_reader, &mut format);
            if res != media_status_t::AMEDIA_OK {
                return Err(anyhow!("AImageReader_getFormat error res={:?}.", res));
            }

            if format != AIMAGE_FORMATS::AIMAGE_FORMAT_YUV_420_888.0 as i32 {
                return Err(anyhow!("format is not AIMAGE_FORMAT_YUV_420_888"));
            }

            let mut width = 0;
            let mut height = 0;

            let res = AImageReader_getWidth(self.image_reader, &mut width);
            if res != media_status_t::AMEDIA_OK {
                return Err(anyhow!("AImageReader_getWidth error res={:?}.", res));
            }
            let res = AImageReader_getHeight(self.image_reader, &mut height);
            if res != media_status_t::AMEDIA_OK {
                return Err(anyhow!("AImageReader_getHeight error res={:?}.", res));
            }

            // info!("获取到了预览帧 {width}x{height}");

            let mut src_rect: AImageCropRect = zeroed();
            let res = AImage_getCropRect(image, &mut src_rect);

            if res != media_status_t::AMEDIA_OK {
                return Err(anyhow!("AImage_getCropRect error res={:?}.", res));
            }

            let mut y_stride = 0;
            let mut uv_stride = 0;
            let mut y_pixel = null_mut();
            let mut u_pixel = null_mut();
            let mut v_pixel = null_mut();
            let mut y_len = 0;
            let mut v_len = 0;
            let mut u_len = 0;
            let mut vu_pixel_stride = 0;

            AImage_getPlaneRowStride(image, 0, &mut y_stride);
            AImage_getPlaneRowStride(image, 1, &mut uv_stride);
            AImage_getPlaneData(image, 0, &mut y_pixel, &mut y_len);
            AImage_getPlaneData(image, 1, &mut v_pixel, &mut v_len);
            AImage_getPlaneData(image, 2, &mut u_pixel, &mut u_len);
            AImage_getPlanePixelStride(image, 1, &mut vu_pixel_stride);

            // println!("y_stride={y_stride}");
            // println!("u_stride={uv_stride}");

            // println!("y_len={y_len}");
            // println!("u_len={u_len}");
            // println!("v_len={v_len}");
            // println!("vu_pixel_stride={vu_pixel_stride}");

            /*
            图像宽度:1280
            图像高度: 960
            y_stride=1280
            u_stride=1280
            v_stride=1280
            y_len=1228800
            u_len=614399
            v_len=614399
            vu_pixel_stride=2

            V 的指针和 U 的指针，实际上指向的是一块数据，他们之前相差1个像素，所以不等于宽度/2
            Y 的指针是 Y数据+UV数据块的开头，实际上整个AImage是一个完整的yuv数据块

             */

            let yuv_data =
                slice::from_raw_parts(y_pixel, ((width * height) + (width * height) / 2) as usize);

            let mut timestamp_ns = 0;
            let _ = AImage_getTimestamp(image, &mut timestamp_ns);

            // info!("gpu yuv_data:{}", yuv_data.len());
            let t = Instant::now();
            // info!("start gpu decode...");
            //GPU转换耗时 6~8毫秒左右，有时会是10ms左右
            self.decoder_gpu.as_mut().unwrap().decode(
                &yuv_data,
                &mut self.rgba_buffer,
                self.sensor_orientation,
            )?;
            let (output_width, output_height) = match self
                .decoder_gpu
                .as_ref()
                .unwrap()
                .rotate_output_size
                .as_ref()
            {
                None => (width, height),
                Some(o) => (o.width as i32, o.height as i32),
            };
            let buf = SharedPixelBuffer::clone_from_slice(&self.rgba_buffer, output_width as u32, output_height as u32);
            self.image_sender.send(Image::from_rgba8(buf)).map_err(|err| anyhow!("{:?}", err))?;
            info!("转码+旋转+Send耗时:{}ms", t.elapsed().as_millis());

            // 预览回调帧率正常是 30FPS
            self.frame_count += 1;
            if self.timer.elapsed().as_millis() > 1000 {
                info!("预览 FPS:{}", self.frame_count);
                self.timer = Instant::now();
                self.frame_count = 0;
            }

            AImage_delete(image);
            Ok(())
        }
    }

    fn create_image_reader(
        &mut self,
        width: u32,
        height: u32,
        image_format: AIMAGE_FORMATS,
    ) -> Result<()> {
        unsafe {
            let res: ndk_sys::media_status_t = AImageReader_new(
                width as i32,
                height as i32,
                image_format.0 as i32,
                2,
                &mut self.image_reader,
            );

            if res != media_status_t::AMEDIA_OK {
                return Err(anyhow!("create Image Reader error."));
            }

            unsafe extern "C" fn on_image_available(
                context: *mut c_void,
                image_reader: *mut AImageReader,
            ) {
                //还原Camera指针
                let camera = &mut *(context as *mut _ as *mut AndroidCamera);
                let _ = camera.on_image_available();
                // println!("on_image_available:{:?}", res);
            }

            let camera_ptr: *mut AndroidCamera = self as *mut _;

            self.image_listener.context = camera_ptr as *mut c_void;
            self.image_listener.onImageAvailable = Some(on_image_available);

            let res = AImageReader_setImageListener(self.image_reader, &mut self.image_listener);
            if res != media_status_t::AMEDIA_OK {
                return Err(anyhow!("set Image Listener error."));
            }
        }
        Ok(())
    }
}

impl Drop for AndroidCamera {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// android: YUV420SP 转 rgb
pub fn decode_yuv420sp(data: &[u8], width: i32, height: i32) -> Vec<u8> {
    let frame_size = width * height;
    let mut yp = 0;
    let mut rgba_data = Vec::with_capacity(frame_size as usize * 4);
    for j in 0..height {
        let (mut uvp, mut u, mut v) = ((frame_size + (j >> 1) * width) as usize, 0, 0);
        for i in 0..width {
            let mut y = (0xff & data[yp] as i32) - 16;
            if y < 0 {
                y = 0;
            }
            if i & 1 == 0 {
                v = (0xff & data[uvp] as i32) - 128;
                uvp += 1;
                u = (0xff & data[uvp] as i32) - 128;
                uvp += 1;
            }

            let y1192 = 1192 * y;
            let mut r = y1192 + 1634 * v;
            let mut g = y1192 - 833 * v - 400 * u;
            let mut b = y1192 + 2066 * u;

            if r < 0 {
                r = 0;
            } else if r > 262143 {
                r = 262143;
            };
            if g < 0 {
                g = 0;
            } else if g > 262143 {
                g = 262143;
            }
            if b < 0 {
                b = 0;
            } else if b > 262143 {
                b = 262143;
            }

            let r = (r >> 10) & 0xff;
            let g = (g >> 10) & 0xff;
            let b = (b >> 10) & 0xff;
            rgba_data.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
            yp += 1;
        }
    }

    rgba_data
}

struct YuvGpuDecoder {
    device: Device,
    queue: Queue,
    width: u32,
    height: u32,
    y_texture: Texture,
    u_texture: Texture,
    easu_texture: Texture,
    texture_size: wgpu::Extent3d,
    u_size: wgpu::Extent3d,
    compute_pipeline_yuv: ComputePipeline,
    compute_yuv_bind_group: BindGroup,
    padded_bytes_per_row: usize,
    unpadded_bytes_per_row: usize,

    rgba_texture_view: TextureView,
    rotate_compute_pipeline: ComputePipeline,
    rotate_bind_group: Option<BindGroup>,
    rotate_output_texture: Option<Texture>,
    rotate_output_size: Option<wgpu::Extent3d>,
}

impl YuvGpuDecoder {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        info!("create YuvGpuDecoder {width}x{height}");
        //------------------------------------------------------
        // 初始化硬件设备
        //------------------------------------------------------

        info!("create YuvGpuDecoder instance...");

        let instance = wgpu::Instance::default();

        info!("create YuvGpuDecoder adapter...");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptionsBase {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .block_on()
            .ok_or(anyhow::anyhow!("Couldn't create the adapter"))?;

        info!("create YuvGpuDecoder device,adapter...");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER,
                    required_limits: Limits::default(),
                },
                None,
            )
            .block_on()?;

        //------------------------------------------------------
        // 创建 pipeline layout、compute pipeline、bind group layout 和 shader module
        //------------------------------------------------------
        info!("create YuvGpuDecoder compute_texture_yuv_bind_group_layout...");
        let compute_texture_yuv_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Sampler(
                            // SamplerBindingType::Comparison is only for TextureSampleType::Depth
                            // SamplerBindingType::Filtering if the sample_type of the texture is:
                            //     TextureSampleType::Float { filterable: true }
                            // Otherwise you'll get an error.
                            wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });
        info!("create YuvGpuDecoder compute_yuv_pipeline_layout...");
        let compute_yuv_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&compute_texture_yuv_bind_group_layout],
                push_constant_ranges: &[],
            });
        info!("create YuvGpuDecoder compute_pipeline_yuv...");
        let compute_pipeline_yuv =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("compute_pipeline"),
                layout: Some(&compute_yuv_pipeline_layout),
                module: &device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("compute_shader_module"),
                    source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("yuv2rgb.wgsl"))),
                }),
                entry_point: "main",
            });

        //------------------------------------------------------
        // 创建纹理、纹理视图、采样器和缓冲区，并设置它们的相关描述符
        //------------------------------------------------------
        info!("create YuvGpuDecoder texture_size...");
        let texture_size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let u_size = wgpu::Extent3d {
            width: width / 2,
            height: height / 2,
            depth_or_array_layers: 1,
        };
        info!("create YuvGpuDecoder y_texture...");
        let y_texture = device.create_texture(&wgpu::TextureDescriptor {
            // All textures are stored as 3D, we represent our 2D texture
            // by setting depth to 1.
            size: texture_size,
            mip_level_count: 1, // We'll talk about this a little later
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Most images are stored using sRGB so we need to reflect that here.
            format: wgpu::TextureFormat::R8Unorm,
            // TEXTURE_BINDING tells wgpu that we want to use this texture in shaders
            // COPY_DST means that we want to copy data to this texture
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("y_texture"),
            view_formats: &[],
        });

        info!("create YuvGpuDecoder u_texture...");
        let u_texture = device.create_texture(&wgpu::TextureDescriptor {
            // All textures are stored as 3D, we represent our 2D texture
            // by setting depth to 1.
            size: u_size,
            mip_level_count: 1, // We'll talk about this a little later
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Most images are stored using sRGB so we need to reflect that here.
            format: wgpu::TextureFormat::Rg8Unorm,
            // TEXTURE_BINDING tells wgpu that we want to use this texture in shaders
            // COPY_DST means that we want to copy data to this texture
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("uv_texture"),
            view_formats: &[],
        });

        info!("create YuvGpuDecoder easu_texture...");
        let easu_texture = device.create_texture(&wgpu::TextureDescriptor {
            // All textures are stored as 3D, we represent our 2D texture
            // by setting depth to 1.
            size: texture_size,
            mip_level_count: 1, // We'll talk about this a little later
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Most images are stored using sRGB so we need to reflect that here.
            // format: wgpu::TextureFormat::Rgba8Unorm,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // TEXTURE_BINDING tells wgpu that we want to use this texture in shaders
            // COPY_DST means that we want to copy data to this texture
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::STORAGE_BINDING,
            label: Some("diffuse_texture"),
            view_formats: &[],
        });

        info!("create YuvGpuDecoder y_texture_view...");
        let y_texture_view = y_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let u_texture_view = u_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let easu_texture_view = easu_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let uv_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToBorder,
            address_mode_v: wgpu::AddressMode::ClampToBorder,
            address_mode_w: wgpu::AddressMode::ClampToBorder,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        info!("create YuvGpuDecoder compute_yuv_bind_group...");
        let compute_yuv_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &compute_texture_yuv_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&y_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&u_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&uv_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&easu_texture_view),
                },
            ],
            label: Some("yuv_bind_group2"),
        });

        let padded_bytes_per_row = YuvGpuDecoder::padded_bytes_per_row(width);
        let unpadded_bytes_per_row = width as usize * 4;

        let rotate_compute_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("compute_pipeline"),
                layout: None,
                module: &device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("compute_shader_module"),
                    source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("rotate.wgsl"))),
                }),
                entry_point: "main",
            });

        Ok(Self {
            device,
            queue,
            width,
            height,
            y_texture,
            u_texture,
            easu_texture,
            texture_size,
            u_size,
            compute_pipeline_yuv,
            compute_yuv_bind_group,
            padded_bytes_per_row,
            unpadded_bytes_per_row,
            rotate_compute_pipeline,
            rgba_texture_view: easu_texture_view,
            rotate_bind_group: None,
            rotate_output_texture: None,
            rotate_output_size: None,
        })
    }

    fn decode(&mut self, data: &[u8], output: &mut [u8], rotate_degree: i32) -> Result<()> {
        // let t = Instant::now();
        //------------------------------------------------------
        // YUV数据写入纹理中
        //------------------------------------------------------

        let y_data = &data[..(self.width * self.height) as usize];
        let uv_data = &data[(self.width * self.height) as usize..];

        self.queue.write_texture(
            wgpu::ImageCopyTextureBase {
                texture: &self.y_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &y_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(self.width),
                rows_per_image: Some(self.height),
            },
            self.texture_size,
        );

        self.queue.write_texture(
            wgpu::ImageCopyTextureBase {
                texture: &self.u_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &uv_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(self.width),
                rows_per_image: Some(self.height),
            },
            self.u_size,
        );

        //------------------------------------------------------
        // 开始新的计算 pass
        //------------------------------------------------------

        //是否需要旋转
        let need_rotate = rotate_degree >= 90 && rotate_degree <= 270;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            cpass.set_pipeline(&self.compute_pipeline_yuv);
            cpass.set_bind_group(0, &self.compute_yuv_bind_group, &[]);
            cpass.dispatch_workgroups(self.width / 8, self.height / 8, 1);
        }

        let mut rgba_output_buffer = None;

        if !need_rotate {
            //如果不需要旋转，复制处理完成的 rgba纹理到缓冲区
            let output_buffer_size = self.padded_bytes_per_row as u64
                * self.height as u64
                * std::mem::size_of::<u8>() as u64;

            let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: output_buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

            encoder.copy_texture_to_buffer(
                wgpu::ImageCopyTexture {
                    aspect: wgpu::TextureAspect::All,
                    texture: &self.easu_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                },
                wgpu::ImageCopyBuffer {
                    buffer: &output_buffer,
                    layout: wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(self.padded_bytes_per_row as u32),
                        rows_per_image: Some(self.height),
                    },
                },
                self.texture_size,
            );

            rgba_output_buffer = Some(output_buffer);
        }

        self.queue.submit(Some(encoder.finish()));

        // info!("转换完成 耗时:{}ms", t.elapsed().as_millis());

        //开始旋转
        if need_rotate {
            // let t = Instant::now();
            if self.rotate_bind_group.is_none() {
                self.rotate_init(rotate_degree);
            }

            //rgba图像转换完成之后，直接使用rgba_texture_view再次处理旋转
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

            {
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                cpass.set_pipeline(&self.rotate_compute_pipeline);
                cpass.set_bind_group(0, self.rotate_bind_group.as_ref().unwrap(), &[]);
                let workgroup_count_x = (self.texture_size.width + 16 - 1) / 16;
                let workgroup_count_y = (self.texture_size.height + 16 - 1) / 16;
                cpass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
            }

            let rotate_output_size = self.rotate_output_size.clone().unwrap();

            let padded_bytes_per_row = Self::padded_bytes_per_row(rotate_output_size.width);
            let unpadded_bytes_per_row = rotate_output_size.width as usize * 4;

            let output_buffer_size = padded_bytes_per_row as u64
                * rotate_output_size.height as u64
                * std::mem::size_of::<u8>() as u64;
            let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: output_buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

            encoder.copy_texture_to_buffer(
                wgpu::ImageCopyTexture {
                    aspect: wgpu::TextureAspect::All,
                    texture: self.rotate_output_texture.as_ref().unwrap(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                },
                wgpu::ImageCopyBuffer {
                    buffer: &output_buffer,
                    layout: wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bytes_per_row as u32),
                        rows_per_image: Some(rotate_output_size.height),
                    },
                },
                rotate_output_size,
            );

            self.queue.submit(Some(encoder.finish()));

            let buffer_slice = output_buffer.slice(..);
            buffer_slice.map_async(wgpu::MapMode::Read, |_| {});

            self.device.poll(wgpu::Maintain::Wait);

            let padded_data = buffer_slice.get_mapped_range();

            for (padded, pixels) in padded_data
                .chunks_exact(padded_bytes_per_row)
                .zip(output.chunks_exact_mut(unpadded_bytes_per_row))
            {
                pixels.copy_from_slice(&padded[..unpadded_bytes_per_row]);
            }
            // info!("旋转完成 耗时:{}ms", t.elapsed().as_millis());
        } else {
            let output_buffer = rgba_output_buffer.as_ref().unwrap();
            let buffer_slice = output_buffer.slice(..);
            buffer_slice.map_async(wgpu::MapMode::Read, |_| {});

            self.device.poll(wgpu::Maintain::Wait);

            let padded_data = buffer_slice.get_mapped_range();

            for (padded, pixels) in padded_data
                .chunks_exact(self.padded_bytes_per_row)
                .zip(output.chunks_exact_mut(self.unpadded_bytes_per_row))
            {
                pixels.copy_from_slice(&padded[..self.unpadded_bytes_per_row]);
            }
        };

        Ok(())
    }

    fn rotate_init(&mut self, rotate_degree: i32) {
        //创建旋转缓冲区
        let (rotate_output_width, rotate_output_height) = if (rotate_degree / 90) % 2 == 0 {
            (self.texture_size.width, self.texture_size.height)
        } else {
            (self.texture_size.height, self.texture_size.width)
        };
        let rotate_output_size = wgpu::Extent3d {
            width: rotate_output_width,
            height: rotate_output_height,
            depth_or_array_layers: 1,
        };

        // 输出图像
        let rotate_output_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("output texture"),
            size: rotate_output_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });

        // 旋转参数
        let config_buffer = self.device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            usage: wgpu::BufferUsages::STORAGE,
            contents: bytemuck::cast_slice(&[rotate_degree]),
        });

        let rotate_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.rotate_compute_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.rgba_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &rotate_output_texture.create_view(&wgpu::TextureViewDescriptor::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: config_buffer.as_entire_binding(),
                },
            ],
            label: Some("bind_group"),
        });

        self.rotate_bind_group = Some(rotate_bind_group);
        self.rotate_output_texture = Some(rotate_output_texture);
        self.rotate_output_size = Some(rotate_output_size);
    }

    /// Compute the next multiple of 256 for texture retrieval padding.
    pub fn padded_bytes_per_row(width: u32) -> usize {
        let bytes_per_row = width as usize * 4;
        let padding = (256 - bytes_per_row % 256) % 256;
        bytes_per_row + padding
    }
}

pub fn sdk_version(app: &slint::android::AndroidApp) -> Result<i32> {
    unsafe {
        let vm = JavaVM::from_raw(app.vm_as_ptr() as *mut *const JNIInvokeInterface_)?;
        let mut env = vm.attach_current_thread()?;
        Ok(env
            .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
            .i()?)
    }
}

pub fn check_self_permission(app: &slint::android::AndroidApp, permission: &str) -> Result<bool> {
    unsafe {
        let vm = JavaVM::from_raw(app.vm_as_ptr() as *mut *const JNIInvokeInterface_)?;
        let mut env = vm.attach_current_thread()?;
        let granted_int = env
            .get_static_field(
                "android/content/pm/PackageManager",
                "PERMISSION_GRANTED",
                "I",
            )?
            .i()?;
        // 创建Java字符串
        let permission_str = env.new_string(permission)?;
        let activity: JObject<'_> = JObject::from_raw(app.activity_as_ptr() as *mut _jobject);
        let result = env
            .call_method(
                activity,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[JValueGen::Object(&JObject::from(permission_str))],
            )?
            .i()?;
        Ok(result == granted_int)
    }
}

pub fn get_cache_dir(app: &slint::android::AndroidApp) -> Result<String> {
    unsafe {
        let vm = JavaVM::from_raw(app.vm_as_ptr() as *mut *const JNIInvokeInterface_)?;
        let mut env = vm.attach_current_thread()?;
        let activity: JObject<'_> = JObject::from_raw(app.activity_as_ptr() as *mut _jobject);

        let file = env.call_method(activity, "getCacheDir", "()Ljava/io/File;", &[])?;

        if let JValueGen::Object(file) = file {
            let path = env.call_method(file, "getAbsolutePath", "()Ljava/lang/String;", &[])?;

            if let JValueGen::Object(path) = path {
                let path: JString = path.into();
                let str = env.get_string(&path)?;
                let str = std::ffi::CStr::from_ptr(str.get_raw());
                Ok(str.to_str()?.to_string())
            } else {
                Err(anyhow!("object is not a string"))
            }
        } else {
            Err(anyhow!("object is not a file"))
        }
    }
}

pub fn request_permissions(
    app: &slint::android::AndroidApp,
    permissions: &[&str],
    request_code: i32,
) -> Result<()> {
    unsafe {
        let vm = JavaVM::from_raw(app.vm_as_ptr() as *mut *const JNIInvokeInterface_)?;
        let mut env = vm.attach_current_thread()?;
        let activity: JObject<'_> = JObject::from_raw(app.activity_as_ptr() as *mut _jobject);

        // 创建一个Java String数组
        let permission_count = permissions.len() as jint;
        let java_permission_array =
            env.new_object_array(permission_count, "java/lang/String", JObject::null())?;
        for (index, permission) in permissions.iter().enumerate() {
            let permission_str = env.new_string(*permission)?;
            env.set_object_array_element(&java_permission_array, index as jint, permission_str)?;
        }

        // 调用requestPermissions方法
        let _ = env.call_method(
            activity,
            "requestPermissions",
            "([Ljava/lang/String;I)V",
            &[
                JValueGen::Object(&JObject::from(java_permission_array)),
                request_code.into(),
            ],
        )?;
    }
    Ok(())
}

pub fn request_camera_permission(app: &slint::android::AndroidApp) -> Result<()> {
    let sdk_version = sdk_version(app)?;
    info!("sdk version:{sdk_version}");
    let permission = "android.permission.CAMERA";
    if sdk_version > 23 {
        if !check_self_permission(app, permission)? {
            request_permissions(app, &[permission], 100)?;
        }
    }
    Ok(())
}

pub unsafe fn get_cstr<'a>(s: *const ::std::os::raw::c_char) -> Option<&'a str> {
    let cstr = CStr::from_ptr(s);
    match cstr.to_str() {
        Ok(s) => Some(s),
        Err(err) => None,
    }
}
