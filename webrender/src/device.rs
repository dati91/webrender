/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use euclid::Matrix4D;
use fnv::FnvHasher;
//use gleam::gl;
use internal_types::{PackedVertex, RenderTargetMode, TextureSampler, DEFAULT_TEXTURE};
use internal_types::{BlurAttribute, ClearAttribute, ClipAttribute, VertexAttribute};
use internal_types::{DebugFontVertex, DebugColorVertex};
//use notify::{self, Watcher};
use super::shader_source;
use std::collections::HashMap;
use std::fs::File;
use std::hash::BuildHasherDefault;
use std::io::Read;
use std::iter::repeat;
use std::mem;
use std::path::PathBuf;
//use std::sync::mpsc::{channel, Sender};
//use std::thread;
use webrender_traits::{ColorF, ImageFormat};
use webrender_traits::{DeviceIntPoint, DeviceIntRect, DeviceIntSize, DeviceUintSize};

use std;
use glutin;
use gfx;
use gfx_core;
use gfx::Factory;
use gfx::texture;
use gfx::traits::FactoryExt;
use gfx::format::{DepthStencil as DepthFormat, Rgba8 as ColorFormat};
use gfx_device_gl as device_gl;
use gfx_device_gl::{Resources as R, CommandBuffer as CB};
use gfx_window_glutin;
use gfx::CombinedError;
use gfx::format::{R8_G8_B8_A8, Rgba8, R32_G32_B32_A32, Rgba32F};
use gfx::memory::{Usage, SHADER_RESOURCE};
use gfx::format::ChannelType::Unorm;
use gfx::format::TextureSurface;
use tiling::Frame;

gfx_defines! {
    vertex V2 {
        pos: [f32; 2] = "a_Pos",
        tex_coord: [f32; 2] = "a_TexCoord",
    }

    pipeline p2 {
        vbuf: gfx::VertexBuffer<V2> = (),
        color: gfx::TextureSampler<[f32; 4]> = "t_Color",
        out_color: gfx::RenderTarget<ColorFormat> = "Target0",
        out_depth: gfx::DepthTarget<DepthFormat> =
            gfx::preset::depth::LESS_EQUAL_WRITE,
    }

    vertex PrimitiveVertex {
        pos: [f32; 3] = "aPosition",
        glob_prim_id: i32 = "aGlobalPrimId",
        primitive_address: i32 = "aPrimitiveAddress",
        task_index: i32 = "aTaskIndex",
        clip_task_index: i32 = "aClipTaskIndex",
        layer_index: i32 = "aLayerIndex",
        element_index: i32 = "aElementIndex",
        user_data: [i32; 2] = "aUserData",
        z_index: i32 = "aZIndex",
    }

    pipeline primitive {
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<PrimitiveVertex> = (),
        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        color1: gfx::TextureSampler<[f32; 4]> = "sColor1",
        color2: gfx::TextureSampler<[f32; 4]> = "sColor2",
        mask: gfx::TextureSampler<[f32; 4]> = "sMask",
        cache: gfx::TextureSampler<f32> = "sCache",
        layers: gfx::TextureSampler<[f32; 4]> = "sLayers",
        render_tasks: gfx::TextureSampler<[f32; 4]> = "sRenderTasks",
        prim_geometry: gfx::TextureSampler<[f32; 4]> = "sPrimGeometry",
        data16: gfx::TextureSampler<[f32; 4]> = "sData16",
        data32: gfx::TextureSampler<[f32; 4]> = "sData32",
        data64: gfx::TextureSampler<[f32; 4]> = "sData64",
        data128: gfx::TextureSampler<[f32; 4]> = "sData128",
        resource_rects: gfx::TextureSampler<[f32; 4]> = "sResourceRects",
        out_color: gfx::RenderTarget<ColorFormat> = "oFragColor",
        out_depth: gfx::DepthTarget<DepthFormat> = gfx::preset::depth::LESS_EQUAL_WRITE,
    }

    // MIN PS RECT

    vertex min_vertex {
        pos: [f32; 3] = "aPosition",
        glob_prim_id: i32 = "aGlobalPrimId",
        primitive_address: i32 = "aPrimitiveAddress",
        task_index: i32 = "aTaskIndex",
        clip_task_index: i32 = "aClipTaskIndex",
        layer_index: i32 = "aLayerIndex",
        element_index: i32 = "aElementIndex",
        user_data: [i32; 2] = "aUserData",
        z_index: i32 = "aZIndex",
    }

    pipeline min_primitive {
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<PrimitiveVertex> = (),
        layers: gfx::TextureSampler<[f32; 4]> = "sLayers",
        render_tasks: gfx::TextureSampler<[f32; 4]> = "sRenderTasks",
        prim_geometry: gfx::TextureSampler<[f32; 4]> = "sPrimGeometry",
        data16: gfx::TextureSampler<[f32; 4]> = "sData16",
        out_color: gfx::RenderTarget<ColorFormat> = "oFragColor",
        out_depth: gfx::DepthTarget<DepthFormat> = gfx::preset::depth::LESS_EQUAL_WRITE,
    }
}

