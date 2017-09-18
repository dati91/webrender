/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use euclid::Transform3D;
use internal_types::RenderTargetMode;
use std::collections::{HashSet, HashMap};
use std::fs::File;
use std::io::Read;
use std::iter::repeat;
use std::mem;
use std::ops::Add;
use std::path::PathBuf;
use std::ptr;
use std::rc::Rc;
use std::thread;
use api::{ColorF, ImageFormat};
use api::{DeviceIntPoint, DeviceIntRect, DeviceIntSize, DeviceUintSize};

use rand::Rng;
use std;
use gfx;
use gfx::CombinedError;
use gfx::Factory;
use gfx::texture::Kind;
use gfx::traits::FactoryExt;
use gfx::format::{DepthStencil as DepthFormat, Rgba8 as ColorFormat};
use gfx::format::{Formatted, R8, Rgba8, Rgba32F, Srgba8, SurfaceTyped, TextureChannel, TextureSurface, Unorm};
use gfx::format::{R8_G8_B8_A8, R32_G32_B32_A32};
use gfx::handle::Sampler;
use gfx::memory::Typed;
use tiling::RenderTargetKind;
use pipelines::{primitive, ClipProgram, Position, PrimitiveInstances, Program, Locals};
use renderer::{BlendMode, MAX_VERTEX_TEXTURE_WIDTH, TextureSampler};

use backend;
use InitWindow;
use ResultWindow;

use backend::Resources as R;
#[cfg(all(target_os = "windows", feature="dx11"))]
pub type CB = self::backend::CommandBuffer<backend::DeferredContext>;
#[cfg(not(feature = "dx11"))]
pub type CB = self::backend::CommandBuffer;

#[cfg(all(target_os = "windows", feature="dx11"))]
pub type BackendDevice = backend::Deferred;
#[cfg(not(feature = "dx11"))]
pub type BackendDevice = backend::Device;
#[cfg(all(target_os = "windows", feature="dx11"))]
use gfx_window_dxgi;

pub const LAYER_TEXTURE_WIDTH: usize = 1017;
pub const RENDER_TASK_TEXTURE_WIDTH: usize = 1023;
pub const TEXTURE_HEIGTH: usize = 8;
pub const DEVICE_PIXEL_RATIO: f32 = 1.0;
pub const MAX_INSTANCE_COUNT: usize = 5000;

pub const A_STRIDE: usize = 1;
pub const RG_STRIDE: usize = 2;
pub const RGB_STRIDE: usize = 3;
pub const RGBA_STRIDE: usize = 4;

pub type TextureId = u32;

//pub const INVALID: TextureId = 0;
pub const DUMMY_A8: TextureId = 0;
pub const DUMMY_RGBA8: TextureId = 1;
pub const DITHER: TextureId = 2;
const FIRST_UNRESERVED_ID: TextureId = DITHER + 1;

pub type A8 = (R8, Unorm);

// The value of the type GL_FRAMEBUFFER_SRGB from https://www.khronos.org/registry/OpenGL/extensions/ARB/ARB_framebuffer_sRGB.txt
const GL_FRAMEBUFFER_SRGB: u32 = 0x8DB9;

#[derive(Debug, Copy, Clone, PartialEq, Ord, Eq, PartialOrd)]
pub struct FrameId(usize);

impl FrameId {
    pub fn new(value: usize) -> FrameId {
        FrameId(value)
    }
}

impl Add<usize> for FrameId {
    type Output = FrameId;

    fn add(self, other: usize) -> FrameId {
        FrameId(self.0 + other)
    }
}

pub struct TextureSlot(pub usize);

// In some places we need to temporarily bind a texture to any slot.
const DEFAULT_TEXTURE: TextureSlot = TextureSlot(0);

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TextureTarget {
    Default,
    Array,
    Rect,
    External,
}


#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TextureFilter {
    Nearest,
    Linear,
}

#[derive(Debug)]
pub enum VertexAttributeKind {
    F32,
    U8Norm,
    I32,
    U16,
}

