/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use fnv::FnvHasher;
use internal_types::{PackedVertex, RenderTargetMode, TextureSampler, DEFAULT_TEXTURE};
use internal_types::{BlurAttribute, ClipAttribute, VertexAttribute};
use internal_types::{DebugFontVertex, DebugColorVertex};
//use notify::{self, Watcher};
use super::shader_source;
use std::collections::HashMap;
use std::fs::File;
use std::hash::BuildHasherDefault;
use std::io::Read;
use std::iter::repeat;
use std::mem;
use std::ops::Add;
use std::path::PathBuf;
use std::ptr;
use std::rc::Rc;
//use std::sync::mpsc::{channel, Sender};
//use std::thread;
use webrender_traits::{ColorF, ImageFormat};
use webrender_traits::{DeviceIntPoint, DeviceIntRect, DeviceIntSize, DeviceUintSize};

//use euclid::Matrix4D;

use rand::Rng;
use std;
use gfx;
use gfx::memory::Typed;
use gfx::Factory;
use gfx::texture::Kind;
use gfx::traits::FactoryExt;
use gfx::format::{DepthStencil as DepthFormat, Rgba8 as ColorFormat};

use backend;
use window;
use WrapperWindow;


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

use gfx::CombinedError;
use gfx::format::{Formatted, R8, Rgba8, Rgba32F, Srgba8, SurfaceTyped, TextureChannel, TextureSurface, Unorm};
use gfx::format::{R8_G8_B8_A8, R32_G32_B32_A32};
use pipelines::{primitive, ClipProgram, Position, PrimitiveInstances, Program, Locals};
use prim_store::GRADIENT_DATA_SIZE;
use tiling::{CacheClipInstance, PrimitiveInstance};
use renderer::{BlendMode, DITHER_ID, DUMMY_A8_ID, DUMMY_RGBA8_ID, MAX_VERTEX_TEXTURE_WIDTH};
use webrender_traits::DeviceUintRect;

pub type A8 = (R8, Unorm);
pub const LAYER_TEXTURE_WIDTH: usize = 1017;
pub const RENDER_TASK_TEXTURE_WIDTH: usize = 1023;
pub const TEXTURE_HEIGTH: usize = 8;
pub const DEVICE_PIXEL_RATIO: f32 = 1.0;
pub const MAX_INSTANCE_COUNT: usize = 5000;

pub const A_STRIDE: usize = 1;
pub const RG_STRIDE: usize = 2;
pub const RGB_STRIDE: usize = 3;
pub const RGBA_STRIDE: usize = 4;
pub const FIRST_UNRESERVED_ID: u32 = DITHER_ID + 1;
// The value of the type GL_FRAMEBUFFER_SRGB from https://www.khronos.org/registry/OpenGL/extensions/ARB/ARB_framebuffer_sRGB.txt
const GL_FRAMEBUFFER_SRGB: u32 = 0x8DB9;

#[derive(Clone, Debug, PartialEq)]
pub struct Texture<R, T> where R: gfx::Resources,
                               T: gfx::format::TextureFormat {
    pub handle: gfx::handle::Texture<R, T::Surface>,
    pub rtv: Option<gfx::handle::RenderTargetView<R, T>>,
    pub srv: gfx::handle::ShaderResourceView<R, T::View>,
    //pub dsv: gfx::handle::DepthStencilView<R, DepthFormat>,
}

impl<R, T> Texture<R, T> where R: gfx::Resources, T: gfx::format::RenderFormat + gfx::format::TextureFormat {

