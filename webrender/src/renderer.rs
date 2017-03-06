/* This Source Code Form is subject to the terms of the Mozilla Public
* License, v. 2.0. If a copy of the MPL was not distributed with this
* file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The webrender API.
//!
//! The `webrender::renderer` module provides the interface to webrender, which
//! is accessible through [`Renderer`][renderer]
//!
//! [renderer]: struct.Renderer.html

use debug_colors;
//use debug_render::DebugRenderer;
//use device::{DepthFunction, Device, ProgramId, TextureId, VertexFormat, GpuMarker, GpuProfiler};
//use device::{TextureFilter, VAOId, VertexUsageHint, FileWatcherHandler, TextureTarget, ShaderError};
use device::{Device, TextureFilter, TextureId, ShaderError};
//use device::{TextureFilter, TextureId, TextureTarget, VertexUsageHint, ShaderError};
use euclid::Matrix4D;
use fnv::FnvHasher;
use frame_builder::FrameBuilderConfig;
use gpu_store::{GpuStore, GpuStoreLayout};
use internal_types::{CacheTextureId, RendererFrame, ResultMsg, TextureUpdateOp};
use internal_types::{ExternalImageUpdateList, TextureUpdateList, PackedVertex, RenderTargetMode};
use internal_types::{ORTHO_NEAR_PLANE, ORTHO_FAR_PLANE, SourceTexture};
use internal_types::{BatchTextures, TextureSampler, GLContextHandleWrapper};
use prim_store::GradientData;
//use profiler::{Profiler, BackendProfileCounters};
//use profiler::{/*GpuProfileTag,*/ RendererProfileTimers, RendererProfileCounters};
use record::ApiRecordingReceiver;
use render_backend::RenderBackend;
use render_task::RenderTaskData;
use std;
use std::cmp;
use std::collections::HashMap;
use std::f32;
use std::hash::BuildHasherDefault;
use std::marker::PhantomData;
use std::mem;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use texture_cache::TextureCache;
use threadpool::ThreadPool;
use tiling::{AlphaBatchKind, BlurCommand, Frame, PrimitiveBatch, PrimitiveBatchData};
use tiling::{CacheClipInstance, PrimitiveInstance, RenderTarget};
use time::precise_time_ns;
use thread_profiler::{register_thread_with_profiler, write_profile};
use util::TransformedRectKind;
use webrender_traits::{ColorF, Epoch, PipelineId, RenderNotifier, RenderDispatcher};
use webrender_traits::{ExternalImageId, ImageData, ImageFormat, RenderApiSender, RendererKind};
use webrender_traits::{DeviceIntRect, DevicePoint, DeviceIntPoint, DeviceIntSize, DeviceUintSize};
use webrender_traits::{ImageDescriptor, BlobImageRenderer};
use webrender_traits::channel;
use webrender_traits::VRCompositorHandler;

use glutin;

pub const GPU_DATA_TEXTURE_POOL: usize = 5;
pub const MAX_VERTEX_TEXTURE_WIDTH: usize = 1024;

#[derive(Debug)]
pub enum InitError {
    Shader(ShaderError),
    Thread(std::io::Error),
}

impl From<ShaderError> for InitError {
    fn from(err: ShaderError) -> Self { InitError::Shader(err) }
}

impl From<std::io::Error> for InitError {
    fn from(err: std::io::Error) -> Self { InitError::Thread(err) }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum BlendMode {
    None,
    Alpha,

    Multiply,
    Max,
    Min,

    // Use the color of the text itself as a constant color blend factor.
    Subpixel(ColorF),
}

struct GpuDataTexture<L> {
    id: TextureId,
    layout: PhantomData<L>,
}

impl<L: GpuStoreLayout> GpuDataTexture<L> {
    fn new(device: &mut Device) -> GpuDataTexture<L> {
        //let id = device.create_texture_ids(1, TextureTarget::Default)[0];
        let id = TextureId::new(1);
        GpuDataTexture {
            id: id,
            layout: PhantomData,
        }
    }

    fn init<T: Default>(&mut self,
                        device: &mut Device,
                        data: &mut Vec<T>) {
        if data.is_empty() {
            return;
        }

        let items_per_row = L::items_per_row::<T>();

        // Extend the data array to be a multiple of the row size.
        // This ensures memory safety when the array is passed to
        // OpenGL to upload to the GPU.
        while data.len() % items_per_row != 0 {
            data.push(T::default());
        }

        let height = data.len() / items_per_row;

        /*device.init_texture(self.id,
                            L::texture_width::<T>() as u32,
                            height as u32,
                            L::image_format(),
                            L::texture_filter(),
                            RenderTargetMode::None,
                            Some(unsafe { mem::transmute(data.as_slice()) } ));*/
    }
}

pub struct VertexDataTextureLayout {}

impl GpuStoreLayout for VertexDataTextureLayout {
    fn image_format() -> ImageFormat {
        ImageFormat::RGBAF32
    }