#[derive(Debug)]
pub struct VertexAttribute {
    pub name: &'static str,
    pub count: u32,
    pub kind: VertexAttributeKind,
}

#[derive(Debug)]
pub struct VertexDescriptor {
    pub vertex_attributes: &'static [VertexAttribute],
    pub instance_attributes: &'static [VertexAttribute],
}

impl VertexAttributeKind {
    fn size_in_bytes(&self) -> u32 {
        match *self {
            VertexAttributeKind::F32 => 4,
            VertexAttributeKind::U8Norm => 1,
            VertexAttributeKind::I32 => 4,
            VertexAttributeKind::U16 => 2,
        }
    }
}

impl VertexAttribute {
    fn size_in_bytes(&self) -> u32 {
        self.count * self.kind.size_in_bytes()
    }
}

impl VertexDescriptor {
    fn instance_stride(&self) -> u32 {
        self.instance_attributes
            .iter()
            .map(|attr| attr.size_in_bytes()).sum()
    }
}

pub struct DataTexture<T> where T: gfx::format::TextureFormat {
    pub handle: gfx::handle::Texture<R, T::Surface>,
    pub srv: gfx::handle::ShaderResourceView<R, T::View>,
}

impl<T> DataTexture<T> where T: gfx::format::TextureFormat {
    pub fn create<F>(factory: &mut F, size: [usize; 2]) -> Result<DataTexture<T>, CombinedError>
        where F: gfx::Factory<R>
    {
        let (width, height) = (size[0] as u16, size[1] as u16);
        let tex_kind = Kind::D2(width, height, gfx::texture::AaMode::Single);

        let (surface, view) = {
            let surface = <T::Surface as gfx::format::SurfaceTyped>::get_surface_type();
            let desc = gfx::texture::Info {
                kind: tex_kind,
                levels: 1,
                format: surface,
                bind: gfx::memory::SHADER_RESOURCE,
                usage: gfx::memory::Usage::Dynamic,
            };
            let cty = <T::Channel as gfx::format::ChannelTyped>::get_channel_type();
            let raw = try!(factory.create_texture_raw(desc, Some(cty), None));
            let levels = (0, raw.get_info().levels - 1);
            let tex = Typed::new(raw);
            let view = try!(factory.view_texture_as_shader_resource::<T>(&tex, levels, gfx::format::Swizzle::new()));
            (tex, view)
        };

        Ok(DataTexture {
            handle: surface,
            srv: view,
        })
    }

    #[inline(always)]
    pub fn get_size(&self) -> (usize, usize) {
        let (w, h, _, _) = self.handle.get_info().kind.get_dimensions();
        (w as usize, h as usize)
    }
}

pub struct CacheTexture<T> where T: gfx::format::RenderFormat + gfx::format::TextureFormat {
    //pub id: TextureId,
    pub handle: gfx::handle::Texture<R, T::Surface>,
    pub rtv: gfx::handle::RenderTargetView<R, T>,
    pub srv: gfx::handle::ShaderResourceView<R, T::View>,
}

impl<T> CacheTexture<T> where T: gfx::format::RenderFormat + gfx::format::TextureFormat {
    pub fn create<F>(factory: &mut F, size: [usize; 2]) -> Result<CacheTexture<T>, CombinedError>
        where F: gfx::Factory<R>
    {
        let (width, height) = (size[0] as u16, size[1] as u16);
        let tex_kind = Kind::D2Array(width, height, 1, gfx::texture::AaMode::Single);

        let (surface, rtv, view) = {
            let surface = <T::Surface as gfx::format::SurfaceTyped>::get_surface_type();
            let desc = gfx::texture::Info {
                kind: tex_kind,
                levels: 1,
                format: surface,
                bind: gfx::memory::SHADER_RESOURCE | gfx::memory::RENDER_TARGET,
                usage: gfx::memory::Usage::Data,
            };
            let cty = <T::Channel as gfx::format::ChannelTyped>::get_channel_type();
            let raw = try!(factory.create_texture_raw(desc, Some(cty), None));
            let levels = (0, raw.get_info().levels - 1);
            let tex = Typed::new(raw);
            let rtv = try!(factory.view_texture_as_render_target(&tex, 0, None));
            let view = try!(factory.view_texture_as_shader_resource::<T>(&tex, levels, gfx::format::Swizzle::new()));
            (tex, rtv, view)
        };

        Ok(CacheTexture {
            handle: surface,
            rtv: rtv,
            srv: view,
        })
    }

