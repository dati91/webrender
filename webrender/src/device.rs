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

use glutin;
use gfx;
use gfx::Factory;
use gfx::texture;
use gfx::traits::FactoryExt;
use gfx::format::{DepthStencil as DepthFormat, Rgba8 as ColorFormat};
use gfx_device_gl as device_gl;
use gfx_device_gl::{Resources as R, CommandBuffer as CB};
use gfx_window_glutin;
use gfx::CombinedError;
use gfx::format::R8_G8_B8_A8;
use gfx::format::Rgba8;
use gfx::memory::{Usage, SHADER_RESOURCE};
use gfx::format::ChannelType::Unorm;

gfx_defines! {
    vertex Vertex {
        pos: [f32; 2] = "aPosition",
        color: [f32; 3] = "aColor",
    }

    pipeline pipe {
        vbuf: gfx::VertexBuffer<Vertex> = (),
        out_color: gfx::RenderTarget<ColorFormat> = "oFragColor",
        out_depth: gfx::DepthTarget<DepthFormat> = gfx::preset::depth::LESS_EQUAL_WRITE,
    }

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
    main_surface: gfx::handle::Texture<R, R8_G8_B8_A8>,
    main_view: gfx::handle::ShaderResourceView<R, [f32; 4]>,
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
        let max_texture_size = 256;

        /*let ps_rectangle_pso = factory.create_pipeline_simple(
            include_bytes!(concat!(env!("OUT_DIR"), "/min_ps_rectangle.vs.glsl")),
            include_bytes!(concat!(env!("OUT_DIR"), "/min_ps_rectangle.fs.glsl")),
            primitive::new()
        ).unwrap();*/

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
            V2 {
                pos: [x0, y0],// color: [1.0, 0.0, 0.0]
                tex_coord: [0.0, 0.0],
            },
            V2 {
                pos: [x1, y0],// color: [0.0, 1.0, 0.0]
                tex_coord: [1.0, 0.0],
            },
            V2 {
                pos: [x0, y1],// color: [0.0, 0.0, 1.0]
                tex_coord: [0.0, 1.0],
            },
            V2 {
                pos: [x1, y1],// color: [1.0, 1.0, 1.0]
                tex_coord: [1.0, 1.0],
            },
        ];

        let (vertex_buffer, slice) = factory.create_vertex_buffer_with_slice(&quad_vertices, quad_indices);

        let sampler_info = gfx::texture::SamplerInfo::new(
            gfx::texture::FilterMethod::Scale,
            gfx::texture::WrapMode::Clamp
        );
        //let sampler = factory.create_sampler_linear();
        let sampler = factory.create_sampler(sampler_info);

        let tex: gfx::handle::Texture<_, gfx::format::R8_G8_B8_A8> =
            factory.create_texture::<gfx::format::R8_G8_B8_A8>(
                texture::Kind::D2(max_texture_size as u16, max_texture_size as u16, texture::AaMode::Single), 1, gfx::memory::SHADER_RESOURCE, Usage::Dynamic, Some(Unorm)).unwrap();

        let texture_view = factory.view_texture_as_shader_resource::<gfx::format::Rgba8>(
            &tex, (0,0), gfx::format::Swizzle::new()
        ).unwrap();

        let data = p2::Data {
            vbuf: vertex_buffer,
            color: (texture_view.clone(), sampler),
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
            main_surface: tex,
            main_view: texture_view,
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

    pub fn draw(&mut self) {
        println!("draw!");
        let tex = &self.main_surface;
        let (width, height, _, _) = self.main_surface.get_info().kind.get_dimensions();
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

        let texels = &self.texels[..];
        let data = gfx::memory::cast_slice(texels);
        self.encoder.update_texture::<_, Rgba8>(tex, None, img_info, data).unwrap();
        self.encoder.draw(&self.slice, &self.pso, &self.data);
        self.encoder.flush(&mut self.device);
    }
}
