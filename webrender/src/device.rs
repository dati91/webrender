/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use euclid::Matrix4D;
use internal_types::{BatchTextures, RenderTargetMode, TextureSampler};
use std::collections::HashMap;
use std::mem;
use webrender_traits::ImageFormat;

use rand::Rng;
use std;
use glutin;
use gfx;
use gfx::state::{Blend, BlendChannel, BlendValue, Equation, Factor};
use gfx_core;
use gfx_core::memory::Typed;
use gfx::Factory;
use gfx::traits::FactoryExt;
use gfx::format::{DepthStencil as DepthFormat, Rgba8 as ColorFormat};
use gfx_device_gl as device_gl;
use gfx_device_gl::{Resources as R, CommandBuffer as CB};
use gfx::CombinedError;
use gfx::format::{Format, Formatted, R8, Rgba32F, Rgba8,Srgba8, SurfaceTyped, TextureChannel, TextureSurface, Unorm};
use tiling::PrimitiveInstance;
use renderer::{BlendMode, DITHER_ID, DUMMY_A8_ID, DUMMY_RGBA8_ID, MAX_VERTEX_TEXTURE_WIDTH};

pub type A8 = (R8, Unorm);
pub const VECS_PER_DATA_16: usize = 1;
pub const VECS_PER_DATA_32: usize = 2;
pub const VECS_PER_DATA_64: usize = 4;
pub const VECS_PER_DATA_128: usize = 8;
pub const VECS_PER_GRADIENT_DATA: usize = 4;
pub const VECS_PER_LAYER: usize = 13;
pub const VECS_PER_PRIM_GEOM: usize = 2;
pub const VECS_PER_RENDER_TASK: usize = 3;
pub const VECS_PER_RESOURCE_RECTS: usize = 1;
pub const VECS_PER_SPLIT_GEOM: usize = 3;
pub const TEXTURE_HEIGTH: usize = 8;
pub const DEVICE_PIXEL_RATIO: f32 = 1.0;
pub const MAX_INSTANCE_COUNT: usize = 2000;

pub const A_STRIDE: usize = 1;
pub const RG_STRIDE: usize = 2;
pub const RGB_STRIDE: usize = 3;
pub const RGBA_STRIDE: usize = 4;
pub const FIRST_UNRESERVED_ID: u32 = DITHER_ID + 1;

pub const ALPHA: Blend = Blend {
    color: BlendChannel {
        equation: Equation::Add,
        source: Factor::ZeroPlus(BlendValue::SourceAlpha),
        destination: Factor::OneMinus(BlendValue::SourceAlpha),
    },
    alpha: BlendChannel {
        equation: Equation::Add,
        source: Factor::One,
        destination: Factor::OneMinus(BlendValue::SourceAlpha),
    },
};

pub const PREM_ALPHA: Blend = Blend {
    color: BlendChannel {
        equation: Equation::Add,
        source: Factor::One,
        destination: Factor::OneMinus(BlendValue::SourceAlpha),
    },
    alpha: BlendChannel {
        equation: Equation::Add,
        source: Factor::One,
        destination: Factor::OneMinus(BlendValue::SourceAlpha),
    },
};

pub const SUBPIXEL: Blend = Blend {
    color: BlendChannel {
        equation: Equation::Add,
        source: Factor::ZeroPlus(BlendValue::ConstColor),
        destination: Factor::OneMinus(BlendValue::SourceColor),
    },
    alpha: BlendChannel {
        equation: Equation::Add,
        source: Factor::ZeroPlus(BlendValue::ConstColor),
        destination: Factor::OneMinus(BlendValue::SourceColor),
    },
};