    #[inline(always)]
    pub fn get_size(&self) -> (usize, usize) {
        let (w, h, _, _) = self.handle.get_info().kind.get_dimensions();
        (w as usize, h as usize)
    }
}

pub struct ImageTexture<T> where T: gfx::format::TextureFormat {
    //pub id: TextureId,
    pub handle: gfx::handle::Texture<R, T::Surface>,
    pub srv: gfx::handle::ShaderResourceView<R, T::View>,
    pub filter: TextureFilter,
}

/*pub struct Texture {
    id: u32,
    target: TextureTarget,
    layer_count: i32,
    format: ImageFormat,
    width: u32,
    height: u32,

    filter: TextureFilter,
    mode: RenderTargetMode,

    handle: Option<gfx::handle::Texture<R, R8_G8_B8_A8>>,
    rtv: Option<gfx::handle::RenderTargetView<R, Rgba8>>,
    srv: Option<gfx::handle::ShaderResourceView<R, R8_G8_B8_A8>>,
}

impl Texture {
    pub fn get_dimensions(&self) -> DeviceUintSize {
        DeviceUintSize::new(self.width, self.height)
    }

    pub fn get_layer_count(&self) -> i32 {
        self.layer_count
    }

    pub fn get_bpp(&self) -> u32 {
        match self.format {
            ImageFormat::A8 => 1,
            ImageFormat::RGB8 => 3,
            ImageFormat::BGRA8 => 4,
            ImageFormat::RG8 => 2,
            ImageFormat::RGBAF32 => 16,
            ImageFormat::Invalid => unreachable!(),
        }
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        debug_assert!(thread::panicking() || self.id == 0);
    }
}*/

const MAX_TIMERS_PER_FRAME: usize = 256;
const MAX_SAMPLERS_PER_FRAME: usize = 16;
const MAX_PROFILE_FRAMES: usize = 4;

