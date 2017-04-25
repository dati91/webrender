/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use euclid::Matrix4D;
use fnv::FnvHasher;
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
use std::rc::Rc;
//use std::sync::mpsc::{channel, Sender};
//use std::thread;
use webrender_traits::{ColorF, ImageFormat};
use webrender_traits::{DeviceIntPoint, DeviceIntRect, DeviceIntSize, DeviceUintSize};

use std;
use std::env;
use glutin;
use gfx;
use gfx::pso::PipelineData;
use gfx_core;
use gfx::Factory;
use gfx::texture;
use gfx::traits::FactoryExt;
use gfx::format::{DepthStencil as DepthFormat, Rgba32F as ColorFormat};
use gfx_device_gl as device_gl;
use gfx_device_gl::{Resources as R, CommandBuffer as CB};
use gfx_window_glutin;
use gfx::CombinedError;
use gfx::format::{R8_G8_B8_A8, Rgba8, R32_G32_B32_A32, Rgba32F};
use gfx::memory::{Usage, SHADER_RESOURCE};
use gfx::format::ChannelType::Unorm;
use gfx::format::TextureSurface;
use tiling::{Frame, PackedLayer, PrimitiveInstance};
use render_task::RenderTaskData;
use prim_store::{GpuBlock16, GpuBlock32, GpuBlock64, GpuBlock128, PrimitiveGeometry, TexelRect};

pub const VECS_PER_LAYER: u32 = 13;
pub const VECS_PER_RENDER_TASK: u32 = 3;
pub const VECS_PER_PRIM_GEOM: u32 = 2;
pub const MAX_INSTANCE_COUNT: usize = 512;
pub const VECS_PER_DATA_16: u32 = 1;
pub const VECS_PER_DATA_32: u32 = 2;
pub const VECS_PER_DATA_64: u32 = 4;
pub const VECS_PER_DATA_128: u32 = 8;
pub const VECS_PER_RESOURCE_RECTS: u32 = 1;
pub const FLOAT_SIZE: u32 = 4;
pub const TEXTURE_HEIGTH: u32 = 4;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ProgramId {
    CS_BLUR,
    CS_BOX_SHADOW,
    CS_CLIP_IMAGE,
    CS_CLIP_RECTANGLE,
    CS_TEXT_RUN,
    PS_ANGLE_GRADIENT,
    PS_ANGLE_GRADIENT_TRANSFORM,
    PS_BLEND,
    PS_BORDER,
    PS_BORDER_TRANSFORM,
    PS_BORDER_CORNER,
    PS_BORDER_CORNER_TRANSFORM,
    PS_BORDER_EDGE,
    PS_BORDER_EDGE_TRANSFORM,
    PS_BOX_SHADOW,
    PS_BOX_SHADOW_TRANSFORM,
    PS_CACHE_IMAGE,
    PS_CACHE_IMAGE_TRANSFORM,
    PS_CLEAR,
    PS_CLEAR_TRANSFORM,
    PS_COMPOSITE,
    PS_GRADIENT,
    PS_GRADIENT_TRANSFORM,
    PS_HARDWARE_COMPOSITE,
    PS_IMAGE,
    PS_IMAGE_TRANSFORM,
    PS_IMAGE_RECT,
    PS_IMAGE_RECT_TRANSFORM,
    PS_RADIAL_GRADIENT,
    PS_RADIAL_GRADIENT_TRANSFORM,
    PS_RECTANGLE,
    PS_RECTANGLE_TRANSFORM,
    PS_RECTANGLE_CLIP,
    PS_RECTANGLE_CLIP_TRANSFORM,
    PS_TEXT_RUN,
    PS_TEXT_RUN_TRANSFORM,
    PS_TEXT_RUN_SUBPIXEL,
    PS_TEXT_RUN_SUBPIXEL_TRANSFORM,
    PS_YUV_IMAGE,
    PS_YUV_IMAGE_TRANSFORM,
}