type PSPrimitive = gfx::PipelineState<R, primitive::Meta>;

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

        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        color1: gfx::TextureSampler<[f32; 4]> = "sColor1",
        color2: gfx::TextureSampler<[f32; 4]> = "sColor2",
        dither: gfx::TextureSampler<f32> = "sDither",
        cache_a8: gfx::TextureSampler<f32> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

        data16: gfx::TextureSampler<[f32; 4]> = "sData16",
        data32: gfx::TextureSampler<[f32; 4]> = "sData32",
        data64: gfx::TextureSampler<[f32; 4]> = "sData64",
        data128: gfx::TextureSampler<[f32; 4]> = "sData128",
        gradients : gfx::TextureSampler<[f32; 4]> = "sGradients",
        layers: gfx::TextureSampler<[f32; 4]> = "sLayers",
        prim_geometry: gfx::TextureSampler<[f32; 4]> = "sPrimGeometry",
        render_tasks: gfx::TextureSampler<[f32; 4]> = "sRenderTasks",
        resource_rects: gfx::TextureSampler<[f32; 4]> = "sResourceRects",
        split_geometry: gfx::TextureSampler<[f32; 4]> = "sSplitGeometry",

        out_color: gfx::RawRenderTarget = ("oFragColor",
                                           Format(gfx::format::SurfaceType::R8_G8_B8_A8,
                                                  gfx::format::ChannelType::Unorm),
                                           gfx::state::MASK_ALL,
                                           None),
        out_depth: gfx::DepthTarget<DepthFormat> = gfx::preset::depth::LESS_EQUAL_WRITE,
        blend_value: gfx::BlendRef = (),
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

    pub fn empty<F>(factory: &mut F, size: [usize; 2]) -> Result<Texture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        Texture::create(factory, None, size, TextureFilter::Nearest)
    }

    pub fn create<F>(factory: &mut F,
                     data: Option<&[&[u8]]>,
                     size: [usize; 2],
                     filter: TextureFilter
    ) -> Result<Texture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        let (width, height) = (size[0] as u16, size[1] as u16);
        let tex_kind = gfx::texture::Kind::D2(width, height, gfx::texture::AaMode::Single);
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
                gfx_core::format::SurfaceType::R8 => ImageFormat::A8,
                gfx_core::format::SurfaceType::R8_G8_B8_A8 => ImageFormat::RGBA8,
                gfx_core::format::SurfaceType::R32_G32_B32_A32 => ImageFormat::RGBAF32,
                _ => unimplemented!(),
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
    pub fn get_size(&self) -> (usize, usize) {
        let (w, h, _, _) = self.surface.get_info().kind.get_dimensions();
        (w as usize, h as usize)
    }
}

pub struct Program {
    pub data: primitive::Data<R>,
    pub pso: PSPrimitive,
    pub pso_alpha: PSPrimitive,
    pub pso_prem_alpha: PSPrimitive,
    pub pso_subpixel: PSPrimitive,
    pub slice: gfx::Slice<R>,
    pub upload: gfx::handle::Buffer<R, Instances>,
}

impl Program {
    fn new(data: primitive::Data<R>, pso: (PSPrimitive, PSPrimitive, PSPrimitive, PSPrimitive), slice: gfx::Slice<R>, upload: gfx::handle::Buffer<R, Instances>) -> Program {
        Program {
            data: data,
            pso: pso.0,
            pso_alpha: pso.1,
            pso_prem_alpha: pso.2,
            pso_subpixel: pso.3,
            slice: slice,
            upload: upload,
        }
    }