    pub fn empty<F>(factory: &mut F, size: [usize; 2], texture_kind: TextureTarget, flags: gfx::Bind, usage: gfx::memory::Usage) -> Result<Texture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        Texture::create(factory, None, size, texture_kind, flags, usage)
    }

    pub fn create<F>(factory: &mut F,
                     data: Option<&[&[u8]]>,
                     size: [usize; 2],
                     texture_kind: TextureTarget,
                     flags: gfx::Bind,
                     usage: gfx::memory::Usage
    ) -> Result<Texture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        let (width, height) = (size[0] as u16, size[1] as u16);
        let tex_kind = match texture_kind {
            TextureTarget::Array => Kind::D2Array(width, height, 1, gfx::texture::AaMode::Single),
            _ => Kind::D2(width, height, gfx::texture::AaMode::Single),
        };
        let cty = <T::Channel as gfx::format::ChannelTyped>::get_channel_type();
        let tex = try!(factory.create_texture(tex_kind,
                                              1,
                                              flags,
                                              //gfx::memory::RENDER_TARGET |
                                              //gfx::memory::SHADER_RESOURCE /*|
                                              //gfx::memory::TRANSFER_SRC*/,
                                              //gfx::memory::Usage::Data,
                                              //gfx::memory::Usage::Dynamic,
                                              usage,
                                              Some(cty)));
        
        let rtv = if flags.contains(gfx::memory::RENDER_TARGET) {
             Some(try!(factory.view_texture_as_render_target(&tex, 0, None)))
        } else {
            None
        };
        let levels = (0, tex.get_info().levels - 1);
        let srv = try!(factory.view_texture_as_shader_resource::<T>(&tex, levels, gfx::format::Swizzle::new()));

        //let dsv_cty = gfx::format::ChannelType::Unorm;
        //let tex_dsv = try!(factory.create_texture(tex_kind, 1, gfx::memory::SHADER_RESOURCE | gfx::memory::DEPTH_STENCIL, gfx::memory::Usage::Data, Some(dsv_cty)));
        //let dsv = try!(factory.view_texture_as_depth_stencil_trivial(&tex_dsv));
        //let dsv = try!(factory.create_depth_stencil_view_only(width, height));
        Ok(Texture {
            handle: tex,
            rtv: rtv,
            srv: srv,
            //dsv: dsv,
        })
    }

    #[inline(always)]
    pub fn get_size(&self) -> (usize, usize) {
        let (w, h, _, _) = self.handle.get_info().kind.get_dimensions();
        (w as usize, h as usize)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataTexture<R, T> where R: gfx::Resources,
                               T: gfx::format::TextureFormat {
    pub surface: gfx::handle::Texture<R, T::Surface>,
    pub sampler: gfx::handle::Sampler<R>,
    pub view: gfx::handle::ShaderResourceView<R, T::View>,
    pub filter: TextureFilter,
    pub format: ImageFormat,
    pub mode: RenderTargetMode,
}

impl<R, T> DataTexture<R, T> where R: gfx::Resources, T: gfx::format::TextureFormat {

    pub fn empty<F>(factory: &mut F, size: [usize; 2], filter_method: TextureFilter, texture_kind: TextureTarget) -> Result<DataTexture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        DataTexture::create(factory, None, size, filter_method, texture_kind)
    }

    pub fn create<F>(factory: &mut F,
                     data: Option<&[&[u8]]>,
                     size: [usize; 2],
                     filter: TextureFilter,
                     texture_kind: TextureTarget,
    ) -> Result<DataTexture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        let (width, height) = (size[0] as u16, size[1] as u16);
        let tex_kind = match texture_kind {
            TextureTarget::Array => Kind::D2Array(width, height, 1, gfx::texture::AaMode::Single),
            _ => Kind::D2(width, height, gfx::texture::AaMode::Single),
        };

        let filter_method = match filter {
            TextureFilter::Nearest => gfx::texture::FilterMethod::Scale,
            TextureFilter::Linear => gfx::texture::FilterMethod::Bilinear,
        };

        let mut sampler_info = gfx::texture::SamplerInfo::new(
            filter_method,
            gfx::texture::WrapMode::Clamp
        );
        sampler_info.wrap_mode = (gfx::texture::WrapMode::Clamp, gfx::texture::WrapMode::Clamp, gfx::texture::WrapMode::Tile);

        let (surface, view, format) = {
            use gfx::{format, texture};
            use gfx::memory::{Usage, SHADER_RESOURCE};

            let surface = <T::Surface as format::SurfaceTyped>::get_surface_type();
            let desc = texture::Info {
                kind: tex_kind,
                levels: 1,
                format: surface,
                bind: SHADER_RESOURCE,
                usage: Usage::Dynamic,
            };
            let cty = <T::Channel as format::ChannelTyped>::get_channel_type();
            let raw = try!(factory.create_texture_raw(desc, Some(cty), data));
            let levels = (0, raw.get_info().levels - 1);
            let tex = Typed::new(raw);
            let view = try!(factory.view_texture_as_shader_resource::<T>(
                &tex, levels, format::Swizzle::new()
            ));
            let format = match surface {
                gfx::format::SurfaceType::R8 => ImageFormat::A8,
                gfx::format::SurfaceType::R8_G8_B8_A8 | gfx::format::SurfaceType::B8_G8_R8_A8 => ImageFormat::BGRA8,
                gfx::format::SurfaceType::R32_G32_B32_A32 => ImageFormat::RGBAF32,
                _ => unimplemented!(),
            };
            (tex, view, format)
        };

        let sampler = factory.create_sampler(sampler_info);

        Ok(DataTexture {
            surface: surface,
            sampler: sampler,
            view: view,
            filter: filter,
            format: format,
            mode: RenderTargetMode::None,
        })
    }

    #[inline(always)]
    pub fn get_size(&self) -> (usize, usize) {
        let (w, h, _, _) = self.surface.get_info().kind.get_dimensions();
        (w as usize, h as usize)
    }
}

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

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TextureTarget {
    Default,
    Array,
    External,
    Rect,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TextureFilter {
    Nearest,
    Linear,
}

impl TextureId {
    pub fn new(name: u32, _: TextureTarget) -> TextureId {
        TextureId {
            name: name,
        }
    }

    pub fn invalid() -> TextureId {
        TextureId {
            name: 0,
        }
    }

    pub fn invalid_a8() -> TextureId {
        TextureId {
            name: 1,
        }
    }

    pub fn is_valid(&self) -> bool { !(*self == TextureId::invalid() || *self == TextureId::invalid_a8()) }
    pub fn is_dummy(&self) -> bool { self.name == DUMMY_A8_ID || self.name == DUMMY_RGBA8_ID }
    pub fn is_skipable(&self) -> bool { !(self.is_valid()) || self.is_dummy() }
}

#[derive(PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Copy, Clone)]
pub struct TextureId {
    pub name: u32,
}

/*#[derive(Debug)]
pub struct TextureData {
    pub id: TextureId,
    pub data: Vec<u8>,
    pub stride: usize,
    pub pitch: usize,
}*/

#[derive(Clone, Debug)]
pub enum ShaderError {
    Compilation(String, String), // name, error mssage
    Link(String), // error message
}

pub struct Device {
    pub device: BackendDevice,
    pub factory: backend::Factory,
    pub encoder: gfx::Encoder<R,CB>,
    pub textures: HashMap<TextureId, Texture<R, Rgba8>>,
    pub dither: DataTexture<R, A8>,
    pub dummy_tex: Texture<R, Rgba8>,
    pub sampler: gfx::handle::Sampler<R>,
    pub color0_tex_id: TextureId,
    pub color1_tex_id: TextureId,
    pub color2_tex_id: TextureId,
    pub cache_a8_tex_id: TextureId,
    pub cache_rgba8_tex_id: TextureId,
    pub layers: DataTexture<R, Rgba32F>,
    pub render_tasks: DataTexture<R, Rgba32F>,
    pub resource_cache: DataTexture<R, Rgba32F>,
    pub max_texture_size: u32,
    pub main_color: gfx::handle::RenderTargetView<R, ColorFormat>,
    pub main_depth: gfx::handle::DepthStencilView<R, DepthFormat>,
    pub vertex_buffer: gfx::handle::Buffer<R, Position>,
    pub slice: gfx::Slice<R>,
    pub frame_id: FrameId,
}

impl Device {
    pub fn new(window: Rc<window::Window>) -> (Device, WrapperWindow) {
        #[cfg(all(target_os = "windows", feature="dx11"))]
        let (win, device, mut factory, main_color, main_depth) = init_existing(window);
        #[cfg(not(feature = "dx11"))]
        let (win, device, mut factory, main_color, main_depth) = init_existing::<ColorFormat, DepthFormat>(window);
        /*println!("Vendor: {:?}", device.get_info().platform_name.vendor);
        println!("Renderer: {:?}", device.get_info().platform_name.renderer);
        println!("Version: {:?}", device.get_info().version);
        println!("Shading Language: {:?}", device.get_info().shading_language);*/

        #[cfg(all(target_os = "windows", feature="dx11"))]
        let encoder = factory.create_command_buffer_native().into();

        #[cfg(not(feature = "dx11"))]
        let encoder = factory.create_command_buffer().into();

        let max_texture_size = MAX_VERTEX_TEXTURE_WIDTH as u32;

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

        let (w, h, _, _) = main_color.get_dimensions();
        //let texture_size = [std::cmp::max(MAX_VERTEX_TEXTURE_WIDTH, h as usize), std::cmp::max(MAX_VERTEX_TEXTURE_WIDTH, w as usize)];
        
        // TODO define some maximum boundaries for texture height
        let layers_tex = DataTexture::empty(&mut factory, [LAYER_TEXTURE_WIDTH, 64], TextureFilter::Nearest, TextureTarget::Default).unwrap();
        let render_tasks_tex = DataTexture::empty(&mut factory, [RENDER_TASK_TEXTURE_WIDTH, TEXTURE_HEIGTH], TextureFilter::Nearest, TextureTarget::Default).unwrap();
        let resource_cache = DataTexture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH, MAX_VERTEX_TEXTURE_WIDTH], TextureFilter::Nearest, TextureTarget::Default).unwrap();

        let mut textures = HashMap::new();
        let dummy_tex: Texture<R, Rgba8> = Texture::empty(&mut factory, [1,1], TextureTarget::Default, gfx::memory::SHADER_RESOURCE | gfx::memory::RENDER_TARGET, gfx::memory::Usage::Data).unwrap();
        textures.insert(TextureId::invalid(), dummy_tex.clone());
        textures.insert(TextureId::invalid_a8(), dummy_tex.clone());
        textures.insert(TextureId { name: DUMMY_RGBA8_ID }, dummy_tex.clone());
        textures.insert(TextureId { name: DUMMY_A8_ID }, dummy_tex.clone());
        let dither_matrix = vec![
            00, 48, 12, 60, 03, 51, 15, 63,
            32, 16, 44, 28, 35, 19, 47, 31,
            08, 56, 04, 52, 11, 59, 07, 55,
            40, 24, 36, 20, 43, 27, 39, 23,
            02, 50, 14, 62, 01, 49, 13, 61,
            34, 18, 46, 30, 33, 17, 45, 29,
            10, 58, 06, 54, 09, 57, 05, 53,
            42, 26, 38, 22, 41, 25, 37, 21
        ];
        let dither = DataTexture::create(&mut factory, None, [8, 8], TextureFilter::Nearest, TextureTarget::Default).unwrap();

        //textures.insert(TextureId { name: DITHER_ID }, dither.surface);

        let mut sampler_info = gfx::texture::SamplerInfo::new(
            gfx::texture::FilterMethod::Scale,
            gfx::texture::WrapMode::Clamp
        );
        sampler_info.wrap_mode = (gfx::texture::WrapMode::Clamp, gfx::texture::WrapMode::Clamp, gfx::texture::WrapMode::Tile);
        let sampler = factory.create_sampler(sampler_info);

        let dev = Device {
            device: device,
            factory: factory,
            encoder: encoder,
            textures: textures,
            dither: dither,
            dummy_tex: dummy_tex,
            sampler: sampler,
            color0_tex_id: TextureId { name: DUMMY_RGBA8_ID },
            color1_tex_id: TextureId { name: DUMMY_RGBA8_ID },
            color2_tex_id: TextureId { name: DUMMY_RGBA8_ID },
            cache_a8_tex_id: TextureId { name: DUMMY_A8_ID },
            cache_rgba8_tex_id: TextureId { name: DUMMY_RGBA8_ID },
            layers: layers_tex,
            render_tasks: render_tasks_tex,
            resource_cache: resource_cache,
            max_texture_size: max_texture_size,
            main_color: main_color,
            main_depth: main_depth,
            vertex_buffer: vertex_buffer,
            slice: slice,
            frame_id: FrameId::new(0),
        };
        (dev, win)
    }

    pub fn read_pixels(&mut self, rect: DeviceUintRect, output: &mut [u8]) {
        // TODO add bgra flag
        self.encoder.flush(&mut self.device);
        let tex = self.main_color.raw().get_texture();
        let tex_info = tex.get_info().to_raw_image_info(gfx::format::ChannelType::Unorm, 0);
        let (w, h, _, _) = self.main_color.get_dimensions();
        let buf = self.factory.create_buffer::<u8>(w as usize * h as usize * RGBA_STRIDE,
                                                   gfx::buffer::Role::Vertex,
                                                   gfx::memory::Usage::Download,
                                                   gfx::TRANSFER_DST).unwrap();
        self.encoder.copy_texture_to_buffer_raw(tex, None, tex_info, buf.raw(), 0).unwrap();
        self.encoder.flush(&mut self.device);
        {
            let reader = self.factory.read_mapping(&buf).unwrap();
            let data = &*reader;
            for j in 0..rect.size.height as usize {
                for i in 0..rect.size.width as usize {
                    let offset = i * RGBA_STRIDE + j * rect.size.width as usize * RGBA_STRIDE;
                    let src = &data[(j + rect.origin.y as usize) * w as usize * RGBA_STRIDE + (i + rect.origin.x as usize) * RGBA_STRIDE ..];
                    output[offset + 0] = src[0];
                    output[offset + 1] = src[1];
                    output[offset + 2] = src[2];
                    output[offset + 3] = src[3];
                }
            }
        }
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    pub fn generate_texture_id(&mut self) -> TextureId {
        use rand::OsRng;

        let mut rng = OsRng::new().unwrap();
        let mut texture_id = TextureId::invalid();
        while self.textures.contains_key(&texture_id) {
            texture_id.name = rng.gen_range(FIRST_UNRESERVED_ID, u32::max_value());
        }
        texture_id
    }

    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        //_filter: TextureFilter,
        target: TextureTarget,
        flags: gfx::Bind,
        usage: gfx::memory::Usage) -> TextureId
    {
        let texture_id = self.generate_texture_id();
        assert!(!self.textures.contains_key(&texture_id));
        let tex = Texture::empty(&mut self.factory, [width as usize, height as usize], target, flags, usage).unwrap();
        self.textures.insert(texture_id, tex);
        texture_id
    }

    pub fn create_empty_texture(&mut self,
                                width: u32,
                                height: u32,
                                _filter: TextureFilter,
                                target: TextureTarget) -> TextureId {
        println!("create_empty_texture w={:?} h={:?}", width, height);
        self.create_texture(width, height, target, gfx::memory::SHADER_RESOURCE | gfx::memory::TRANSFER_DST, gfx::memory::Usage::Dynamic)
    }

    pub fn create_cache_texture(&mut self,
                                width: u32,
                                height: u32) -> TextureId {
        println!("create_cache_texture w={:?} h={:?}", width, height);
        self.create_texture(width, height, TextureTarget::Array, gfx::memory::SHADER_RESOURCE | gfx::memory::RENDER_TARGET, gfx::memory::Usage::Data)
    }

    pub fn update_texture(&mut self,
                          texture_id: TextureId,
                          x0: u32,
                          y0: u32,
                          width: u32,
                          height: u32,
                          format: ImageFormat,
                          stride: Option<u32>,
                          pixels: Option<&[u8]>) {
        println!("update_texture");
        println!("texture_id={:?} x0={:?} y0={:?} width={:?} height={:?} format={:?} stride={:?} pixels={:?}",
                  texture_id, x0, y0, width, height, format, stride, pixels.is_some());
        if pixels.is_none() {
            //TODO set format
            return;
        }

        let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        let data = match format {
            ImageFormat::A8 => Device::convert_data_to_rgba8(width as usize, height as usize, pixels.unwrap(), A_STRIDE),
            ImageFormat::RG8 => Device::convert_data_to_rgba8(width as usize, height as usize, pixels.unwrap(), RG_STRIDE),
            ImageFormat::RGB8 => Device::convert_data_to_rgba8(width as usize, height as usize, pixels.unwrap(), RGB_STRIDE),
            ImageFormat::BGRA8 => {
                let row_length = match stride {
                    Some(value) => value as usize / RGBA_STRIDE,
                    None => width as usize,
                };
                // Take the stride into account for all rows, except the last one.
                let data_pitch = row_length * RGBA_STRIDE;
                let len = data_pitch * (height - 1) as usize + width as usize * RGBA_STRIDE;
                let pixels = pixels.unwrap();
                let data = &pixels[0 .. len];
                Device::convert_data_to_bgra8(width as usize, height as usize, data_pitch,data)
            }
            _ => unimplemented!(),
        };
        Device::update_texture_data(&mut self.encoder, &texture, [x0 as usize, y0 as usize], [width as usize, height as usize], data.as_slice());
    }

    /*pub fn init_texture(&mut self,
                        texture_id: TextureId,
                        _width: u32,
                        _height: u32,
                        format: ImageFormat,
                        _filter: TextureFilter,
                        _mode: RenderTargetMode,
                        pixels: Option<&[u8]>) {
        println!("init_texture {:?}", texture_id);
        //println!("init_texture texture_id={:?} _width={:?} _height={:?} format={:?} _filter={:?} _mode={:?}", texture_id, _width, _height, format, _filter, _mode);
        /*let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        //println!("init_texture texture.stride={:?} texture.data.len={:?}", texture.stride, texture.data.len());
        let stride = match format {
            ImageFormat::A8 => A_STRIDE,
            ImageFormat::BGRA8 => RGBA_STRIDE,
            ImageFormat::RG8 => RG_STRIDE,
            ImageFormat::RGB8 => RGB_STRIDE,
            _ => unimplemented!(),
        };
        if stride != texture.stride {
            texture.stride = stride;
            texture.data.clear();
        }
        let actual_pixels = match pixels {
            Some(data) => data.to_vec(),
            None => {
                let (w, h) = self.color0.get_size();
                vec![0u8; w * h * texture.stride]
            },
        };
        assert!(texture.data.len() == actual_pixels.len());
        mem::replace(&mut texture.data, actual_pixels);*/
    }*/

    /*pub fn update_texture(&mut self,
                          texture_id: TextureId,
                          x0: u32,
                          y0: u32,
                          width: u32,
                          height: u32,
                          stride: Option<u32>,
                          data: &[u8]) {
        println!("update_texture {:?}", texture_id);
        /*//println!("update {:?} x0={:?} y0={:?} width={:?} height={:?} stride={:?} size={:?}", texture_id, x0, y0, width, height, stride, data.len());
        let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        assert!(!(texture.data.len() < data.len()));
        let row_length = match stride {
            Some(value) => value as usize / texture.stride,
            None => width as usize,
        };
        // Take the stride into account for all rows, except the last one.
        let data_pitch = row_length * texture.stride;
        let len = data_pitch * (height - 1) as usize + width as usize * texture.stride;
        let data = &data[0 .. len];
        Device::update_texture_data(texture, x0 as usize, y0 as usize, width as usize, height as usize, data_pitch, data);*/
    }*/

    pub fn resize_texture(&mut self,
                          texture_id: TextureId,
                          new_width: u32,
                          new_height: u32,
                          format: ImageFormat,
                          _filter: TextureFilter,
                          _mode: RenderTargetMode) {
        println!("resize_texture {:?}", texture_id);
        /*let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        let stride = match format {
            ImageFormat::A8 => A_STRIDE,
            ImageFormat::BGRA8 => RGBA_STRIDE,
            ImageFormat::RG8 => RG_STRIDE,
            ImageFormat::RGB8 => RGB_STRIDE,
            _ => unimplemented!(),
        };
        if stride != texture.stride {
            texture.stride = stride;
            texture.data.clear();
        }
        let new_len = new_width as usize * new_height as usize * texture.stride;
        texture.data.resize(new_len, 0u8);*/
    }

    pub fn deinit_texture(&mut self, texture_id: TextureId) {
        println!("deinit_texture {:?}", texture_id);
        /*let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        let (w, h) = self.color0.get_size();
        let data = vec![0u8; w * h * texture.stride];
        assert!(texture.data.len() == data.len());
        mem::replace(&mut texture.data, data.to_vec());*/
    }

    fn convert_data_to_bgra8(width: usize, height: usize, data_pitch: usize, data: &[u8]) -> Vec<u8> {
        let mut new_data = vec![0u8; width * height * RGBA_STRIDE];
        for j in 0..height {
            for i in 0..width {
                let offset = i*RGBA_STRIDE + j*RGBA_STRIDE*width;
                let src = &data[j * data_pitch + i * RGBA_STRIDE ..];
                assert!(offset + 3 < new_data.len()); // optimization
                // convert from BGRA
                new_data[offset + 0] = src[2];
                new_data[offset + 1] = src[1];
                new_data[offset + 2] = src[0];
                new_data[offset + 3] = src[3];
            }
        }
        return new_data;
    }
    /*fn update_texture_data(texture: &mut TextureData,
        x_offset: usize, y_offset: usize,
        width: usize, height: usize,
        data_pitch: usize, new_data: &[u8]
    ) {
        assert_eq!(data_pitch * (height-1) + width * texture.stride, new_data.len());
        for j in 0..height {
            if texture.stride != RGBA_STRIDE {
                //fast path
                let dst_offset = x_offset*texture.stride + (j+y_offset)*texture.pitch;
                let src = &new_data[j * data_pitch ..];
                texture.data[dst_offset .. dst_offset + width*texture.stride].copy_from_slice(&src[.. width*texture.stride]);
                continue;
            }
            for i in 0..width {
                let offset = (i + x_offset)*texture.stride + (j+y_offset)*texture.pitch;
                let src = &new_data[j * data_pitch + i * texture.stride ..];
                assert!(offset + 3 < texture.data.len()); // optimization
                // convert from BGRA
                texture.data[offset + 0] = src[2];
                texture.data[offset + 1] = src[1];
                texture.data[offset + 2] = src[0];
                texture.data[offset + 3] = src[3];
            }
        }
    }*/

    pub fn bind_texture(&mut self,
                        sampler: TextureSampler,
                        texture_id: TextureId) {
        println!("bind_texture texture_id={:?}", texture_id);
        /*if texture_id.is_skipable() {
            return;
        }*/
        let texture = match self.textures.get(&texture_id) {
            Some(data) => data,
            None => {
                println!("Didn't find texture! {}", texture_id.name);
                return;
            }
        };

        match sampler {
            TextureSampler::Color0 => self.color0_tex_id = texture_id,
            TextureSampler::Color1 => self.color1_tex_id = texture_id,
            TextureSampler::Color2 => self.color2_tex_id = texture_id,
            TextureSampler::CacheA8 => self.cache_a8_tex_id = texture_id,
            TextureSampler::CacheRGBA8 => self.cache_rgba8_tex_id = texture_id,
            TextureSampler::Dither => unreachable!("The dither sampler should be inicialised at startup"),
            _ => println!("There are only 5 samplers supported. {:?}", sampler),
        }

        //println!("bind_texture {:?} {:?} {:?} {:?}", texture_id, sampler, texture.stride, texture.data.len());
        //println!("texture.data={:?}", &texture.data[0..64]);
        /*match sampler {
            TextureSampler::Color0 => Device::update_texture_surface(&mut self.encoder, &self.color0, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Color1 => Device::update_texture_surface(&mut self.encoder, &self.color1, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Color2 => Device::update_texture_surface(&mut self.encoder, &self.color2, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::CacheA8 => Device::update_texture_surface(&mut self.encoder, &self.cache_a8, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::CacheRGBA8 => Device::update_texture_surface(&mut self.encoder, &self.cache_rgba8, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Dither => Device::update_texture_surface(&mut self.encoder, &self.dither, texture.data.as_slice(), A_STRIDE),
            _ => println!("There are only 5 samplers supported. {:?}", sampler),
        }*/
    }

    /*pub fn bind_yuv_texture(&mut self,
                            sampler: TextureSampler,
                            texture_id: TextureId) {
        let texture = match self.textures.get_mut(&texture_id) {
            Some(data) => data,
            None => {
                println!("Didn't find texture! {}", texture_id.name);
                return;
            }
        };

        let (w, h) = self.color0.get_size();
        let new_data = Device::convert_data_to_rgba8(w, h, texture.data.as_slice(), texture.stride);
        match sampler {
            TextureSampler::Color0 => Device::update_texture_surface(&mut self.encoder, &self.color0, new_data.as_slice(), RGBA_STRIDE),
            TextureSampler::Color1 => Device::update_texture_surface(&mut self.encoder, &self.color1, new_data.as_slice(), RGBA_STRIDE),
            TextureSampler::Color2 => Device::update_texture_surface(&mut self.encoder, &self.color2, new_data.as_slice(), RGBA_STRIDE),
            _ => println!("The yuv image shouldn't use this sampler: {:?}", sampler),
        }
    }*/

    pub fn convert_data_to_rgba8(width: usize, height: usize, data: &[u8], orig_stride: usize) -> Vec<u8> {
        let mut new_data = vec![0u8; width * height * RGBA_STRIDE];
        for s in 0..orig_stride {
            for h in 0..height {
                for w in 0..width {
                    new_data[s+(w*RGBA_STRIDE)+h*width*RGBA_STRIDE] = data[s+(w*orig_stride)+h*width*orig_stride];
                }
            }
        }
        return new_data;
    }

    #[cfg(all(target_os = "windows", feature="dx11"))]
    pub fn update_gpu_cache(&mut self, data: &[f32]) {
        Device::update_texture_surface(&mut self.encoder, &self.resource_cache, data, RGBA_STRIDE);
    }

    #[cfg(not(feature = "dx11"))]
    pub fn update_gpu_cache(&mut self, row_index: u16, data: &[f32]) {
        Device::update_gpu_texture(&mut self.encoder, &self.resource_cache, row_index, data);
    }

    pub fn update_sampler_f32(&mut self,
                              sampler: TextureSampler,
                              data: &[f32]) {
        match sampler {
            TextureSampler::Layers => Device::update_texture_surface(&mut self.encoder, &self.layers, data, RGBA_STRIDE),
            TextureSampler::RenderTasks => Device::update_texture_surface(&mut self.encoder, &self.render_tasks, data, RGBA_STRIDE),
            _ => println!("{:?} sampler is not supported", sampler),
        }
    }

    pub fn clear_target(&mut self, color: Option<[f32; 4]>, depth: Option<f32>) {
        if let Some(color) = color {
            self.encoder.clear(&self.main_color, [color[0], color[1], color[2], color[3]]);
        }

        if let Some(depth) = depth {
            self.encoder.clear_depth(&self.main_depth, depth);
        }
    }

    pub fn clear_render_target(&mut self, texture_id: TextureId, color: [f32; 4]) {
        let tex = self.textures.get(&texture_id).unwrap().clone();
        let rtv = tex.rtv.unwrap().clone();
        self.encoder.clear(&rtv, [color[0], color[1], color[2], color[3]]);
    }

    pub fn flush(&mut self) {
        self.encoder.flush(&mut self.device);
    }

    pub fn update_gpu_texture<S, F, T>(encoder: &mut gfx::Encoder<R,CB>,
                                       texture: &DataTexture<R, F>,
                                       row_index: u16,
                                       memory: &[T])
    where S: SurfaceTyped + TextureSurface,
          S::DataType: Copy,
          F: Formatted<Surface=S>,
          F::Channel: TextureChannel,
          T: Default + Clone + gfx::traits::Pod {
        //println!("update_gpu_texture row_index={:?} memory.len={:?}", row_index, memory.len());
        assert!(memory.len() == MAX_VERTEX_TEXTURE_WIDTH * RGBA_STRIDE);
        let img_info = gfx::texture::ImageInfoCommon {
            xoffset: 0,
            yoffset: row_index,
            zoffset: 0,
            width: MAX_VERTEX_TEXTURE_WIDTH as u16,
            height: 1,
            depth: 0,
            format: (),
            mipmap: 0,
        };

        let data = gfx::memory::cast_slice(memory);
        encoder.update_texture::<_, F>(&texture.surface, None, img_info, data).unwrap();
    }

    pub fn update_texture_surface<S, F, T>(encoder: &mut gfx::Encoder<R,CB>,
                                           texture: &DataTexture<R, F>,
                                           memory: &[T],
                                           stride: usize)
    where S: SurfaceTyped + TextureSurface,
          S::DataType: Copy,
          F: Formatted<Surface=S>,
          F::Channel: TextureChannel,
          T: Default + Clone + gfx::traits::Pod {
        let (width, height) = texture.get_size();
        let resized_data = Device::convert_sampler_data(memory, (width * height * stride) as usize);
        let img_info = gfx::texture::ImageInfoCommon {
            xoffset: 0,
            yoffset: 0,
            zoffset: 0,
            width: width as u16,
            height: height as u16,
            depth: 0,
            format: (),
            mipmap: 0,
        };

        let data = gfx::memory::cast_slice(resized_data.as_slice());
        encoder.update_texture::<_, F>(&texture.surface, None, img_info, data).unwrap();
    }

    fn convert_sampler_data<T: Default + Clone>(data: &[T], max_size: usize) -> Vec<T> {
        let mut data = data.to_vec();
        let len = data.len();
        if len < max_size {
            data.extend_from_slice(&vec![T::default(); max_size - len]);
        }
        assert!(data.len() == max_size);
        data
    }