gfx_defines! {
    vertex Position {
        pos: [f32; 3] = "aPosition",
    }

    vertex Instances {
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
        vbuf: gfx::VertexBuffer<Position> = (),
        ibuf: gfx::InstanceBuffer<Instances> = (),

        // FIXME: Find the correct data type for these color samplers
        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        color1: gfx::TextureSampler<[f32; 4]> = "sColor1",
        color2: gfx::TextureSampler<[f32; 4]> = "sColor2",
        dither: gfx::TextureSampler<[f32; 4]> = "sDtiher",
        cache_a8: gfx::TextureSampler<[f32; 4]> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

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

impl Position {
    fn new(p: [f32; 2]) -> Position {
        Position {
            pos: [p[0], p[1], 0.0],
        }
    }
}

impl Instances {
    fn new() -> Instances {
        Instances {
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

    fn update(&mut self, instance: &PrimitiveInstance) {
        self.glob_prim_id = instance.global_prim_id;
        self.primitive_address = instance.prim_address.0;
        self.task_index = instance.task_index;
        self.clip_task_index = instance.clip_task_index;
        self.layer_index = instance.layer_index;
        self.element_index = instance.sub_index;
        self.user_data = instance.user_data;
        self.z_index = instance.z_sort_index;
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

enum Program {
    Primitive {
        pso: gfx::PipelineState<R, primitive::Meta>,
        data: primitive::Data<R>,
        slice: gfx::Slice<R>,
        upload: gfx::handle::Buffer<R, Instances>,
    },
}

impl Program {
    fn get_prim_pso(&self) -> Result<&gfx::PipelineState<R, primitive::Meta>, String> {
        match *self {
            Program::Primitive {ref pso, ..} => Ok(pso),
            _=> Err(String::from("The type is not primitive.")),
        }
    }
    fn get_prim_data_mut(&mut self) -> Result<&mut primitive::Data<R>, String> {
        match *self {
            Program::Primitive {ref mut data, ..} => Ok(data),
            _=> Err(String::from("The type is not primitive.")),
        }
    }
    fn get_prim_data(&self) -> Result<&primitive::Data<R>, String> {
        match *self {
            Program::Primitive {ref data, ..} => Ok(data),
            _=> Err(String::from("The type is not primitive.")),
        }
    }
    fn get_prim_upload(&self) -> Result<&gfx::handle::Buffer<R, Instances>, String> {
        match *self {
            Program::Primitive {ref upload, ..} => Ok(upload),
            _=> Err(String::from("The type is not primitive.")),
        }
    }
    fn get_slice(&self) -> &gfx::Slice<R> {
        match *self {
            Program::Primitive {ref slice, ..} => slice,
        }
    }
    fn get_slice_mut(&mut self) -> &mut gfx::Slice<R> {
        match *self {
            Program::Primitive {ref mut slice, ..} => slice,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct FrameId(usize);

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

#[derive(PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Copy, Clone)]
pub struct TextureId {
    name: u32,
    target: gfx::texture::Kind,
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
    programs: HashMap<ProgramId, Program>,
    color0: Texture<R, Rgba32F>,
    color1: Texture<R, Rgba32F>,
    color2: Texture<R, Rgba32F>,
    dither: Texture<R, Rgba32F>,
    cache_a8: Texture<R, Rgba32F>,
    cache_rgba8: Texture<R, Rgba32F>,
    layers: Texture<R, Rgba32F>,
    render_tasks: Texture<R, Rgba32F>,
    prim_geo: Texture<R, Rgba32F>,
    data16: Texture<R, Rgba32F>,
    data32: Texture<R, Rgba32F>,
    data64: Texture<R, Rgba32F>,
    data128: Texture<R, Rgba32F>,
    resource_rects: Texture<R, Rgba32F>,
    max_texture_size: u32,
    main_color: gfx_core::handle::RenderTargetView<R, ColorFormat>,
    main_depth: gfx_core::handle::DepthStencilView<R, DepthFormat>,
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
        let max_texture_size = factory.get_capabilities().max_texture_size as u32;

        let ps_rect_pso = factory.create_pipeline_simple(
            include_bytes!(concat!(env!("OUT_DIR"), "/ps_rectangle.vert")),
            include_bytes!(concat!(env!("OUT_DIR"), "/ps_rectangle.frag")),
            primitive::new()
        ).unwrap();

        let ps_rect_transform_pso = factory.create_pipeline_simple(
            include_bytes!(concat!(env!("OUT_DIR"), "/ps_rectangle.vert")),
            include_bytes!(concat!(env!("OUT_DIR"), "/ps_rectangle.frag")),
            primitive::new()
        ).unwrap();

        let ps_rect_clip_pso = factory.create_pipeline_simple(
            include_bytes!(concat!(env!("OUT_DIR"), "/ps_rectangle.vert")),
            include_bytes!(concat!(env!("OUT_DIR"), "/ps_rectangle.frag")),
            primitive::new()
        ).unwrap();

        let ps_rect_clip_transform_pso = factory.create_pipeline_simple(
            include_bytes!(concat!(env!("OUT_DIR"), "/ps_rectangle.vert")),
            include_bytes!(concat!(env!("OUT_DIR"), "/ps_rectangle.frag")),
            primitive::new()
        ).unwrap();

        let x0 = 0.0;
        let y0 = 0.0;
        let x1 = 1.0;
        let y1 = 1.0;

        let quad_indices: &[u16] = &[ 0, 1, 2, 2, 1, 3 ];
        let quad_vertices = [
            Position::new([x0, y0]),
            Position::new([x1, y0]),
            Position::new([x0, y1]),
            Position::new([x1, y1]),
        ];

        let instance_count = MAX_INSTANCE_COUNT;
        let upload = factory.create_upload_buffer(instance_count as usize).unwrap();
        {
            let mut writer = factory.write_mapping(&upload).unwrap();

            writer[0] = min_instance::new();
            for i in 0..instance_count {
                writer[i] = Instances::new();
            }
        }

        let instances = factory
            .create_buffer(instance_count as usize,
                           gfx::buffer::Role::Vertex,
                           gfx::memory::Usage::Data,
                           gfx::TRANSFER_DST).unwrap();

        let (vertex_buffer, mut slice) = factory.create_vertex_buffer_with_slice(&quad_vertices, quad_indices);
        slice.instances = Some((instance_count as u32, 0));
 
        // FIXME: find the correct limits for these variables
        let color0 = Texture::empty(&mut factory, [1024, 1]).unwrap();
        let color1 = Texture::empty(&mut factory, [1024, 1]).unwrap();
        let color2 = Texture::empty(&mut factory, [1024, 1]).unwrap();
        let dither = Texture::empty(&mut factory, [1024, 1]).unwrap();
        let cache_a8 = Texture::empty(&mut factory, [1024, 1]).unwrap();
        let cache_rgba8 = Texture::empty(&mut factory, [1024, 1]).unwrap();

        let layers_tex = Texture::empty(&mut factory, [1024 / VECS_PER_LAYER as u32, 64]).unwrap();
        let render_tasks_tex = Texture::empty(&mut factory, [1024 / VECS_PER_RENDER_TASK as u32, TEXTURE_HEIGTH]).unwrap();
        let prim_geo_tex = Texture::empty(&mut factory, [1024 / VECS_PER_PRIM_GEOM as u32, TEXTURE_HEIGTH]).unwrap();
        let data16_tex = Texture::empty(&mut factory, [1024 / VECS_PER_DATA_16 as u32, TEXTURE_HEIGTH]).unwrap();
        let data32_tex = Texture::empty(&mut factory, [1024 / VECS_PER_DATA_32 as u32, TEXTURE_HEIGTH]).unwrap();
        let data64_tex = Texture::empty(&mut factory, [1024 / VECS_PER_DATA_64 as u32, TEXTURE_HEIGTH]).unwrap();
        let data128_tex = Texture::empty(&mut factory, [1024 / VECS_PER_DATA_128 as u32, TEXTURE_HEIGTH]).unwrap();
        let resource_rects = Texture::empty(&mut factory, [1024 / VECS_PER_RESOURCE_RECTS as u32, TEXTURE_HEIGTH]).unwrap();

        let data = primitive::Data {
            transform: [[0f32;4];4],
            device_pixel_ratio: 1f32,
            vbuf: vertex_buffer,
            ibuf: instances,
            color0: (color0.clone().view, color0.clone().sampler),
            color1: (color1.clone().view, color1.clone().sampler),
            color2: (color2.clone().view, color2.clone().sampler),
            dither: (dither.clone().view, dither.clone().sampler),
            cache_a8: (cache_a8.clone().view, cache_a8.clone().sampler),
            cache_rgba8: (cache_rgba8.clone().view, cache_rgba8.clone().sampler),
            layers: (layers_tex.clone().view, layers_tex.clone().sampler),
            render_tasks: (render_tasks_tex.clone().view, render_tasks_tex.clone().sampler),
            prim_geometry: (prim_geo_tex.clone().view, prim_geo_tex.clone().sampler),
            data16: (data16_tex.clone().view, data16_tex.clone().sampler),
            data32: (data32_tex.clone().view, data32_tex.clone().sampler),
            data64: (data64_tex.clone().view, data64_tex.clone().sampler),
            data128: (data128_tex.clone().view, data128_tex.clone().sampler),
            resource_rects: (resource_rects.clone().view, resource_rects.clone().sampler),
            out_color: main_color.clone(),
            out_depth: main_depth.clone(),
        };

        let ps_rectangle = Program::Primitive {
            pso: ps_rect_pso,
            data: data.clone(),
            slice: slice.clone(),
            upload: upload.clone(),
        };

        let ps_rectangle_transform = Program::Primitive {
            pso: ps_rect_transform_pso,
            data: data.clone(),
            slice: slice.clone(),
            upload: upload.clone(),
        };

        let ps_rectangle_clip = Program::Primitive {
            pso: ps_rect_clip_pso,
            data: data.clone(),
            slice: slice.clone(),
            upload: upload.clone(),
        };

        let ps_rectangle_clip_transform = Program::Primitive {
            pso: ps_rect_clip_transform_pso,
            data: data,
            slice: slice,
            upload: upload,
        };

        let mut programs = HashMap::new();
        programs.insert(ProgramId::PS_RECTANGLE, ps_rectangle);
        programs.insert(ProgramId::PS_RECTANGLE_TRANSFORM, ps_rectangle_transform);
        programs.insert(ProgramId::PS_RECTANGLE_CLIP, ps_rectangle_clip);
        programs.insert(ProgramId::PS_RECTANGLE_CLIP_TRANSFORM, ps_rectangle_clip_transform);

        Device {
            device: device,
            factory: factory,
            encoder: encoder,
            programs: programs,
            color0: color0,
            color1: color1,
            color2: color2,
            dither: dither,
            cache_a8: cache_a8,
            cache_rgba8: cache_rgba8,
            layers: layers_tex,
            render_tasks: render_tasks_tex,
            prim_geo: prim_geo_tex,
            data16: data16_tex,
            data32: data32_tex,
            data64: data64_tex,
            data128: data128_tex,
            resource_rects: resource_rects,
            max_texture_size: max_texture_size,
            main_color: main_color,
            main_depth: main_depth,
        }
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    pub fn clear_target(&mut self, color: Option<[f32; 4]>, depth: Option<f32>) {
        if let Some(color) = color {
            println!("clear:{:?}", color);
            self.encoder.clear(&self.main_color,
                               [color[0].powf(2.2),
                                color[1].powf(2.2),
                                color[2].powf(2.2),
                                color[3].powf(2.2)]);
        }

        if let Some(depth) = depth {
            self.encoder.clear_depth(&self.main_depth, depth);
        }
    }

    pub fn update(&mut self, frame: &mut Frame) {
        println!("update!");
        println!("gpu_data16.len {}", frame.gpu_data16.len());
        println!("gpu_data32.len {}", frame.gpu_data32.len());
        println!("gpu_data64.len {}", frame.gpu_data64.len());
        println!("gpu_data128.len {}", frame.gpu_data128.len());
        println!("gpu_geometry.len {}", frame.gpu_geometry.len());
        println!("gpu_resource_rects.len {}", frame.gpu_resource_rects.len());
        println!("layer_texture_data.len {}", frame.layer_texture_data.len());
        println!("render_task_data.len {}", frame.render_task_data.len());
        println!("gpu_gradient_data.len {}", frame.gpu_gradient_data.len());
        println!("device_pixel_ratio: {}", frame.device_pixel_ratio);
        Device::update_texture_f32(&mut self.encoder, &self.layers, Device::convert_layer(frame.layer_texture_data.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.render_tasks, Device::convert_render_task(frame.render_task_data.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.prim_geo, Device::convert_prim_geo(frame.gpu_geometry.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.data16, Device::convert_data16(frame.gpu_data16.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.data32, Device::convert_data32(frame.gpu_data32.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.data64, Device::convert_data64(frame.gpu_data64.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.data128, Device::convert_data128(frame.gpu_data128.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.resource_rects, Device::convert_resource_rects(frame.gpu_resource_rects.clone()).as_slice());
    }

    pub fn flush(&mut self) {
        println!("flush");
        self.encoder.flush(&mut self.device);
    }

    pub fn draw(&mut self, program_id: &ProgramId, proj: &Matrix4D<f32>, instances: &[PrimitiveInstance]) {
        /*println!("draw!");
        println!("proj: {:?}", proj);
        println!("data: {:?}", instances);*/
        if let Some(program) = self.programs.get_mut(program_id) {
            match * program_id {
                ProgramId::PS_RECTANGLE | ProgramId::PS_RECTANGLE_TRANSFORM | ProgramId::PS_RECTANGLE_CLIP | ProgramId::PS_RECTANGLE_CLIP_TRANSFORM => {
                    program.get_prim_data_mut().unwrap().transform = proj.to_row_arrays();
                    {
                        let mut writer = self.factory.write_mapping(program.get_prim_upload().unwrap()).unwrap();
                        //println!("writer: {} instances: {}", writer.len(), instances.len());
                        for (i, inst) in instances.iter().enumerate() {
                            //println!("instance[{}]: {:?}", i, inst);
                            writer[i].update(inst);
                            //println!("instance[{}]: {:?}", i, writer[i]);
                        }
                    }
                    {
                        //writer[0].update(&instances[0]);
                        program.get_slice_mut().instances = Some((instances.len() as u32, 0));
                    }
                    //println!("upload {:?}", &self.upload);
                    //println!("copy");
                    self.encoder.copy_buffer(program.get_prim_upload().unwrap(), &program.get_prim_data().unwrap().ibuf,
                                        0, 0, program.get_prim_upload().unwrap().len()).unwrap();
                    /*println!("vbuf {:?}", self.data.vbuf.get_info());
                    println!("ibuf {:?}", self.data.ibuf);
                    println!("layers {:?}", self.layers);
                    println!("render_tasks {:?}", self.render_tasks);
                    println!("prim_geo {:?}", self.prim_geo);
                    println!("data16 {:?}", self.data16);*/
                    self.encoder.draw(program.get_slice(), program.get_prim_pso().unwrap(), program.get_prim_data().unwrap());
                },
                _ => println!("Shader not yet implemented {:?}",  program_id),
            }
        } else {
            println!("Shader not yet implemented {:?}",  program_id);
        }
    }

    pub fn update_texture_u8(encoder: &mut gfx::Encoder<R,CB>, texture: &Texture<R, Rgba8>, memory: &[u8]) {
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

    pub fn update_texture_f32(encoder: &mut gfx::Encoder<R,CB>, texture: &Texture<R, Rgba32F>, memory: &[f32]) {
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

    fn convert_data16(data16: Vec<GpuBlock16>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for d in data16 {
            data.append(&mut d.data.to_vec());
        }
        let max_size = ((1024 / VECS_PER_DATA_16) * FLOAT_SIZE * TEXTURE_HEIGTH) as usize;
        println!("convert_data16 len {:?} max_size: {}", data.len(), max_size);
        if max_size > data.len() {
            let mut zeros = vec![0f32; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }

    fn convert_data32(data32: Vec<GpuBlock32>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for d in data32 {
            data.append(&mut d.data.to_vec());
        }
        let max_size = ((1024 / VECS_PER_DATA_32) * FLOAT_SIZE * TEXTURE_HEIGTH) as usize;
        println!("convert_data32 len {:?} max_size: {}", data.len(), max_size);
        if max_size > data.len() {
            let mut zeros = vec![0f32; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }

    fn convert_data64(data64: Vec<GpuBlock64>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for d in data64 {
            data.append(&mut d.data.to_vec());
        }
        let max_size = ((1024 / VECS_PER_DATA_64) * FLOAT_SIZE * TEXTURE_HEIGTH) as usize;
        println!("convert_data64 len {:?} max_size: {}", data.len(), max_size);
        if max_size > data.len() {
            let mut zeros = vec![0f32; (max_size - data.len())];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }

    fn convert_data128(data128: Vec<GpuBlock128>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for d in data128 {
            data.append(&mut d.data.to_vec());
        }
        let max_size = ((1024 / VECS_PER_DATA_128) * FLOAT_SIZE * TEXTURE_HEIGTH) as usize;
        println!("convert_data128 len {:?} max_size: {}", data.len(), max_size);
        if max_size > data.len() {
            let mut zeros = vec![0f32; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }

    fn convert_layer(layers: Vec<PackedLayer>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for l in layers {
            //println!("{:?}", l);
            data.append(&mut l.transform.to_row_major_array().to_vec());
            data.append(&mut l.inv_transform.to_row_major_array().to_vec());
            data.append(&mut l.local_clip_rect.origin.to_array().to_vec());
            data.append(&mut l.local_clip_rect.size.to_array().to_vec());
            data.append(&mut l.screen_vertices[0].to_array().to_vec());
            data.append(&mut l.screen_vertices[1].to_array().to_vec());
            data.append(&mut l.screen_vertices[2].to_array().to_vec());
            data.append(&mut l.screen_vertices[3].to_array().to_vec());
        }
        let max_size = ((1024 / VECS_PER_LAYER) * FLOAT_SIZE * 64) as usize;
        println!("convert_layer len {:?} max_size: {}", data.len(), max_size);
        if max_size > data.len() {
            let mut zeros = vec![0f32; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }

    fn convert_render_task(render_tasks: Vec<RenderTaskData>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for rt in render_tasks {
            data.append(&mut rt.data.to_vec());
        }
        let max_size = ((1024 / VECS_PER_RENDER_TASK) * FLOAT_SIZE * TEXTURE_HEIGTH) as usize;
        println!("convert_render_task len {:?} max_size: {}", data.len(), max_size);
        if max_size > data.len() {
            let mut zeros = vec![0f32; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }

    fn convert_prim_geo(prim_geo: Vec<PrimitiveGeometry>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for pg in prim_geo {
            data.append(&mut pg.local_rect.origin.to_array().to_vec());
            data.append(&mut pg.local_rect.size.to_array().to_vec());
            data.append(&mut pg.local_clip_rect.origin.to_array().to_vec());
            data.append(&mut pg.local_clip_rect.size.to_array().to_vec());
        }
        let max_size = ((1024 / VECS_PER_PRIM_GEOM) * FLOAT_SIZE * TEXTURE_HEIGTH) as usize;
        println!("convert_prim_geo len {:?} max_size: {}", data.len(), max_size);
        if max_size > data.len() {
            let mut zeros = vec![0f32; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }

    fn convert_resource_rects(resource_rects: Vec<TexelRect>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for r in resource_rects {
            data.append(&mut r.to_vec());
        }
        let max_size = ((1024 / VECS_PER_RESOURCE_RECTS) * FLOAT_SIZE * TEXTURE_HEIGTH) as usize;
        println!("convert_resource_rects len {:?} max_size: {}", data.len(), max_size);
        if max_size > data.len() {
            let mut zeros = vec![0f32; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }
}