    fn get_pso(&self, blend: &BlendMode) -> &PSPrimitive {
        match *blend {
            BlendMode::None => &self.pso,
            BlendMode::Alpha => &self.pso_alpha,
            BlendMode::PremultipliedAlpha => &self.pso_prem_alpha,
            BlendMode::Subpixel(..) => &self.pso_subpixel,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct FrameId(usize);

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TextureTarget {
    Default,
    _Array,
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

    pub fn _is_valid(&self) -> bool { !(*self == TextureId::invalid() || *self == TextureId::invalid_a8()) }
}

#[derive(PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Copy, Clone)]
pub struct TextureId {
    name: u32,
}

#[derive(Debug)]
pub struct TextureData {
    id: TextureId,
    pub data: Vec<u8>,
    stride: usize,
    pitch: usize,
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
    textures: HashMap<TextureId, TextureData>,
    color0: Texture<R, Rgba8>,
    color1: Texture<R, Rgba8>,
    color2: Texture<R, Rgba8>,
    dither: Texture<R, A8>,
    cache_a8: Texture<R, A8>,
    cache_rgba8: Texture<R, Rgba8>,
    data16: Texture<R, Rgba32F>,
    data32: Texture<R, Rgba32F>,
    data64: Texture<R, Rgba32F>,
    data128: Texture<R, Rgba32F>,
    gradient_data: Texture<R, Rgba8>,
    layers: Texture<R, Rgba32F>,
    prim_geo: Texture<R, Rgba32F>,
    render_tasks: Texture<R, Rgba32F>,
    resource_rects: Texture<R, Rgba32F>,
    split_geo: Texture<R, Rgba32F>,
    max_texture_size: u32,
    main_color: gfx_core::handle::RenderTargetView<R, ColorFormat>,
    main_depth: gfx_core::handle::DepthStencilView<R, DepthFormat>,
    vertex_buffer: gfx::handle::Buffer<R, Position>,
    slice: gfx::Slice<R>,
}

impl Device {
    pub fn new(window: &glutin::Window) -> Device {
        let (device, mut factory, main_color, main_depth) = init_existing::<ColorFormat, DepthFormat>(window);
        /*println!("Vendor: {:?}", device.get_info().platform_name.vendor);
        println!("Renderer: {:?}", device.get_info().platform_name.renderer);
        println!("Version: {:?}", device.get_info().version);
        println!("Shading Language: {:?}", device.get_info().shading_language);*/
        let encoder: gfx::Encoder<_,_> = factory.create_command_buffer().into();
        let max_texture_size = factory.get_capabilities().max_texture_size as u32;

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

        let (vertex_buffer, mut slice) = factory.create_vertex_buffer_with_slice(&quad_vertices, quad_indices);
        slice.instances = Some((MAX_INSTANCE_COUNT as u32, 0));

        let (w, h, _, _) = main_color.get_dimensions();
        let texture_size = [std::cmp::max(MAX_VERTEX_TEXTURE_WIDTH, h as usize), std::cmp::max(MAX_VERTEX_TEXTURE_WIDTH, w as usize)];
        let color0 = Texture::empty(&mut factory, texture_size).unwrap();
        let color1 = Texture::empty(&mut factory, texture_size).unwrap();
        let color2 = Texture::empty(&mut factory, texture_size).unwrap();
        let dither = Texture::empty(&mut factory, [8, 8]).unwrap();
        let cache_a8 = Texture::empty(&mut factory, texture_size).unwrap();
        let cache_rgba8 = Texture::empty(&mut factory, texture_size).unwrap();

        // TODO define some maximum boundaries for texture height
        let data16_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_DATA_16, TEXTURE_HEIGTH * 4]).unwrap();
        let data32_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_DATA_32, TEXTURE_HEIGTH]).unwrap();
        let data64_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_DATA_64, TEXTURE_HEIGTH]).unwrap();
        let data128_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_DATA_128, TEXTURE_HEIGTH * 4]).unwrap();
        let gradient_data = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_GRADIENT_DATA , TEXTURE_HEIGTH * 10]).unwrap();
        let layers_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_LAYER, 64]).unwrap();
        let prim_geo_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_PRIM_GEOM, TEXTURE_HEIGTH]).unwrap();
        let render_tasks_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_RENDER_TASK, TEXTURE_HEIGTH]).unwrap();
        let resource_rects = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_RESOURCE_RECTS, TEXTURE_HEIGTH * 2]).unwrap();
        let split_geo_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH / VECS_PER_SPLIT_GEOM, TEXTURE_HEIGTH * 2]).unwrap();

        let mut textures = HashMap::new();
        let (w, h) = color0.get_size();
        let invalid_id = TextureId::invalid();
        textures.insert(invalid_id, TextureData { id: invalid_id, data: vec![0u8; w * h * RGBA_STRIDE], stride: RGBA_STRIDE, pitch: w * RGBA_STRIDE });
        let invalid_a8_id = TextureId::invalid_a8();
        textures.insert(invalid_a8_id, TextureData { id: invalid_a8_id, data: vec![0u8; w * h * A_STRIDE], stride: A_STRIDE, pitch: w * A_STRIDE });
        let dummy_rgba8_id = TextureId { name: DUMMY_RGBA8_ID };
        textures.insert(dummy_rgba8_id, TextureData { id: dummy_rgba8_id, data: vec![0u8; w * h * RGBA_STRIDE], stride: RGBA_STRIDE, pitch: w * RGBA_STRIDE });
        let dummy_a8_id = TextureId { name: DUMMY_A8_ID };
        textures.insert(dummy_a8_id, TextureData { id: dummy_a8_id, data: vec![0u8; w * h * A_STRIDE], stride: A_STRIDE, pitch: w * A_STRIDE });
        let dither_id = TextureId { name: DITHER_ID };
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
        textures.insert(dither_id, TextureData { id: dither_id, data: dither_matrix, stride: A_STRIDE, pitch: 8 * RGBA_STRIDE });

        Device {
            device: device,
            factory: factory,
            encoder: encoder,
            textures: textures,
            color0: color0,
            color1: color1,
            color2: color2,
            dither: dither,
            cache_a8: cache_a8,
            cache_rgba8: cache_rgba8,
            data16: data16_tex,
            data32: data32_tex,
            data64: data64_tex,
            data128: data128_tex,
            gradient_data: gradient_data,
            layers: layers_tex,
            prim_geo: prim_geo_tex,
            render_tasks: render_tasks_tex,
            resource_rects: resource_rects,
            split_geo: split_geo_tex,
            max_texture_size: max_texture_size,
            main_color: main_color,
            main_depth: main_depth,
            vertex_buffer: vertex_buffer,
            slice: slice,
        }
    }

    pub fn read_pixels(&mut self, output: &mut [u8]) {
        self.encoder.flush(&mut self.device);
        let tex = self.main_color.raw().get_texture();
        let buf = self.factory.create_buffer::<u8>(output.len(),
                                                   gfx::buffer::Role::Vertex,
                                                   gfx::memory::Usage::Download,
                                                   gfx::TRANSFER_DST).unwrap();
        self.encoder.copy_texture_to_buffer_raw(tex, None, tex.get_info().to_raw_image_info(gfx::format::ChannelType::Unorm, 0), buf.raw(), 0).unwrap();
        self.encoder.flush(&mut self.device);
        {
            let reader = self.factory.read_mapping(&buf).unwrap();
            for i in 0..output.len() {
                output[i] = reader[i];
            }
        }
    }

    fn create_psos(&mut self, vert_src: &[u8],frag_src: &[u8]) -> (PSPrimitive, PSPrimitive, PSPrimitive, PSPrimitive) {
        let pso = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::new()
        ).unwrap();

        let pso_alpha = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8,gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(ALPHA)),
                .. primitive::new()
            }
        ).unwrap();

        let pso_prem_alpha = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8,gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(PREM_ALPHA)),
                .. primitive::new()
            }
        ).unwrap();

        let pso_subpixel = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8,gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(SUBPIXEL)),
                .. primitive::new()
            }
        ).unwrap();

        (pso, pso_alpha, pso_prem_alpha, pso_subpixel)
    }

    pub fn create_program(&mut self, vert_src: &[u8], frag_src: &[u8]) -> Program {
        let upload = self.factory.create_upload_buffer(MAX_INSTANCE_COUNT).unwrap();
        {
            let mut writer = self.factory.write_mapping(&upload).unwrap();
            for i in 0..MAX_INSTANCE_COUNT {
                writer[i] = Instances::new();
            }
        }

        let instances = self.factory.create_buffer(MAX_INSTANCE_COUNT,
                                                   gfx::buffer::Role::Vertex,
                                                   gfx::memory::Usage::Data,
                                                   gfx::TRANSFER_DST).unwrap();

        let data = primitive::Data {
            transform: [[0f32; 4]; 4],
            device_pixel_ratio: DEVICE_PIXEL_RATIO,
            vbuf: self.vertex_buffer.clone(),
            ibuf: instances,
            color0: (self.color0.clone().view, self.color0.clone().sampler),
            color1: (self.color1.clone().view, self.color1.clone().sampler),
            color2: (self.color2.clone().view, self.color2.clone().sampler),
            dither: (self.dither.clone().view, self.dither.clone().sampler),
            cache_a8: (self.cache_a8.clone().view, self.cache_a8.clone().sampler),
            cache_rgba8: (self.cache_rgba8.clone().view, self.cache_rgba8.clone().sampler),
            data16: (self.data16.clone().view, self.data16.clone().sampler),
            data32: (self.data32.clone().view, self.data32.clone().sampler),
            data64: (self.data64.clone().view, self.data64.clone().sampler),
            data128: (self.data128.clone().view, self.data128.clone().sampler),
            gradients: (self.gradient_data.clone().view, self.gradient_data.clone().sampler),
            layers: (self.layers.clone().view, self.layers.clone().sampler),
            prim_geometry: (self.prim_geo.clone().view, self.prim_geo.clone().sampler),
            render_tasks: (self.render_tasks.clone().view, self.render_tasks.clone().sampler),
            resource_rects: (self.resource_rects.clone().view, self.resource_rects.clone().sampler),
            split_geometry: (self.split_geo.clone().view, self.split_geo.clone().sampler),
            out_color: self.main_color.raw().clone(),
            out_depth: self.main_depth.clone(),
            blend_value: [0.0, 0.0, 0.0, 0.0]
        };
        let psos = self.create_psos(vert_src, frag_src);
        Program::new(data, psos, self.slice.clone(), upload)
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    fn generate_texture_id(&mut self) -> TextureId {
        use rand::OsRng;

        let mut rng = OsRng::new().unwrap();
        let mut texture_id = TextureId::invalid();
        loop {
            texture_id.name = rng.gen_range(FIRST_UNRESERVED_ID, u32::max_value());
            if !self.textures.contains_key(&texture_id) {
                break;
            }
        }
        texture_id
    }

    pub fn _create_texture_ids(&mut self,
                              count: i32,
                              _target: TextureTarget,
                              format: ImageFormat) -> Vec<TextureId> {
        let mut texture_ids = Vec::new();

        let (w, h) = self.color0.get_size();
        for _ in 0..count {
            let texture_id = self.generate_texture_id();
            let stride = match format {
                ImageFormat::A8 => A_STRIDE,
                ImageFormat::RGBA8 => RGBA_STRIDE,
                _ => unimplemented!(),
            };
            let texture_data = vec![0u8; w * h * stride];

            assert!(self.textures.contains_key(&texture_id) == false);
            self.textures.insert(texture_id, TextureData {id: texture_id, data: texture_data, stride: stride, pitch: w * stride });
            texture_ids.push(texture_id);
        }

        texture_ids
    }

    pub fn create_texture_id(&mut self,
                             _target: TextureTarget,
                             format: ImageFormat) -> TextureId {
        let mut texture_ids = Vec::new();
        let (w, h) = self.color0.get_size();
        let texture_id = self.generate_texture_id();

        let stride = match format {
            ImageFormat::A8 => A_STRIDE,
            ImageFormat::RGBA8 => RGBA_STRIDE,
            ImageFormat::RG8 => RG_STRIDE,
            ImageFormat::RGB8 => RGB_STRIDE,
            _ => unimplemented!(),
        };
        let texture_data = vec![0u8; w * h * stride];
        assert!(self.textures.contains_key(&texture_id) == false);
        self.textures.insert(texture_id, TextureData {id: texture_id, data: texture_data, stride: stride, pitch: w * stride });
        texture_ids.push(texture_id);

        texture_id
    }

    pub fn init_texture(&mut self,
                        texture_id: TextureId,
                        _width: u32,
                        _height: u32,
                        format: ImageFormat,
                        _filter: TextureFilter,
                        _mode: RenderTargetMode,
                        pixels: Option<&[u8]>) {
        println!("init_texture {:?} {:?} {:?} {:?} {:?} {:?}", texture_id, _width, _height, format, _filter, _mode);
        let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        println!("init_texture {:?} {:?}", texture.stride, texture.data.len());
        let stride = match format {
            ImageFormat::A8 => A_STRIDE,
            ImageFormat::RGBA8 => RGBA_STRIDE,
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
                let data = vec![0u8; w * h * texture.stride];
                data
            }
        };
        assert!(texture.data.len() == actual_pixels.len());
        mem::replace(&mut texture.data, actual_pixels);
    }

    pub fn update_texture(&mut self,
                          texture_id: TextureId,
                          x0: u32,
                          y0: u32,
                          width: u32,
                          height: u32,
                          stride: Option<u32>,
                          data: &[u8]) {
        println!("update {:?} x0={:?} y0={:?} width={:?} height={:?} stride={:?} size={:?}", texture_id, x0, y0, width, height, stride, data.len());
        let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        println!("\ttex.stride={:?} tex.pitch={:?} tex.size={:?}", texture.stride, texture.pitch, texture.data.len());
        //assert!(texture.data.len() >= data.len());
        let (w, _) = self.color0.get_size();
        let row_length = match stride {
            Some(value) => value as usize / texture.stride,
            None => width as usize,
        };
        // Take the stride into account for all rows, except the last one.
        let data_pitch = row_length * texture.stride;
        let len = data_pitch * (height - 1) as usize + width as usize * texture.stride;
        //let len = std::cmp::min(texture.stride * row_length * (height - 1) as usize
        //                        + width as usize * texture.stride, data.len());
        println!("\tcomputed len={:?}, height*pitch={:?} data_size={:?}", len, height as usize * data_pitch, data.len());
        println!("\ttarget_width={:?}, row_length={:?}", w, row_length);
        let data = &data[0 .. len];
        Device::update_texture_data(texture, x0 as usize, y0 as usize, width as usize, height as usize, data_pitch, data);
    }

    pub fn resize_texture(&mut self,
                          _texture_id: TextureId,
                          _new_width: u32,
                          _new_height: u32,
                          _format: ImageFormat,
                          _filter: TextureFilter,
                          _mode: RenderTargetMode) {
          println!("Unimplemented! resize_texture");
    }

    pub fn deinit_texture(&mut self, texture_id: TextureId) {
        let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        let (w, h) = self.color0.get_size();
        let data = vec![0u8; w * h * texture.stride];
        assert!(texture.data.len() == data.len());
        mem::replace(&mut texture.data, data.to_vec());
    }

    fn update_texture_data(texture: &mut TextureData,
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
                texture.data[dst_offset ..].copy_from_slice(&src[.. width*texture.stride]);
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
    }

    pub fn bind_texture(&mut self,
                        sampler: TextureSampler,
                        texture_id: TextureId) {
        let texture = match self.textures.get(&texture_id) {
            Some(data) => data,
            None => {
                println!("Didn't find texture! {}", texture_id.name);
                return;
            }
        };
        println!("bind_texture {:?} {:?} {:?} {:?}", texture_id, sampler, texture.stride, texture.data.len());
        match sampler {
            TextureSampler::Color0 => Device::update_texture_u8::<_, Rgba8>(&mut self.encoder, &self.color0, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Color1 => Device::update_texture_u8::<_, Rgba8>(&mut self.encoder, &self.color1, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Color2 => Device::update_texture_u8::<_, Rgba8>(&mut self.encoder, &self.color2, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::CacheA8 => Device::update_texture_u8::<_, A8>(&mut self.encoder, &self.cache_a8, texture.data.as_slice(), A_STRIDE),
            TextureSampler::CacheRGBA8 => Device::update_texture_u8::<_, Rgba8>(&mut self.encoder, &self.cache_rgba8, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Dither => Device::update_texture_u8::<_, A8>(&mut self.encoder, &self.dither, texture.data.as_slice(), A_STRIDE),
            _ => println!("There are only 5 samplers supported. {:?}", sampler),
        }
    }

    pub fn update_sampler_f32(&mut self,
                              sampler: TextureSampler,
                              data: &[f32]) {
        match sampler {
            TextureSampler::Layers => Device::update_texture_f32(&mut self.encoder, &self.layers, data),
            TextureSampler::RenderTasks => Device::update_texture_f32(&mut self.encoder, &self.render_tasks, data),
            TextureSampler::Geometry => Device::update_texture_f32(&mut self.encoder, &self.prim_geo, data),
            TextureSampler::SplitGeometry => Device::update_texture_f32(&mut self.encoder, &self.split_geo, data),
            TextureSampler::Data16 => Device::update_texture_f32(&mut self.encoder, &self.data16, data),
            TextureSampler::Data32 => Device::update_texture_f32(&mut self.encoder, &self.data32, data),
            TextureSampler::Data64 => Device::update_texture_f32(&mut self.encoder, &self.data64, data),
            TextureSampler::Data128 => Device::update_texture_f32(&mut self.encoder, &self.data128, data),
            TextureSampler::ResourceRects => Device::update_texture_f32(&mut self.encoder, &self.resource_rects, data),
            _ => println!("{:?} sampler is not supported", sampler),
        }
    }

    pub fn update_sampler_u8(&mut self,
                             sampler: TextureSampler,
                             data: &[u8]) {
        match sampler {
            TextureSampler::Gradients => Device::update_texture_u8::<_, Rgba8>(&mut self.encoder, &self.gradient_data, data, RGBA_STRIDE),
            _ => println!("{:?} sampler is not supported", sampler),
        }
    }

    pub fn clear_target(&mut self, color: Option<[f32; 4]>, depth: Option<f32>) {
        if let Some(color) = color {
            self.encoder.clear(&self.main_color,
                               //Srgba gamma correction
                               [color[0].powf(2.2),
                                color[1].powf(2.2),
                                color[2].powf(2.2),
                                color[3].powf(2.2)]);
        }

        if let Some(depth) = depth {
            self.encoder.clear_depth(&self.main_depth, depth);
        }
    }

    pub fn flush(&mut self) {
        self.encoder.flush(&mut self.device);
    }

    pub fn draw(&mut self,
                program: &mut Program,
                proj: &Matrix4D<f32>,
                instances: &[PrimitiveInstance],
                _textures: &BatchTextures,
                blendmode: &BlendMode) {
        program.data.transform = proj.to_row_arrays();

        {
            let mut writer = self.factory.write_mapping(&program.upload).unwrap();
            for (i, inst) in instances.iter().enumerate() {
                writer[i].update(inst);
            }
        }

        {
            program.slice.instances = Some((instances.len() as u32, 0));
        }

        if let &BlendMode::Subpixel(ref color) = blendmode {
            program.data.blend_value = [color.r, color.g, color.b, color.a];
        }

        self.encoder.copy_buffer(&program.upload, &program.data.ibuf, 0, 0, program.upload.len()).unwrap();
        self.encoder.draw(&program.slice, &program.get_pso(blendmode), &program.data);
    }

    pub fn update_texture_u8<S, T>(encoder: &mut gfx::Encoder<R,CB>,
                                   texture: &Texture<R, T>,
                                   memory: &[u8],
                                   stride: usize)
        where S: SurfaceTyped + TextureSurface,
              S::DataType: Copy,
              T: Formatted<Surface=S>,
              T::Channel: TextureChannel {
        let tex = &texture.surface;
        let (width, height) = texture.get_size();
        let resized_data = Device::convert_sampler_data_u8(memory, (width * height * stride) as usize);
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
        encoder.update_texture::<_, T>(tex, None, img_info, data).unwrap();
    }

    pub fn update_texture_f32(encoder: &mut gfx::Encoder<R,CB>, texture: &Texture<R, Rgba32F>, memory: &[f32]) {
        let tex = &texture.surface;
        let (width, height) = texture.get_size();
        let resized_data = Device::convert_sampler_data_f32(memory, (width * height * RGBA_STRIDE) as usize);
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
        encoder.update_texture::<_, Rgba32F>(tex, None, img_info, data).unwrap();
    }

    fn convert_sampler_data_u8(data: &[u8], max_size: usize) -> Vec<u8> {
        let mut data = data.to_vec();
        if data.len() < max_size {
            let mut zeros = vec![0u8; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }

    fn convert_sampler_data_f32(data: &[f32], max_size: usize) -> Vec<f32> {
        let mut data = data.to_vec();
        if data.len() < max_size {
            let mut zeros = vec![0f32; max_size - data.len()];
            data.append(&mut zeros);
        }
        assert!(data.len() == max_size);
        data
    }
}

pub fn init_existing<Cf, Df>(window: &glutin::Window) ->
                            (device_gl::Device,device_gl::Factory,
                             gfx::handle::RenderTargetView<R, Cf>, gfx::handle::DepthStencilView<R, Df>)
where Cf: gfx::format::RenderFormat, Df: gfx::format::DepthFormat,
{
    unsafe { window.make_current().unwrap() };
    let (device, factory) = device_gl::create(|s|
        window.get_proc_address(s) as *const std::os::raw::c_void);

    let (width, height) = window.get_inner_size().unwrap();
    let aa = window.get_pixel_format().multisampling.unwrap_or(0) as gfx::texture::NumSamples;
    let dim = ((width as f32 * window.hidpi_factor()) as gfx::texture::Size,
               (height as f32 * window.hidpi_factor()) as gfx::texture::Size,
               1,
               aa.into());

    let (color_view, ds_view) = device_gl::create_main_targets_raw(dim, Cf::get_format().0, Df::get_format().0);
    (device, factory, Typed::new(color_view), Typed::new(ds_view))
}