impl min_vertex {
    fn new(p: [f32; 2]) -> min_vertex {
        min_vertex {
            pos: [p[0], p[1], 0.0],
            glob_prim_id: 0,
            primitive_address: 0,
            task_index: 0,
            clip_task_index: 0,
            layer_index: 0,
            element_index: 0,
            user_data: [0, 0],
            z_index: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Texture<R, T> where R: gfx::Resources,
                               T: gfx::format::TextureFormat {
    // Pixel storage for texture.
    pub surface: gfx::handle::Texture<R, T::Surface>,
    // Sampler for texture.
    pub sampler: gfx::handle::Sampler<R>,
    // View used by shader.
    pub view: gfx::handle::ShaderResourceView<R, T::View>,
    // Filtering mode
    pub filter: TextureFilter,
    // ImageFormat
    pub format: ImageFormat,
    // Render Target mode
    pub mode: RenderTargetMode,
}

impl<R, T> Texture<R, T> where R: gfx::Resources, T: gfx::format::TextureFormat {

    pub fn empty<F>(factory: &mut F, size: [u32; 2]) -> Result<Texture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        Texture::create(factory, None, size, TextureFilter::Nearest)
    }

    pub fn create<F>(factory: &mut F,
                     data: Option<&[&[u8]]>,
                     size: [u32; 2],
                     filter: TextureFilter
    ) -> Result<Texture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        let (width, height) = (size[0] as u16, size[1] as u16);
        let tex_kind = gfx::texture::Kind::D2(width, height,
            gfx::texture::AaMode::Single);

        let filter_method = match filter {
            TextureFilter::Nearest => gfx::texture::FilterMethod::Scale,
            TextureFilter::Linear => gfx::texture::FilterMethod::Bilinear,
        };
        let sampler_info = gfx::texture::SamplerInfo::new(
            filter_method,
            gfx::texture::WrapMode::Clamp
        );

        let (surface, view, format) = {
            use gfx::{format, texture};
            use gfx::memory::{Usage, SHADER_RESOURCE};
            use gfx_core::memory::Typed;

            let surface = <T::Surface as format::SurfaceTyped>::get_surface_type();
            //let num_slices = tex_kind.get_num_slices().unwrap_or(1) as usize;
            //let num_faces = if tex_kind.is_cube() {6} else {1};
            let desc = texture::Info {
                kind: tex_kind,
                levels: 1,//(data.len() / (num_slices * num_faces)) as texture::Level,
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
                R8_G8_B8_A8 => ImageFormat::RGBA8,
                R32_G32_B32_A32 => ImageFormat::RGBAF32,
            };
            (tex, view, format)
        };

        let sampler = factory.create_sampler(sampler_info);

        Ok(Texture {
            surface: surface,
            sampler: sampler,
            view: view,
            filter: filter,
            format: format,
            mode: RenderTargetMode::None,
        })
    }

    /*pub fn update<C>(
        &mut self,
        encoder: &mut gfx::Encoder<R, C>,
        img: &[u8],
    ) -> Result<(), gfx::UpdateError<[u16; 3]>>
        where C: gfx::CommandBuffer<R>,
    {
        let (width, height) = self.get_size();
        let offset = [0, 0];
        let size = [width, height];
        let tex = &self.surface;
        let face = None;
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
        use gfx::format;
        let data = gfx::memory::cast_slice(img);

        encoder.update_texture::<_, T>(tex, face, img_info, data).map_err(Into::into)
    }*/

    #[inline(always)]
    pub fn get_size(&self) -> (u32, u32) {
        let (w, h, _, _) = self.surface.get_info().kind.get_dimensions();
        (w as u32, h as u32)
    }

    #[inline(always)]
    fn get_width(&self) -> u32 {
        let (w, _) = self.get_size();
        w
    }

    #[inline(always)]
    fn get_height(&self) -> u32 {
        let (_, h) = self.get_size();
        h
    }
}


#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TextureTarget {
    Default,
    Array,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TextureFilter {
    Nearest,
    Linear,
}

pub trait NamedTag {
    fn get_label(&self) -> &str;
}

#[derive(PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Copy, Clone)]
pub struct TextureId {
    name: u32,
    target: gfx::texture::Kind,
}

impl TextureId {
    pub fn new(name: u32) -> TextureId {
        TextureId {
            name: name,
            target: gfx::texture::Kind::D2(1,1,gfx::texture::AaMode::Single),
        }
    }