    fn texture_width<T>() -> usize {
        MAX_VERTEX_TEXTURE_WIDTH - (MAX_VERTEX_TEXTURE_WIDTH % Self::texels_per_item::<T>())
    }

    fn texture_filter() -> TextureFilter {
        TextureFilter::Nearest
    }
}

type VertexDataTexture = GpuDataTexture<VertexDataTextureLayout>;
pub type VertexDataStore<T> = GpuStore<T, VertexDataTextureLayout>;

pub struct GradientDataTextureLayout {}

impl GpuStoreLayout for GradientDataTextureLayout {
    fn image_format() -> ImageFormat {
        ImageFormat::RGBA8
    }

    fn texture_width<T>() -> usize {
        mem::size_of::<GradientData>() / Self::texel_size()
    }

    fn texture_filter() -> TextureFilter {
        TextureFilter::Linear
    }
}

type GradientDataTexture = GpuDataTexture<GradientDataTextureLayout>;
pub type GradientDataStore = GpuStore<GradientData, GradientDataTextureLayout>;

pub struct RendererOptions {
    pub device_pixel_ratio: f32,
    pub resource_override_path: Option<PathBuf>,
    pub enable_aa: bool,
    pub enable_profiler: bool,
    pub debug: bool,
    pub enable_scrollbars: bool,
    pub precache_shaders: bool,
    pub renderer_kind: RendererKind,
    pub enable_subpixel_aa: bool,
    pub clear_framebuffer: bool,
    pub clear_color: ColorF,
    pub render_target_debug: bool,
    pub max_texture_size: Option<u32>,
    pub workers: Option<Arc<Mutex<ThreadPool>>>,
    pub blob_image_renderer: Option<Box<BlobImageRenderer>>,
    pub recorder: Option<Box<ApiRecordingReceiver>>,
}

impl Default for RendererOptions {
    fn default() -> RendererOptions {
        RendererOptions {
            device_pixel_ratio: 1.0,
            resource_override_path: None,
            enable_aa: true,
            enable_profiler: false,
            debug: false,
            enable_scrollbars: false,
            precache_shaders: false,
            renderer_kind: RendererKind::Native,
            enable_subpixel_aa: false,
            clear_framebuffer: true,
            clear_color: ColorF::new(1.0, 1.0, 1.0, 1.0),
            render_target_debug: false,
            max_texture_size: None,
            workers: None,
            blob_image_renderer: None,
            recorder: None,
        }
    }
}

/// The renderer is responsible for submitting to the GPU the work prepared by the
/// RenderBackend.
pub struct Renderer {
    result_rx: Receiver<ResultMsg>,
    device: Device,
    current_frame: Option<RendererFrame>,
    notifier: Arc<Mutex<Option<Box<RenderNotifier>>>>,
    clear_framebuffer: bool,
    clear_color: ColorF,
    last_time: u64,
    pipeline_epoch_map: HashMap<PipelineId, Epoch, BuildHasherDefault<FnvHasher>>,
    main_thread_dispatcher: Arc<Mutex<Option<Box<RenderDispatcher>>>>,
}

impl Renderer {
    /// Initializes webrender and creates a Renderer and RenderApiSender.
    ///
    /// # Examples
    /// Initializes a Renderer with some reasonable values. For more information see
    /// [RendererOptions][rendereroptions].
    /// [rendereroptions]: struct.RendererOptions.html
    ///
    /// ```rust,ignore
    /// # use webrender::renderer::Renderer;
    /// # use std::path::PathBuf;
    /// let opts = webrender::RendererOptions {
    ///    device_pixel_ratio: 1.0,
    ///    resource_override_path: None,
    ///    enable_aa: false,
    ///    enable_profiler: false,
    /// };
    /// let (renderer, sender) = Renderer::new(opts);
    /// ```
    pub fn new(window: &glutin::Window, mut options: RendererOptions) -> Result<(Renderer, RenderApiSender), InitError> {
        let (api_tx, api_rx) = try!{ channel::msg_channel() };
        let (payload_tx, payload_rx) = try!{ channel::payload_channel() };
        let (result_tx, result_rx) = channel();

        let notifier = Arc::new(Mutex::new(None));
        let mut device = Device::new(window);
        let main_thread_dispatcher = Arc::new(Mutex::new(None));
        let backend_notifier = notifier.clone();
        let backend_main_thread_dispatcher = main_thread_dispatcher.clone();
        let payload_tx_for_backend = payload_tx.clone();
        let config = FrameBuilderConfig::new(options.enable_scrollbars,
                                             options.enable_subpixel_aa,
                                             options.debug);
        let (device_pixel_ratio, enable_aa) = (options.device_pixel_ratio, options.enable_aa);
        let device_max_size = device.max_texture_size();
        let max_texture_size = cmp::min(device_max_size, options.max_texture_size.unwrap_or(device_max_size));
        let mut texture_cache = TextureCache::new(max_texture_size);
        let recorder = options.recorder;
        let workers = options.workers.take().unwrap_or_else(||{
            // TODO(gw): Use a heuristic to select best # of worker threads.
            Arc::new(Mutex::new(ThreadPool::new_with_name("WebRender:Worker".to_string(), 4)))
        });
        let blob_image_renderer = options.blob_image_renderer.take();
        try!{ thread::Builder::new().name("RenderBackend".to_string()).spawn(move || {
            let mut backend = RenderBackend::new(api_rx,
                                                 payload_rx,
                                                 payload_tx_for_backend,
                                                 result_tx,
                                                 device_pixel_ratio,
                                                 texture_cache,
                                                 enable_aa,
                                                 workers,
                                                 backend_notifier,
                                                 None,//context_handle,
                                                 config,
                                                 recorder,
                                                 backend_main_thread_dispatcher,
                                                 blob_image_renderer,
                                                 Arc::new(Mutex::new(None)));
            backend.run(/*backend_profile_counters*/);
        })};

        let renderer = Renderer {
            result_rx: result_rx,
            device: device,
            current_frame: None,
            notifier: notifier,
            clear_framebuffer: options.clear_framebuffer,
            clear_color: options.clear_color,
            last_time: 0,
            pipeline_epoch_map: HashMap::with_hasher(Default::default()),
            main_thread_dispatcher: main_thread_dispatcher,
        };

        let sender = RenderApiSender::new(api_tx, payload_tx);
        Ok((renderer, sender))
    }