pub trait NamedTag {
    fn get_label(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct GpuTimer<T> {
    pub tag: T,
    pub time_ns: u64,
}

#[derive(Debug, Clone)]
pub struct GpuSampler<T> {
    pub tag: T,
    pub count: u64,
}

pub struct QuerySet<T> {
    data: Vec<T>,
}

impl<T> QuerySet<T> {
    fn new() -> Self {
        QuerySet {
            data: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.data.clear();
    }

    fn add(&mut self, value: T) {
        self.data.push(value);
    }

    fn take(&mut self) -> Vec<T> {
        let mut data = mem::replace(&mut self.data, Vec::new());
        data
    }
}

pub struct GpuFrameProfile<T> {
    timers: QuerySet<GpuTimer<T>>,
    samplers: QuerySet<GpuSampler<T>>,
    frame_id: FrameId,
    inside_frame: bool,
}

impl<T> GpuFrameProfile<T> {
    fn new() -> Self {
        GpuFrameProfile {
            timers: QuerySet::new(),
            samplers: QuerySet::new(),
            frame_id: FrameId(0),
            inside_frame: false,
        }
    }

    fn begin_frame(&mut self, frame_id: FrameId) {
        self.frame_id = frame_id;
        self.timers.reset();
        self.samplers.reset();
        self.inside_frame = true;
    }

    fn end_frame(&mut self) {
        self.done_marker();
        self.done_sampler();
        self.inside_frame = false;
    }

    fn done_marker(&mut self) {
        debug_assert!(self.inside_frame);
    }

    fn add_marker(&mut self, tag: T) -> GpuMarker where T: NamedTag {
        self.done_marker();

        let marker = GpuMarker::new(tag.get_label());

        self.timers.add(GpuTimer { tag, time_ns: 0 });

        marker
    }

    fn done_sampler(&mut self) {
        /* FIXME: samplers crash on MacOS
        debug_assert!(self.inside_frame);
        if self.samplers.pending != 0 {
            self.gl.end_query(gl::SAMPLES_PASSED);
            self.samplers.pending = 0;
        }
        */
    }

    fn add_sampler(&mut self, _tag: T) where T: NamedTag {
        /* FIXME: samplers crash on MacOS
        self.done_sampler();

        if let Some(query) = self.samplers.add(GpuSampler { tag, count: 0 }) {
            self.gl.begin_query(gl::SAMPLES_PASSED, query);
        }
        */
    }

    fn is_valid(&self) -> bool {
        //!self.timers.set.is_empty() || !self.samplers.set.is_empty()
        true
    }

    fn build_samples(&mut self) -> (Vec<GpuTimer<T>>, Vec<GpuSampler<T>>) {
        debug_assert!(!self.inside_frame);
        (self.timers.take(), self.samplers.take())
    }
}

impl<T> Drop for GpuFrameProfile<T> {
    fn drop(&mut self) {
    }
}

pub struct GpuProfiler<T> {
    frames: [GpuFrameProfile<T>; MAX_PROFILE_FRAMES],
    next_frame: usize,
}

impl<T> GpuProfiler<T> {
    pub fn new() -> GpuProfiler<T> {
        GpuProfiler {
            next_frame: 0,
            frames: [
                GpuFrameProfile::new(),
                GpuFrameProfile::new(),
                GpuFrameProfile::new(),
                GpuFrameProfile::new(),
            ],
        }
    }

    pub fn build_samples(&mut self) -> Option<(FrameId, Vec<GpuTimer<T>>, Vec<GpuSampler<T>>)> {
        let frame = &mut self.frames[self.next_frame];
        if frame.is_valid() {
            let (timers, samplers) = frame.build_samples();
            Some((frame.frame_id, timers, samplers))
        } else {
            None
        }
    }

    pub fn begin_frame(&mut self, frame_id: FrameId) {
        let frame = &mut self.frames[self.next_frame];
        frame.begin_frame(frame_id);
    }

    pub fn end_frame(&mut self) {
        let frame = &mut self.frames[self.next_frame];
        frame.end_frame();
        self.next_frame = (self.next_frame + 1) % MAX_PROFILE_FRAMES;
    }

    pub fn add_marker(&mut self, tag: T) -> GpuMarker
    where T: NamedTag {
        self.frames[self.next_frame].add_marker(tag)
    }

    pub fn add_sampler(&mut self, tag: T)
    where T: NamedTag {
        self.frames[self.next_frame].add_sampler(tag)
    }

    pub fn done_sampler(&mut self) {
        self.frames[self.next_frame].done_sampler()
    }
}

#[must_use]
pub struct GpuMarker{
}

impl GpuMarker {
    pub fn new(message: &str) -> GpuMarker {
        GpuMarker{
        }
    }

    pub fn fire(message: &str) {
    }
}

#[cfg(not(any(target_arch="arm", target_arch="aarch64")))]
impl Drop for GpuMarker {
    fn drop(&mut self) {
    }
}

#[derive(Debug, Copy, Clone)]
pub enum VertexUsageHint {
    Static,
    Dynamic,
    Stream,
}

pub struct Capabilities {
    pub supports_multisampling: bool,
}

#[derive(Clone, Debug)]
pub enum ShaderError {
    Compilation(String, String), // name, error mssage
    Link(String, String), // name, error message
}

#[derive(Debug)]
pub struct BoundTextures {
    pub color0: TextureId,
    pub color1: TextureId,
    pub color2: TextureId,
    pub cache_a8: TextureId,
    pub cache_rgba8: TextureId,
    pub shared_cache_a8: TextureId,
}

pub struct Device {
    pub device: BackendDevice,
    pub factory: backend::Factory,
    pub encoder: gfx::Encoder<R,CB>,
    pub sampler: (Sampler<R>, Sampler<R>),
    pub color0: TextureId,
    pub color1: TextureId,
    pub color2: TextureId,
    pub cache_a8_textures: HashMap<TextureId, CacheTexture<A8>>,
    pub cache_rgba8_textures: HashMap<TextureId, CacheTexture<Rgba8>>,
    pub bound_textures: BoundTextures,
    //pub image_textures: HashMap<TextureId, ImageTexture<Rgba8>>,
    //pub dummy_cache_a8: CacheTexture<A8>,
    //pub dummy_cache_rgba8: CacheTexture<Rgba8>,
    pub layers: DataTexture<Rgba32F>,
    pub render_tasks: DataTexture<Rgba32F>,
    pub resource_cache: DataTexture<Rgba32F>,
    pub main_color: gfx::handle::RenderTargetView<R, ColorFormat>,
    pub main_depth: gfx::handle::DepthStencilView<R, DepthFormat>,
    pub vertex_buffer: gfx::handle::Buffer<R, Position>,
    pub slice: gfx::Slice<R>,

    // device state
    device_pixel_ratio: f32,

    // HW or API capabilties
    capabilities: Capabilities,

    // debug
    inside_frame: bool,

    // resources
    resource_override_path: Option<PathBuf>,

    max_texture_size: u32,

    // Frame counter. This is used to map between CPU
    // frames and GPU frames.
    frame_id: FrameId,
}

impl Device {
    pub fn new(window: Rc<InitWindow>, resource_override_path: Option<PathBuf>) -> (Device, ResultWindow) {
        let max_texture_size = 1024;

        let (win, device, mut factory, main_color, main_depth) = init_existing::<ColorFormat, DepthFormat>(window);
        
        #[cfg(all(target_os = "windows", feature="dx11"))]
        let encoder = factory.create_command_buffer_native().into();

        #[cfg(not(feature = "dx11"))]
        let encoder = factory.create_command_buffer().into();
        
        let (x0, y0, x1, y1) = (0.0, 0.0, 1.0, 1.0);
        let quad_indices: &[u16] = &[ 0, 1, 2, 2, 1, 3 ];
        let quad_vertices = [
            Position::new([x0, y0]),
            Position::new([x1, y0]),
            Position::new([x0, y1]),
            Position::new([x1, y1]),
        ];

        let (vertex_buffer, mut slice) = factory.create_vertex_buffer_with_slice(&quad_vertices, quad_indices);
        slice.instances = Some((MAX_INSTANCE_COUNT as u32, 0));

        let wrap_mode = (gfx::texture::WrapMode::Clamp, gfx::texture::WrapMode::Clamp, gfx::texture::WrapMode::Tile);
        let mut sampler_info = gfx::texture::SamplerInfo::new(gfx::texture::FilterMethod::Scale, gfx::texture::WrapMode::Clamp);
        sampler_info.wrap_mode = wrap_mode;
        let sampler_nearest = factory.create_sampler(sampler_info);
        sampler_info.filter = gfx::texture::FilterMethod::Bilinear;
        let sampler_linear = factory.create_sampler(sampler_info);

        let dummy_cache_a8_tex = CacheTexture::create(&mut factory, [1, 1]).unwrap();
        let dummy_cache_rgba8_tex = CacheTexture::create(&mut factory, [1, 1]).unwrap();
        let layers_tex = DataTexture::create(&mut factory, [LAYER_TEXTURE_WIDTH, 64]).unwrap();
        let render_tasks_tex = DataTexture::create(&mut factory, [RENDER_TASK_TEXTURE_WIDTH, TEXTURE_HEIGTH]).unwrap();
        let resource_cache_tex = DataTexture::create(&mut factory, [max_texture_size, max_texture_size]).unwrap();

        let mut cache_a8_textures = HashMap::new();
        cache_a8_textures.insert(DUMMY_A8, dummy_cache_a8_tex);
        let mut cache_rgba8_textures = HashMap::new();
        cache_rgba8_textures.insert(DUMMY_RGBA8, dummy_cache_rgba8_tex);

        let bound_textures = BoundTextures {
            color0: DUMMY_RGBA8,
            color1: DUMMY_RGBA8,
            color2: DUMMY_RGBA8,
            cache_a8: DUMMY_A8,
            cache_rgba8: DUMMY_RGBA8,
            shared_cache_a8: DUMMY_A8,
        };

        let dev = Device {
            device: device,
            factory: factory,
            encoder: encoder,
            sampler: (sampler_nearest, sampler_linear),
            color0: 0,
            color1: 0,
            color2: 0,
            cache_a8_textures: cache_a8_textures,
            cache_rgba8_textures: cache_rgba8_textures,
            bound_textures: bound_textures,
            //dummy_cache_a8: dummy_cache_a8_tex,
            //dummy_cache_rgba8: dummy_cache_rgba8_tex,
            layers: layers_tex,
            render_tasks: render_tasks_tex,
            resource_cache: resource_cache_tex,
            main_color: main_color,
            main_depth: main_depth,
            vertex_buffer: vertex_buffer,
            slice: slice,
            resource_override_path,
            // This is initialized to 1 by default, but it is set
            // every frame by the call to begin_frame().
            device_pixel_ratio: 1.0,
            inside_frame: false,

            capabilities: Capabilities {
                supports_multisampling: false, //TODO
            },

            max_texture_size: max_texture_size as u32,
            frame_id: FrameId(0),
        };
        (dev, None)
    }

    pub fn dummy_cache_a8(&mut self) -> &CacheTexture<A8> {
        self.cache_a8_textures.get(&DUMMY_A8).unwrap()
    }

    pub fn dummy_cache_rgba8(&mut self) -> &CacheTexture<Rgba8> {
        self.cache_rgba8_textures.get(&DUMMY_RGBA8).unwrap()
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    pub fn get_capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub fn reset_state(&mut self) {
    }

    pub fn begin_frame(&mut self, device_pixel_ratio: f32) -> FrameId {
        debug_assert!(!self.inside_frame);
        self.inside_frame = true;
        self.device_pixel_ratio = device_pixel_ratio;
        self.frame_id
    }

    pub fn bind_texture(&mut self,
                        sampler: TextureSampler,
                        texture: TextureId) {
        debug_assert!(self.inside_frame);

        match sampler {
            TextureSampler::Color0 => self.bound_textures.color0 = texture,
            TextureSampler::Color1 => self.bound_textures.color1 = texture,
            TextureSampler::Color2 => self.bound_textures.color2 = texture,
            TextureSampler::CacheA8 => self.bound_textures.cache_a8 = texture,
            TextureSampler::CacheRGBA8 => self.bound_textures.cache_rgba8 = texture,
            TextureSampler::SharedCacheA8 => self.bound_textures.shared_cache_a8 = texture,
            _ => return
        }
    }

    pub fn generate_texture_id(&mut self) -> TextureId {
        use rand::OsRng;

        let mut rng = OsRng::new().unwrap();
        let mut texture_id = FIRST_UNRESERVED_ID;
        while self.cache_a8_textures.contains_key(&texture_id) ||
              self.cache_rgba8_textures.contains_key(&texture_id) {
            texture_id = rng.gen_range(FIRST_UNRESERVED_ID, u32::max_value());
        }
        texture_id
    }

    pub fn create_cache_texture(&mut self, width: u32, height: u32, kind: RenderTargetKind) -> TextureId
    {
        let id = self.generate_texture_id();
        println!("create_cache_texture={:?}", id);
        match kind {
            RenderTargetKind::Alpha => {
                let tex = CacheTexture::create(&mut self.factory, [width as usize, height as usize]).unwrap();
                self.cache_a8_textures.insert(id, tex);
            }
            RenderTargetKind::Color => {
                let tex = CacheTexture::create(&mut self.factory, [width as usize, height as usize]).unwrap();
                self.cache_rgba8_textures.insert(id, tex);
            }
        }
        id
    }

    pub fn create_texture(&mut self, target: TextureTarget) -> TextureId {
        /*Texture {
            id: 0,
            target: target,
            width: 0,
            height: 0,
            layer_count: 0,
            format: ImageFormat::Invalid,
            filter: TextureFilter::Nearest,
            mode: RenderTargetMode::None,
            handle: None,
            rtv: None,
            srv: None,
        }*/
        0
    }

    pub fn init_texture(&mut self,
                        texture: &TextureId,
                        width: u32,
                        height: u32,
                        format: ImageFormat,
                        filter: TextureFilter,
                        mode: RenderTargetMode,
                        layer_count: i32,
                        pixels: Option<&[u8]>) {
        debug_assert!(self.inside_frame);

        /*let resized = texture.width != width || texture.height != height;

        texture.format = format;
        texture.width = width;
        texture.height = height;
        texture.filter = filter;
        texture.layer_count = layer_count;
        texture.mode = mode;

        match mode {
            RenderTargetMode::RenderTarget => {
            }
            RenderTargetMode::None => {
            }
        }*/
    }

    pub fn free_texture_storage(&mut self, texture: &TextureId) {
        debug_assert!(self.inside_frame);

        /*if texture.format == ImageFormat::Invalid {
            return;
        }

        texture.format = ImageFormat::Invalid;
        texture.width = 0;
        texture.height = 0;
        texture.layer_count = 0;*/
    }

    /*pub fn delete_texture(&mut self, mut texture: Texture) {
        self.free_texture_storage(&mut texture);
        texture.id = 0;
    }*/

    pub fn update_data_texture<T>(&mut self, sampler: TextureSampler, offset: [u16; 2], size: [u16; 2], memory: &[T]) where T: gfx::traits::Pod {
        let img_info = gfx::texture::ImageInfoCommon {
            xoffset: offset[0],
            yoffset: offset[1],
            zoffset: 0,
            width: size[0],
            height: size[1],
            depth: 0,
            format: (),
            mipmap: 0,
        };

        let data = gfx::memory::cast_slice(memory);
        let tex = match sampler {
            TextureSampler::ResourceCache => &self.resource_cache.handle,
            TextureSampler::Layers => &self.layers.handle,
            TextureSampler::RenderTasks => &self.render_tasks.handle,
            _=> unreachable!(),
        };
        self.encoder.update_texture::<_, Rgba32F>(tex, None, img_info, data).unwrap();
    }

    pub fn update_pbo_data<T>(&mut self, data: &[T]) {
        debug_assert!(self.inside_frame);
        //debug_assert_ne!(self.bound_pbo, 0);

        /*gl::buffer_data(&*self.gl,
                        gl::PIXEL_UNPACK_BUFFER,
                        data,
                        gl::STREAM_DRAW);*/
    }

    pub fn update_texture_from_pbo(&mut self,
                                   texture: &TextureId,
                                   x0: u32,
                                   y0: u32,
                                   width: u32,
                                   height: u32,
                                   layer_index: i32,
                                   stride: Option<u32>,
                                   offset: usize) {
        debug_assert!(self.inside_frame);

        /*let (gl_format, bpp, data_type) = match texture.format {
            ImageFormat::A8 => (GL_FORMAT_A, 1, gl::UNSIGNED_BYTE),
            ImageFormat::RGB8 => (gl::RGB, 3, gl::UNSIGNED_BYTE),
            ImageFormat::BGRA8 => (get_gl_format_bgra(self.gl()), 4, gl::UNSIGNED_BYTE),
            ImageFormat::RG8 => (gl::RG, 2, gl::UNSIGNED_BYTE),
            ImageFormat::RGBAF32 => (gl::RGBA, 16, gl::FLOAT),
            ImageFormat::Invalid => unreachable!(),
        };

        let row_length = match stride {
            Some(value) => value / bpp,
            None => width,
        };

        if let Some(..) = stride {
            self.gl.pixel_store_i(gl::UNPACK_ROW_LENGTH, row_length as gl::GLint);
        }

        self.bind_texture(DEFAULT_TEXTURE, texture);

        match texture.target {
            gl::TEXTURE_2D_ARRAY => {
                self.gl.tex_sub_image_3d_pbo(texture.target,
                                             0,
                                             x0 as gl::GLint,
                                             y0 as gl::GLint,
                                             layer_index,
                                             width as gl::GLint,
                                             height as gl::GLint,
                                             1,
                                             gl_format,
                                             data_type,
                                             offset);
            }
            gl::TEXTURE_2D |
            gl::TEXTURE_RECTANGLE |
            gl::TEXTURE_EXTERNAL_OES => {
                self.gl.tex_sub_image_2d_pbo(texture.target,
                                             0,
                                             x0 as gl::GLint,
                                             y0 as gl::GLint,
                                             width as gl::GLint,
                                             height as gl::GLint,
                                             gl_format,
                                             data_type,
                                             offset);
            }
            _ => panic!("BUG: Unexpected texture target!"),
        }

        // Reset row length to 0, otherwise the stride would apply to all texture uploads.
        if let Some(..) = stride {
            self.gl.pixel_store_i(gl::UNPACK_ROW_LENGTH, 0 as gl::GLint);
        }*/
    }

    pub fn end_frame(&mut self) {
        debug_assert!(self.inside_frame);
        self.inside_frame = false;
        self.frame_id.0 += 1;
    }

    pub fn clear_target(&mut self,
                        color: Option<[f32; 4]>,
                        depth: Option<f32>) {
       if let Some(color) = color {
            self.encoder.clear(&self.main_color, [color[0], color[1], color[2], color[3]]);
        }

        if let Some(depth) = depth {
            self.encoder.clear_depth(&self.main_depth, depth);
        }
    }

    pub fn clear_render_target(&mut self, texture_id: &TextureId, color: f32) {
        self.encoder.clear(&self.cache_a8_textures.get(texture_id).unwrap().rtv.clone(), color);
    }

    pub fn flush(&mut self) {
        self.encoder.flush(&mut self.device);
    }
}

#[cfg(not(feature = "dx11"))]
pub fn init_existing<Cf, Df>(window: Rc<InitWindow>) ->
                            (ResultWindow, BackendDevice, backend::Factory,
                             gfx::handle::RenderTargetView<R, Cf>, gfx::handle::DepthStencilView<R, Df>)
where Cf: gfx::format::RenderFormat, Df: gfx::format::DepthFormat,
{
    unsafe { window.make_current().unwrap() };
    let (mut device, factory) = backend::create(|s|
        window.get_proc_address(s) as *const std::os::raw::c_void);

    unsafe { device.with_gl(|ref gl| gl.Disable(GL_FRAMEBUFFER_SRGB)); }

    let (width, height) = window.get_inner_size().unwrap();
    let aa = window.get_pixel_format().multisampling.unwrap_or(0) as gfx::texture::NumSamples;
    let dim = ((width as f32 * window.hidpi_factor()) as gfx::texture::Size,
               (height as f32 * window.hidpi_factor()) as gfx::texture::Size,
               1,
               aa.into());
    let (color_view, ds_view) = backend::create_main_targets_raw(dim, Cf::get_format().0, Df::get_format().0);
    (None, device, factory, Typed::new(color_view), Typed::new(ds_view))
}

#[cfg(all(target_os = "windows", feature="dx11"))]
pub fn init_existing<Cf, Df>(window: Rc<InitWindow>)
    -> (ResultWindow, BackendDevice, backend::Factory,
        gfx::handle::RenderTargetView<R, Cf>,
        gfx::handle::DepthStencilView<R, Df>)
where Cf: gfx::format::RenderFormat, Df: gfx::format::DepthFormat,
{
    let (mut win, device, mut factory, main_color) = gfx_window_dxgi::init_existing_raw(window, Cf::get_format()).unwrap();
    let main_depth = factory.create_depth_stencil_view_only(win.size.0, win.size.1).unwrap();
    let mut device = backend::Deferred::from(device);
    (win, device, factory, gfx::memory::Typed::new(main_color), main_depth)
}