    pub fn invalid() -> TextureId {
        TextureId {
            name: 0,
            target: gfx::texture::Kind::D2(1,1,gfx::texture::AaMode::Single),
        }
    }

    pub fn is_valid(&self) -> bool { *self != TextureId::invalid() }
}

fn get_optional_shader_source(shader_name: &str, base_path: &Option<PathBuf>) -> Option<String> {
    if let Some(ref base) = *base_path {
        let shader_path = base.join(&format!("{}.glsl", shader_name));
        if shader_path.exists() {
            let mut source = String::new();
            File::open(&shader_path).unwrap().read_to_string(&mut source).unwrap();
            return Some(source);
        }
    }

    shader_source::SHADERS.get(shader_name).and_then(|s| Some((*s).to_owned()))
}

fn get_shader_source(shader_name: &str, base_path: &Option<PathBuf>) -> String {
    get_optional_shader_source(shader_name, base_path)
        .expect(&format!("Couldn't get required shader: {}", shader_name))
}

#[derive(Clone, Debug)]
pub enum ShaderError {
    Compilation(String, String), // name, error mssage
    Link(String), // error message
}

pub struct Device {
    device: device_gl::Device,
    factory: device_gl::Factory,
    encoder: gfx::Encoder<R,CB>,
    pso: gfx::PipelineState<R, p2::Meta>,
    data: p2::Data<R>,
    slice: gfx::Slice<R>,
    //tex: Texture<R, Rgba8>,
    //data16: Texture<R, Rgba8>,
    layer: Texture<R, Rgba8>,
    texels: Vec<u8>,
    max_texture_size: u32,
}

impl Device {
    pub fn new(window: &glutin::Window) -> Device {
        let (mut device, mut factory, main_color, main_depth) =
            gfx_window_glutin::init_existing::<ColorFormat, DepthFormat>(window);
        println!("Vendor: {:?}", device.get_info().platform_name.vendor);
        println!("Renderer: {:?}", device.get_info().platform_name.renderer);
        println!("Version: {:?}", device.get_info().version);
        println!("Shading Language: {:?}", device.get_info().shading_language);
        let mut encoder: gfx::Encoder<_,_> = factory.create_command_buffer().into();
        //let max_texture_size = factory.get_capabilities().max_texture_size as u32;
        let max_texture_size = 16;

        let ps_rectangle_pso = factory.create_pipeline_simple(
            include_bytes!(concat!(env!("OUT_DIR"), "/min2_ps_rectangle.vs.glsl")),
            include_bytes!(concat!(env!("OUT_DIR"), "/min2_ps_rectangle.fs.glsl")),
            min_primitive::new()
        ).unwrap();

        let pso = factory.create_pipeline_simple(
            include_bytes!("../res/v2.glsl"),
            include_bytes!("../res/f2.glsl"),
            p2::new()
        ).unwrap();

        let x0 = -1.0;
        let y0 = -1.0;
        let x1 = 1.0;
        let y1 = 1.0;

        let quad_indices: &[u16] = &[ 0, 1, 2, 2, 1, 3 ];
        let quad_vertices = [
            V2 { pos: [x0, y0], tex_coord: [0.0, 0.0] },
            V2 { pos: [x1, y0], tex_coord: [1.0, 0.0] },
            V2 { pos: [x0, y1], tex_coord: [0.0, 1.0] },
            V2 { pos: [x1, y1], tex_coord: [1.0, 1.0] },
        ];

        let min_quad_vertices = [
            min_vertex::new([x0, y0]),
            min_vertex::new([x1, y0]),
            min_vertex::new([x0, y1]),
            min_vertex::new([x1, y1]),
        ];

        let (vertex_buffer, slice) = factory.create_vertex_buffer_with_slice(&quad_vertices, quad_indices);

        //let data16_tex = Texture::empty(&mut factory, [16, 16]).unwrap();
        let layer_tex = Texture::empty(&mut factory, [16, 16]).unwrap();

        let data = p2::Data {
            vbuf: vertex_buffer,
            color: (layer_tex.clone().view, layer_tex.clone().sampler),
            out_color: main_color,
            out_depth: main_depth,
        };

        /*let data = pipe::Data {
            transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
            device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
            vbuf: vertex_buffer,

            color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
            color1: gfx::TextureSampler<[f32; 4]> = "sColor1",
            color2: gfx::TextureSampler<[f32; 4]> = "sColor2",
            mask: gfx::TextureSampler<[f32; 4]> = "sMask",
            cache: gfx::TextureSampler<f32> = "sCache",
            layers: gfx::TextureSampler<[f32; 4]> = "sLayers",
            render_tasks: gfx::TextureSampler<[f32; 4]> = "sRenderTasks",
            prim_geometry: gfx::TextureSampler<[f32; 4]> = "sPrimGeometry",
            data16: gfx::TextureSampler<[f32; 4]> = "sData16",
            data32: gfx::TextureSampler<[f32; 4]> = "sData32",
            data64: gfx::TextureSampler<[f32; 4]> = "sData64",
            data128: gfx::TextureSampler<[f32; 4]> = "sData128",
            resource_rects: gfx::TextureSampler<[f32; 4]> = "sResourceRects",


            out_color: main_color,
            out_depth: main_depth,
        };*/

        let mut texels = vec![];
        for j in 0..max_texture_size {
            for i in 0..max_texture_size {
                texels.append(&mut vec![0x20, 0xA0, 0xC0, 0x00]);
            }
        }

        Device {
            device: device,
            factory: factory,
            encoder: encoder,
            pso: pso,
            data: data,
            slice: slice,
            //tex: main_tex,
            //data16: data16_tex,
            layer: layer_tex,
            texels: texels,
            max_texture_size: max_texture_size,
        }
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    pub fn clear_target(&mut self,
                        color: Option<[f32; 4]>,
                        depth: Option<f32>) {
        if let Some(color) = color {
            println!("clear:{:?}", color);
            self.encoder.clear(&self.data.out_color, color);
        }

        if let Some(depth) = depth {
            self.encoder.clear_depth(&self.data.out_depth, depth);
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        println!("draw!");
        println!("gpu_data16.len {}", frame.gpu_data16.len());
        println!("gpu_data32.len {}", frame.gpu_data32.len());
        println!("gpu_data64.len {}", frame.gpu_data64.len());
        println!("gpu_data128.len {}", frame.gpu_data128.len());
        println!("gpu_geometry.len {}", frame.gpu_geometry.len());
        println!("gpu_resource_rects.len {}", frame.gpu_resource_rects.len());
        println!("layer_texture_data.len {}", frame.layer_texture_data.len());
        println!("render_task_data.len {}", frame.render_task_data.len());
        println!("gpu_gradient_data.len {}", frame.gpu_gradient_data.len());
        //let data = self.texels.clone();
        //self.update_texture(&data[..]);
        //Device::update_texture_u8(&mut self.encoder, &self.layer, unsafe { mem::transmute(frame.layer_texture_data.as_slice()) });

        self.encoder.draw(&self.slice, &self.pso, &self.data);
        self.encoder.flush(&mut self.device);
    }

    pub fn update_texture_f32(encoder: &mut gfx::Encoder<R,CB>, texture: &Texture<R, Rgba32F>, memory: &[u8]) {
        let tex = &texture.surface;
        let (width, height) = texture.get_size();
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

        let data = gfx::memory::cast_slice(memory);
        encoder.update_texture::<_, Rgba32F>(tex, None, img_info, data).unwrap();
    }

    fn update_texture_u8(encoder: &mut gfx::Encoder<R,CB>, texture: &Texture<R, Rgba8>, memory: &[u8]) {
        let tex = &texture.surface;
        let (width, height) = texture.get_size();
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

        let data = gfx::memory::cast_slice(memory);
        encoder.update_texture::<_, Rgba8>(tex, None, img_info, data).unwrap();
    }
}