pub fn update_texture_data<S, F, T>(encoder: &mut gfx::Encoder<R,CB>,
                                    texture: &Texture<R, F>,
                                    offset: [usize; 2],
                                    size: [usize; 2],
                                    memory: &[T])
    where S: SurfaceTyped + TextureSurface,
          S::DataType: Copy,
          F: Formatted<Surface=S>,
          F::Channel: TextureChannel,
          T: Default + Clone + gfx::traits::Pod {
        //let (width, height) = texture.get_size();
        let resized_data = Device::convert_sampler_data(memory, (size[0] * size[1] * RGBA_STRIDE) as usize);
        //assert!(size[0] * size[1] * RGBA_STRIDE == memory.len());
        let img_info = gfx::texture::ImageInfoCommon {
            xoffset: offset[0] as u16,
            yoffset: offset[1] as u16,
            zoffset: 0,
            width: size[0] as u16,
            height: size[1] as u16,
            depth: 0,
            format: (),
            mipmap: 0,
        };

        //let data = gfx::memory::cast_slice(memory);
        let data = gfx::memory::cast_slice(resized_data.as_slice());
        encoder.update_texture::<_, F>(&texture.handle, None, img_info, data).unwrap();
    }

    pub fn begin_frame(&self) -> FrameId {
        self.frame_id
    }

    pub fn end_frame(&mut self) {
        self.frame_id.0 += 1;
    }
}