    /// Sets the new RenderNotifier.
    ///
    /// The RenderNotifier will be called when processing e.g. of a (scrolling) frame is done,
    /// and therefore the screen should be updated.
    pub fn set_render_notifier(&self, notifier: Box<RenderNotifier>) {
        let mut notifier_arc = self.notifier.lock().unwrap();
        *notifier_arc = Some(notifier);
    }

    /// Processes the result queue.
    ///
    /// Should be called before `render()`, as texture cache updates are done here.
    pub fn update(&mut self) {
        // Pull any pending results and return the most recent.
        //println!("update!");
        while let Ok(msg) = self.result_rx.try_recv() {
            match msg {
                ResultMsg::NewFrame(frame, texture_update_list, external_image_update_list/*, profile_counters*/) => {
                    println!("new frame!");
                    //self.pending_texture_updates.push(texture_update_list);

                    // Update the list of available epochs for use during reftests.
                    // This is a workaround for https://github.com/servo/servo/issues/13149.
                    for (pipeline_id, epoch) in &frame.pipeline_epoch_map {
                        self.pipeline_epoch_map.insert(*pipeline_id, *epoch);
                    }

                    self.current_frame = Some(frame);
                }
                ResultMsg::RefreshShader(path) => {
                    println!("refrest shader!");
                    //self.pending_shader_updates.push(path);
                }
            }
        }
    }

    /// Renders the current frame.
    ///
    /// A Frame is supplied by calling [set_root_stacking_context()][newframe].
    /// [newframe]: ../../webrender_traits/struct.RenderApi.html#method.set_root_stacking_context
    pub fn render(&mut self, framebuffer_size: DeviceUintSize) {
        //println!("render!");
        if let Some(mut frame) = self.current_frame.take() {
            if let Some(ref mut frame) = frame.frame {
                //println!("frame!");
                println!("{:?}", frame.background_color);
                self.device.clear_target(Some(frame.background_color.unwrap().to_array()), Some(1.0));
                self.device.draw();
            }

            // Restore frame - avoid borrow checker!
            self.current_frame = Some(frame);
        } else {
            println!("no frame!");
        }
    }

    fn draw_tile_frame(&mut self,
                       frame: &mut Frame,
                       framebuffer_size: &DeviceUintSize) {
        if frame.passes.is_empty() {
            println!("empty!");
            self.device.clear_target(Some(self.clear_color.to_array()), Some(1.0));
        }
    }
}