/*fn create_main_targets<Cf: gfx::format::RenderFormat + gfx::format::TextureFormat,
                       Df: gfx::format::DepthFormat + gfx::format::TextureFormat>
                       (factory: &mut backend::Factory, width: gfx::texture::Size, height: gfx::texture::Size)
                        -> Result<(gfx::handle::RenderTargetView<R, Cf>,
                                   gfx::handle::DepthStencilView<R, Df>), gfx::CombinedError>
{
    let kind = gfx::texture::Kind::D2(width, height, gfx::texture::AaMode::Single);
    let levels = 1;
    let rtv_cty = <Cf::Channel as gfx::format::ChannelTyped>::get_channel_type();
    let tex_rtv = try!(factory.create_texture(kind, levels, gfx::memory::RENDER_TARGET | gfx::memory::TRANSFER_SRC, gfx::memory::Usage::Data, Some(rtv_cty)));
    let rtv = try!(factory.view_texture_as_render_target(&tex_rtv, 0, None));

    let dsv_cty = <Df::Channel as gfx::format::ChannelTyped>::get_channel_type();
    let tex_dsv = try!(factory.create_texture(kind, levels, gfx::memory::DEPTH_STENCIL, gfx::memory::Usage::Data, Some(dsv_cty)));
    let dsv = try!(factory.view_texture_as_depth_stencil_trivial(&tex_dsv));
    Ok((rtv, dsv))
}*/

#[cfg(not(feature = "dx11"))]
pub fn init_existing<Cf, Df>(window: Rc<window::Window>) ->
                            (WrapperWindow, BackendDevice, backend::Factory,
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
pub fn init_existing(window: Rc<window::Window>) ->
                    (WrapperWindow, BackendDevice, backend::Factory,
                     gfx::handle::RenderTargetView<R, ColorFormat>, gfx::handle::DepthStencilView<R, DepthFormat>)
{
    let (mut win, device, mut factory, main_color) = gfx_window_dxgi::init_existing_raw(window, ColorFormat::get_format()).unwrap();
    let main_depth = factory.create_depth_stencil_view_only(win.size.0, win.size.1).unwrap();
    let mut device = backend::Deferred::from(device);
    (win, device, factory, gfx::memory::Typed::new(main_color), main_depth)
}
