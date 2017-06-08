/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use euclid::Matrix4D;
use internal_types::{RenderTargetMode, TextureSampler};
use std::collections::HashMap;
use std::mem;
use webrender_traits::ImageFormat;

use rand::Rng;
use std;
use glutin;
use gfx;
use gfx::state::{Blend, BlendChannel, BlendValue, Comparison, Depth, Equation, Factor};
use gfx::memory::Typed;
use gfx::Factory;
use gfx::traits::FactoryExt;
use gfx::format::{DepthStencil as DepthFormat, Rgba8 as ColorFormat};
use gfx_device_gl as device_gl;
use gfx_device_gl::{Resources as R, CommandBuffer as CB};
use gfx::CombinedError;
use gfx::format::{Format, Formatted, R8, Rgba8, Rgba32F, Srgba8, SurfaceTyped, TextureChannel, TextureSurface, Unorm};
use prim_store::GRADIENT_DATA_SIZE;
use tiling::{BlurCommand, CacheClipInstance, PrimitiveInstance};
use renderer::{BlendMode, DITHER_ID, DUMMY_A8_ID, DUMMY_RGBA8_ID, MAX_VERTEX_TEXTURE_WIDTH};
use webrender_traits::DeviceUintRect;

pub type A8 = (R8, Unorm);
pub const VECS_PER_DATA_16: usize = 1;
pub const VECS_PER_DATA_32: usize = 2;
pub const VECS_PER_DATA_64: usize = 4;
pub const VECS_PER_DATA_128: usize = 8;
pub const VECS_PER_GRADIENT_DATA: usize = 520;
pub const VECS_PER_LAYER: usize = 13;
pub const VECS_PER_PRIM_GEOM: usize = 2;
pub const VECS_PER_RENDER_TASK: usize = 3;
pub const VECS_PER_RESOURCE_RECTS: usize = 1;
pub const VECS_PER_SPLIT_GEOM: usize = 3;
pub const LAYER_TEXTURE_WIDTH: usize = 1017;
pub const RENDER_TASK_TEXTURE_WIDTH: usize = 1023;
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

pub const MULTIPLY: Blend = Blend {
    color: BlendChannel {
        equation: Equation::Add,
        source: Factor::Zero,
        destination: Factor::ZeroPlus(BlendValue::SourceColor),
    },
    alpha: BlendChannel {
        equation: Equation::Add,
        source: Factor::Zero,
        destination: Factor::ZeroPlus(BlendValue::SourceAlpha),
    },
};

pub const MAX: Blend = Blend {
    color: BlendChannel {
        equation: Equation::Max,
        source: Factor::One,
        destination: Factor::One,
    },
    alpha: BlendChannel {
        equation: Equation::Add,
        source: Factor::One,
        destination: Factor::One,
    },
};

type PrimPSO = gfx::PipelineState<R, primitive::Meta>;
type CachePSO = gfx::PipelineState<R, cache::Meta>;
type ClipPSO = gfx::PipelineState<R, clip::Meta>;

gfx_defines! {
    vertex Position {
        pos: [f32; 3] = "aPosition",
    }

    /*vertex Instances {
        glob_prim_id: i32 = "aGlobalPrimId",
        primitive_address: i32 = "aPrimitiveAddress",
        task_index: i32 = "aTaskIndex",
        clip_task_index: i32 = "aClipTaskIndex",
        layer_index: i32 = "aLayerIndex",
        element_index: i32 = "aElementIndex",
        user_data: [i32; 2] = "aUserData",
        z_index: i32 = "aZIndex",
    }*/

    vertex PrimitiveInstances {
            data0: [i32; 4] = "aData0",
            data1: [i32; 4] = "aData1",
    }

    vertex BlurInstances {
        render_task_index: i32 = "aBlurRenderTaskIndex",
        source_task_index: i32 = "aBlurSourceTaskIndex",
        direction: i32 = "aBlurDirection",
    }

    vertex ClipInstances {
        render_task_index: i32 = "aClipRenderTaskIndex",
        layer_index: i32 = "aClipLayerIndex",
        data_index: i32 = "aClipDataIndex",
        segment_index: i32 = "aClipSegmentIndex",
    }

    pipeline primitive {
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<Position> = (),
        ibuf: gfx::InstanceBuffer<PrimitiveInstances> = (),

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

    pipeline cache {
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<Position> = (),
        ibuf: gfx::InstanceBuffer<PrimitiveInstances> = (),

        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        /*color1: gfx::TextureSampler<[f32; 4]> = "sColor1",
        color2: gfx::TextureSampler<[f32; 4]> = "sColor2",
        dither: gfx::TextureSampler<f32> = "sDither",*/
        cache_a8: gfx::TextureSampler<f32> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

        data16: gfx::TextureSampler<[f32; 4]> = "sData16",
        data32: gfx::TextureSampler<[f32; 4]> = "sData32",
        data64: gfx::TextureSampler<[f32; 4]> = "sData64",
        data128: gfx::TextureSampler<[f32; 4]> = "sData128",
        //gradients : gfx::TextureSampler<[f32; 4]> = "sGradients",
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
        out_depth: gfx::DepthTarget<DepthFormat> = Depth{fun: Comparison::Never , write: false},
    }

    pipeline blur {
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<Position> = (),
        ibuf: gfx::InstanceBuffer<BlurInstances> = (),

        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        /*color1: gfx::TextureSampler<[f32; 4]> = "sColor1",
        color2: gfx::TextureSampler<[f32; 4]> = "sColor2",
        dither: gfx::TextureSampler<f32> = "sDither",*/
        cache_a8: gfx::TextureSampler<f32> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

        data16: gfx::TextureSampler<[f32; 4]> = "sData16",
        data32: gfx::TextureSampler<[f32; 4]> = "sData32",
        data64: gfx::TextureSampler<[f32; 4]> = "sData64",
        data128: gfx::TextureSampler<[f32; 4]> = "sData128",
        //gradients : gfx::TextureSampler<[f32; 4]> = "sGradients",
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
        out_depth: gfx::DepthTarget<DepthFormat> = Depth{fun: Comparison::Never , write: false},
    }

    pipeline clip {
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<Position> = (),
        ibuf: gfx::InstanceBuffer<ClipInstances> = (),

        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        /*color1: gfx::TextureSampler<[f32; 4]> = "sColor1",
        color2: gfx::TextureSampler<[f32; 4]> = "sColor2",
        dither: gfx::TextureSampler<f32> = "sDither",*/
        cache_a8: gfx::TextureSampler<f32> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

        data16: gfx::TextureSampler<[f32; 4]> = "sData16",
        data32: gfx::TextureSampler<[f32; 4]> = "sData32",
        data64: gfx::TextureSampler<[f32; 4]> = "sData64",
        data128: gfx::TextureSampler<[f32; 4]> = "sData128",
        //gradients : gfx::TextureSampler<[f32; 4]> = "sGradients",
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
        out_depth: gfx::DepthTarget<DepthFormat> = Depth{fun: Comparison::Never , write: false},
    }
}

impl Position {
    fn new(p: [f32; 2]) -> Position {
        Position {
            pos: [p[0], p[1], 0.0],
        }
    }
}

impl PrimitiveInstances {
    fn new() -> PrimitiveInstances {
        PrimitiveInstances {
            data0: [0; 4],
            data1: [0; 4],
        }
    }

    fn update(&mut self, instance: &PrimitiveInstance) {
        self.data0 = [instance.data[0], instance.data[1], instance.data[2], instance.data[3]];
        self.data1 = [instance.data[4], instance.data[5], instance.data[6], instance.data[7]];
    }
}

impl BlurInstances {
    fn new() -> BlurInstances {
        BlurInstances {
            render_task_index: 0,
            source_task_index: 0,
            direction: 0,
        }
    }

    fn update(&mut self, blur_command: &BlurCommand) {
        self.render_task_index = blur_command.task_id;
        self.source_task_index = blur_command.src_task_id;
        self.direction = blur_command.blur_direction;
    }
}

impl ClipInstances {
    fn new() -> ClipInstances {
        ClipInstances {
            render_task_index: 0,
            layer_index: 0,
            data_index: 0,
            segment_index: 0,
        }
    }

    fn update(&mut self, instance: &CacheClipInstance) {
        self.render_task_index = instance.task_id;
        self.layer_index = instance.layer_index;
        self.data_index = instance.address.0;
        self.segment_index = instance.segment;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Texture<R, T> where R: gfx::Resources,
                               T: gfx::format::TextureFormat {
    /// Pixel storage for texture.
    pub surface: gfx::handle::Texture<R, T::Surface>,
    /// Sampler for texture.
    pub sampler: gfx::handle::Sampler<R>,
    /// View used by shader.
    pub view: gfx::handle::ShaderResourceView<R, T::View>,
    /// Filtering mode
    pub filter: TextureFilter,
    /// ImageFormat
    pub format: ImageFormat,
    /// Render Target mode
    pub mode: RenderTargetMode,
}

impl<R, T> Texture<R, T> where R: gfx::Resources, T: gfx::format::TextureFormat {

    pub fn empty<F>(factory: &mut F, size: [usize; 2]) -> Result<Texture<R, T>, CombinedError>
        where F: gfx::Factory<R>
    {
        Texture::create(factory, None, size, TextureFilter::Linear)
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
                gfx::format::SurfaceType::R8 => ImageFormat::A8,
                gfx::format::SurfaceType::R8_G8_B8_A8 | gfx::format::SurfaceType::B8_G8_R8_A8 => ImageFormat::RGBA8,
                gfx::format::SurfaceType::R32_G32_B32_A32 => ImageFormat::RGBAF32,
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
    pub pso: (PrimPSO, PrimPSO),
    pub pso_alpha: (PrimPSO, PrimPSO),
    pub pso_prem_alpha: (PrimPSO, PrimPSO),
    pub pso_subpixel: (PrimPSO, PrimPSO),
    pub slice: gfx::Slice<R>,
    pub upload: gfx::handle::Buffer<R, PrimitiveInstances>,
}

impl Program {
    fn new(data: primitive::Data<R>,
           psos: (PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO),
           slice: gfx::Slice<R>,
           upload: gfx::handle::Buffer<R, PrimitiveInstances>)
           -> Program {
        Program {
            data: data,
            pso: (psos.0, psos.1),
            pso_alpha: (psos.2, psos.3),
            pso_prem_alpha: (psos.4, psos.5),
            pso_subpixel: (psos.6, psos.7),
            slice: slice,
            upload: upload,
        }
    }

    fn get_pso(&self, blend: &BlendMode, depth_write: bool) -> &PrimPSO {
        match *blend {
            BlendMode::Alpha => if depth_write { &self.pso_alpha.0 } else { &self.pso_alpha.1 },
            BlendMode::PremultipliedAlpha => if depth_write { &self.pso_prem_alpha.0 } else { &self.pso_prem_alpha.1 },
            BlendMode::Subpixel(..) => if depth_write { &self.pso_subpixel.0 } else { &self.pso_subpixel.1 },
            _ => if depth_write { &self.pso.0 } else { &self.pso.1 },
        }
    }
}

pub struct CacheProgram {
    pub data: cache::Data<R>,
    pub pso: CachePSO,
    pub pso_alpha: CachePSO,
    pub slice: gfx::Slice<R>,
    pub upload: gfx::handle::Buffer<R, PrimitiveInstances>,
}

impl CacheProgram {
    fn new(data: cache::Data<R>,
           psos: (CachePSO, CachePSO),
           slice: gfx::Slice<R>,
           upload: gfx::handle::Buffer<R, PrimitiveInstances>)
           -> CacheProgram {
        CacheProgram {
            data: data,
            pso: psos.0,
            pso_alpha: psos.1,
            slice: slice,
            upload: upload,
        }
    }

    fn get_pso(&self, blend: &BlendMode) -> &CachePSO {
        match *blend {
            BlendMode::Alpha => &self.pso_alpha,
            _ => &self.pso,
        }
    }
}

pub struct BlurProgram {
    pub data: blur::Data<R>,
    pub pso: gfx::PipelineState<R, blur::Meta>,
    pub slice: gfx::Slice<R>,
    pub upload: gfx::handle::Buffer<R, BlurInstances>,
}

pub struct ClipProgram {
    pub data: clip::Data<R>,
    pub pso: ClipPSO,
    pub pso_multiply: ClipPSO,
    pub pso_max: ClipPSO,
    pub slice: gfx::Slice<R>,
    pub upload: gfx::handle::Buffer<R, ClipInstances>,
}

impl ClipProgram {
    fn new(data: clip::Data<R>,
           psos: (ClipPSO, ClipPSO, ClipPSO),
           slice: gfx::Slice<R>,
           upload: gfx::handle::Buffer<R, ClipInstances>)
           -> ClipProgram {
        ClipProgram {
            data: data,
            pso: psos.0,
            pso_multiply: psos.1,
            pso_max: psos.2,
            slice: slice,
            upload: upload,
        }
    }

    fn get_pso(&self, blend: &BlendMode) -> &ClipPSO {
        match *blend {
            BlendMode::Multiply => &self.pso_multiply,
            BlendMode::Max => &self.pso_max,
            _ => &self.pso,
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
    color0: Texture<R, Srgba8>,
    color1: Texture<R, Rgba8>,
    color2: Texture<R, Rgba8>,
    dither: Texture<R, A8>,
    cache_a8: Texture<R, A8>,
    cache_rgba8: Texture<R, Rgba8>,
    data16: Texture<R, Rgba32F>,
    data32: Texture<R, Rgba32F>,
    data64: Texture<R, Rgba32F>,
    data128: Texture<R, Rgba32F>,
    gradient_data: Texture<R, Srgba8>,
    layers: Texture<R, Rgba32F>,
    prim_geo: Texture<R, Rgba32F>,
    render_tasks: Texture<R, Rgba32F>,
    resource_rects: Texture<R, Rgba32F>,
    split_geo: Texture<R, Rgba32F>,
    max_texture_size: u32,
    main_color: gfx::handle::RenderTargetView<R, ColorFormat>,
    main_depth: gfx::handle::DepthStencilView<R, DepthFormat>,
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
        let texture_size = [std::cmp::max(MAX_VERTEX_TEXTURE_WIDTH, h as usize), std::cmp::max(MAX_VERTEX_TEXTURE_WIDTH, w as usize)];
        let color0 = Texture::empty(&mut factory, texture_size).unwrap();
        let color1 = Texture::empty(&mut factory, texture_size).unwrap();
        let color2 = Texture::empty(&mut factory, texture_size).unwrap();
        let dither = Texture::empty(&mut factory, [8, 8]).unwrap();
        let cache_a8 = Texture::empty(&mut factory, texture_size).unwrap();
        let cache_rgba8 = Texture::empty(&mut factory, texture_size).unwrap();

        // TODO define some maximum boundaries for texture height
        let data16_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH, TEXTURE_HEIGTH * 4]).unwrap();
        let data32_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH, TEXTURE_HEIGTH]).unwrap();
        let data64_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH, TEXTURE_HEIGTH]).unwrap();
        let data128_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH, TEXTURE_HEIGTH * 4]).unwrap();
        let gradient_data = Texture::empty(&mut factory, [2* GRADIENT_DATA_SIZE, TEXTURE_HEIGTH * 10]).unwrap();
        let layers_tex = Texture::empty(&mut factory, [LAYER_TEXTURE_WIDTH, 64]).unwrap();
        let prim_geo_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH, TEXTURE_HEIGTH]).unwrap();
        let render_tasks_tex = Texture::empty(&mut factory, [RENDER_TASK_TEXTURE_WIDTH, TEXTURE_HEIGTH]).unwrap();
        let resource_rects = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH, TEXTURE_HEIGTH * 2]).unwrap();
        let split_geo_tex = Texture::empty(&mut factory, [MAX_VERTEX_TEXTURE_WIDTH, TEXTURE_HEIGTH * 2]).unwrap();

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

    fn create_prim_psos(&mut self, vert_src: &[u8],frag_src: &[u8]) -> (PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO) {
        let pso1 = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::new()
        ).unwrap();

        let pso2 = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_depth: gfx::preset::depth::LESS_EQUAL_TEST,
                .. primitive::new()
            }
        ).unwrap();

        let pso_alpha1 = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(ALPHA)),
                .. primitive::new()
            }
        ).unwrap();

        let pso_alpha2 = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(ALPHA)),
                out_depth: gfx::preset::depth::LESS_EQUAL_TEST,
                .. primitive::new()
            }
        ).unwrap();

        let pso_prem_alpha1 = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(PREM_ALPHA)),
                .. primitive::new()
            }
        ).unwrap();

        let pso_prem_alpha2 = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(PREM_ALPHA)),
            out_depth: gfx::preset::depth::LESS_EQUAL_TEST,
                .. primitive::new()
            }
        ).unwrap();

        let pso_subpixel1 = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(SUBPIXEL)),
                .. primitive::new()
            }
        ).unwrap();

        let pso_subpixel2 = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(SUBPIXEL)),
                out_depth: gfx::preset::depth::LESS_EQUAL_TEST,
                .. primitive::new()
            }
        ).unwrap();

        (pso1, pso2, pso_alpha1, pso_alpha2, pso_prem_alpha1, pso_prem_alpha2, pso_subpixel1, pso_subpixel2)
    }

    fn create_cache_psos(&mut self, vert_src: &[u8],frag_src: &[u8]) -> (CachePSO, CachePSO) {
        let pso = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            cache::new()
        ).unwrap();


        let pso_alpha = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            cache::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(ALPHA)),
                .. cache::new()
            }
        ).unwrap();

        (pso, pso_alpha)
    }

    fn create_clip_psos(&mut self, vert_src: &[u8],frag_src: &[u8]) -> (ClipPSO, ClipPSO, ClipPSO) {
        let pso = self.factory.create_pipeline_simple(vert_src, frag_src, clip::new()).unwrap();

        let pso_multiply = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            clip::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(MULTIPLY)),
                .. clip::new()
            }
        ).unwrap();

        let pso_max = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            clip::Init {
                out_color: ("oFragColor",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Unorm),
                            gfx::state::MASK_ALL,
                            Some(MAX)),
                .. clip::new()
            }
        ).unwrap();
        (pso, pso_multiply, pso_max)
    }

    pub fn create_program(&mut self, vert_src: &[u8], frag_src: &[u8]) -> Program {
        let upload = self.factory.create_upload_buffer(MAX_INSTANCE_COUNT).unwrap();
        {
            let mut writer = self.factory.write_mapping(&upload).unwrap();
            for i in 0..MAX_INSTANCE_COUNT {
                writer[i] = PrimitiveInstances::new();
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
        let psos = self.create_prim_psos(vert_src, frag_src);
        Program::new(data, psos, self.slice.clone(), upload)
    }

    pub fn create_cache_program(&mut self, vert_src: &[u8], frag_src: &[u8]) -> CacheProgram {
        let upload = self.factory.create_upload_buffer(MAX_INSTANCE_COUNT).unwrap();
        {
            let mut writer = self.factory.write_mapping(&upload).unwrap();
            for i in 0..MAX_INSTANCE_COUNT {
                writer[i] = PrimitiveInstances::new();
            }
        }

        let instances = self.factory.create_buffer(MAX_INSTANCE_COUNT,
                                                   gfx::buffer::Role::Vertex,
                                                   gfx::memory::Usage::Data,
                                                   gfx::TRANSFER_DST).unwrap();

        let data = cache::Data {
            transform: [[0f32; 4]; 4],
            device_pixel_ratio: DEVICE_PIXEL_RATIO,
            vbuf: self.vertex_buffer.clone(),
            ibuf: instances,
            color0: (self.color0.clone().view, self.color0.clone().sampler),
            /*color1: (self.color1.clone().view, self.color1.clone().sampler),
            color2: (self.color2.clone().view, self.color2.clone().sampler),
            dither: (self.dither.clone().view, self.dither.clone().sampler),*/
            cache_a8: (self.cache_a8.clone().view, self.cache_a8.clone().sampler),
            cache_rgba8: (self.cache_rgba8.clone().view, self.cache_rgba8.clone().sampler),
            data16: (self.data16.clone().view, self.data16.clone().sampler),
            data32: (self.data32.clone().view, self.data32.clone().sampler),
            data64: (self.data64.clone().view, self.data64.clone().sampler),
            data128: (self.data128.clone().view, self.data128.clone().sampler),
            //gradients: (self.gradient_data.clone().view, self.gradient_data.clone().sampler),
            layers: (self.layers.clone().view, self.layers.clone().sampler),
            prim_geometry: (self.prim_geo.clone().view, self.prim_geo.clone().sampler),
            render_tasks: (self.render_tasks.clone().view, self.render_tasks.clone().sampler),
            resource_rects: (self.resource_rects.clone().view, self.resource_rects.clone().sampler),
            split_geometry: (self.split_geo.clone().view, self.split_geo.clone().sampler),
            out_color: self.main_color.raw().clone(),
            out_depth: self.main_depth.clone(),
            //blend_value: [0.0, 0.0, 0.0, 0.0]
        };
        let psos = self.create_cache_psos(vert_src, frag_src);
        CacheProgram::new(data, psos, self.slice.clone(), upload)
    }

    pub fn create_blur_program(&mut self, vert_src: &[u8], frag_src: &[u8]) -> BlurProgram {
        let upload = self.factory.create_upload_buffer(MAX_INSTANCE_COUNT).unwrap();
        {
            let mut writer = self.factory.write_mapping(&upload).unwrap();
            for i in 0..MAX_INSTANCE_COUNT {
                writer[i] = BlurInstances::new();
            }
        }

        let blur_instances = self.factory.create_buffer(MAX_INSTANCE_COUNT,
                                                        gfx::buffer::Role::Vertex,
                                                        gfx::memory::Usage::Data,
                                                        gfx::TRANSFER_DST).unwrap();

        let data = blur::Data {
            transform: [[0f32; 4]; 4],
            device_pixel_ratio: DEVICE_PIXEL_RATIO,
            vbuf: self.vertex_buffer.clone(),
            ibuf: blur_instances,
            color0: (self.color0.clone().view, self.color0.clone().sampler),
            /*color1: (self.color1.clone().view, self.color1.clone().sampler),
            color2: (self.color2.clone().view, self.color2.clone().sampler),
            dither: (self.dither.clone().view, self.dither.clone().sampler),*/
            cache_a8: (self.cache_a8.clone().view, self.cache_a8.clone().sampler),
            cache_rgba8: (self.cache_rgba8.clone().view, self.cache_rgba8.clone().sampler),
            data16: (self.data16.clone().view, self.data16.clone().sampler),
            data32: (self.data32.clone().view, self.data32.clone().sampler),
            data64: (self.data64.clone().view, self.data64.clone().sampler),
            data128: (self.data128.clone().view, self.data128.clone().sampler),
            //gradients: (self.gradient_data.clone().view, self.gradient_data.clone().sampler),
            layers: (self.layers.clone().view, self.layers.clone().sampler),
            prim_geometry: (self.prim_geo.clone().view, self.prim_geo.clone().sampler),
            render_tasks: (self.render_tasks.clone().view, self.render_tasks.clone().sampler),
            resource_rects: (self.resource_rects.clone().view, self.resource_rects.clone().sampler),
            split_geometry: (self.split_geo.clone().view, self.split_geo.clone().sampler),
            out_color: self.main_color.raw().clone(),
            out_depth: self.main_depth.clone(),
            //blend_value: [0.0, 0.0, 0.0, 0.0]
        };
        let pso = self.factory.create_pipeline_simple(vert_src, frag_src, blur::new()).unwrap();
        BlurProgram {data: data, pso: pso, slice: self.slice.clone(), upload:upload}
    }

    pub fn create_clip_program(&mut self, vert_src: &[u8], frag_src: &[u8]) -> ClipProgram {
        let upload = self.factory.create_upload_buffer(MAX_INSTANCE_COUNT).unwrap();
        {
            let mut writer = self.factory.write_mapping(&upload).unwrap();
            for i in 0..MAX_INSTANCE_COUNT {
                writer[i] = ClipInstances::new();
            }
        }

        let cache_instances = self.factory.create_buffer(MAX_INSTANCE_COUNT,
                                                         gfx::buffer::Role::Vertex,
                                                         gfx::memory::Usage::Data,
                                                         gfx::TRANSFER_DST).unwrap();

        let data = clip::Data {
            transform: [[0f32; 4]; 4],
            device_pixel_ratio: DEVICE_PIXEL_RATIO,
            vbuf: self.vertex_buffer.clone(),
            ibuf: cache_instances,
            color0: (self.color0.clone().view, self.color0.clone().sampler),
            /*color1: (self.color1.clone().view, self.color1.clone().sampler),
            color2: (self.color2.clone().view, self.color2.clone().sampler),
            dither: (self.dither.clone().view, self.dither.clone().sampler),*/
            cache_a8: (self.cache_a8.clone().view, self.cache_a8.clone().sampler),
            cache_rgba8: (self.cache_rgba8.clone().view, self.cache_rgba8.clone().sampler),
            data16: (self.data16.clone().view, self.data16.clone().sampler),
            data32: (self.data32.clone().view, self.data32.clone().sampler),
            data64: (self.data64.clone().view, self.data64.clone().sampler),
            data128: (self.data128.clone().view, self.data128.clone().sampler),
            //gradients: (self.gradient_data.clone().view, self.gradient_data.clone().sampler),
            layers: (self.layers.clone().view, self.layers.clone().sampler),
            prim_geometry: (self.prim_geo.clone().view, self.prim_geo.clone().sampler),
            render_tasks: (self.render_tasks.clone().view, self.render_tasks.clone().sampler),
            resource_rects: (self.resource_rects.clone().view, self.resource_rects.clone().sampler),
            split_geometry: (self.split_geo.clone().view, self.split_geo.clone().sampler),
            out_color: self.main_color.raw().clone(),
            out_depth: self.main_depth.clone(),
            //blend_value: [0.0, 0.0, 0.0, 0.0]
        };
        let psos = self.create_clip_psos(vert_src, frag_src);
        ClipProgram::new(data, psos, self.slice.clone(), upload)
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    fn generate_texture_id(&mut self) -> TextureId {
        use rand::OsRng;

        let mut rng = OsRng::new().unwrap();
        let mut texture_id = TextureId::invalid();
        while self.textures.contains_key(&texture_id) {
            texture_id.name = rng.gen_range(FIRST_UNRESERVED_ID, u32::max_value());
        }
        texture_id
    }

    pub fn create_texture_id(&mut self,
                             _target: TextureTarget,
                             format: ImageFormat) -> TextureId {
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
        assert!(!self.textures.contains_key(&texture_id));
        self.textures.insert(texture_id, TextureData {id: texture_id, data: texture_data, stride: stride, pitch: w * stride });
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
        //println!("init_texture texture_id={:?} _width={:?} _height={:?} format={:?} _filter={:?} _mode={:?}", texture_id, _width, _height, format, _filter, _mode);
        let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        //println!("init_texture texture.stride={:?} texture.data.len={:?}", texture.stride, texture.data.len());
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
                vec![0u8; w * h * texture.stride]
            },
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
        //println!("update {:?} x0={:?} y0={:?} width={:?} height={:?} stride={:?} size={:?}", texture_id, x0, y0, width, height, stride, data.len());
        let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
        assert!(!(texture.data.len() < data.len()));
        //println!("\ttex.stride={:?} tex.pitch={:?} tex.size={:?}", texture.stride, texture.pitch, texture.data.len());
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
        //println!("\tcomputed len={:?}, height*pitch={:?} data_size={:?}", len, height as usize * data_pitch, data.len());
        //println!("\ttarget_width={:?}, row_length={:?}", w, row_length);
        let data = &data[0 .. len];
        Device::update_texture_data(texture, x0 as usize, y0 as usize, width as usize, height as usize, data_pitch, data);
    }

    pub fn resize_texture(&mut self,
                          texture_id: TextureId,
                          new_width: u32,
                          new_height: u32,
                          format: ImageFormat,
                          _filter: TextureFilter,
                          _mode: RenderTargetMode) {
        let texture = self.textures.get_mut(&texture_id).expect("Didn't find texture!");
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
        let new_len = new_width as usize * new_height as usize * texture.stride;
        texture.data.resize(new_len, 0u8);
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
        //println!("bind_texture {:?} {:?} {:?} {:?}", texture_id, sampler, texture.stride, texture.data.len());
        match sampler {
            TextureSampler::Color0 => Device::update_texture_surface(&mut self.encoder, &self.color0, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Color1 => Device::update_texture_surface(&mut self.encoder, &self.color1, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Color2 => Device::update_texture_surface(&mut self.encoder, &self.color2, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::CacheA8 => Device::update_texture_surface(&mut self.encoder, &self.cache_a8, texture.data.as_slice(), A_STRIDE),
            TextureSampler::CacheRGBA8 => Device::update_texture_surface(&mut self.encoder, &self.cache_rgba8, texture.data.as_slice(), RGBA_STRIDE),
            TextureSampler::Dither => Device::update_texture_surface(&mut self.encoder, &self.dither, texture.data.as_slice(), A_STRIDE),
            _ => println!("There are only 5 samplers supported. {:?}", sampler),
        }
    }

    pub fn bind_yuv_texture(&mut self,
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
    }

    fn convert_data_to_rgba8(width: usize, height: usize, data: &[u8], orig_stride: usize) -> Vec<u8> {
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

    pub fn update_sampler_f32(&mut self,
                              sampler: TextureSampler,
                              data: &[f32]) {
        match sampler {
            TextureSampler::Layers => Device::update_texture_surface(&mut self.encoder, &self.layers, data, RGBA_STRIDE),
            TextureSampler::RenderTasks => Device::update_texture_surface(&mut self.encoder, &self.render_tasks, data, RGBA_STRIDE),
            TextureSampler::Geometry => Device::update_texture_surface(&mut self.encoder, &self.prim_geo, data, RGBA_STRIDE),
            TextureSampler::SplitGeometry => Device::update_texture_surface(&mut self.encoder, &self.split_geo, data, RGBA_STRIDE),
            TextureSampler::Data16 => Device::update_texture_surface(&mut self.encoder, &self.data16, data, RGBA_STRIDE),
            TextureSampler::Data32 => Device::update_texture_surface(&mut self.encoder, &self.data32, data, RGBA_STRIDE),
            TextureSampler::Data64 => Device::update_texture_surface(&mut self.encoder, &self.data64, data, RGBA_STRIDE),
            TextureSampler::Data128 => Device::update_texture_surface(&mut self.encoder, &self.data128, data, RGBA_STRIDE),
            TextureSampler::ResourceRects => Device::update_texture_surface(&mut self.encoder, &self.resource_rects, data, RGBA_STRIDE),
            _ => println!("{:?} sampler is not supported", sampler),
        }
    }

    pub fn update_sampler_u8(&mut self,
                             sampler: TextureSampler,
                             data: &[u8]) {
        match sampler {
            TextureSampler::Gradients => Device::update_texture_surface(&mut self.encoder, &self.gradient_data, data, RGBA_STRIDE),
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
                blendmode: &BlendMode,
                enable_depth_write: bool) {
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
        self.encoder.draw(&program.slice, &program.get_pso(blendmode, enable_depth_write), &program.data);
    }

    pub fn draw_cache(&mut self, program: &mut CacheProgram, proj: &Matrix4D<f32>, instances: &[PrimitiveInstance], blendmode: &BlendMode) {
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

        self.encoder.copy_buffer(&program.upload, &program.data.ibuf, 0, 0, program.upload.len()).unwrap();
        self.encoder.draw(&program.slice, &program.get_pso(blendmode), &program.data);
    }

    pub fn draw_blur(&mut self, program: &mut BlurProgram, proj: &Matrix4D<f32>, blur_commands: &[BlurCommand]) {
       program.data.transform = proj.to_row_arrays();

        {
            let mut writer = self.factory.write_mapping(&program.upload).unwrap();
            for (i, blur_command) in blur_commands.iter().enumerate() {
                writer[i].update(blur_command);
            }
        }

        {
            program.slice.instances = Some((blur_commands.len() as u32, 0));
        }

        self.encoder.copy_buffer(&program.upload, &program.data.ibuf, 0, 0, program.upload.len()).unwrap();
        self.encoder.draw(&program.slice, &program.pso, &program.data);
    }

    pub fn draw_clip(&mut self, program: &mut ClipProgram, proj: &Matrix4D<f32>, instances: &[CacheClipInstance], blendmode: &BlendMode) {
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

        self.encoder.copy_buffer(&program.upload, &program.data.ibuf, 0, 0, program.upload.len()).unwrap();
        self.encoder.draw(&program.slice, &program.get_pso(blendmode), &program.data);
    }

    pub fn update_texture_surface<S, F, T>(encoder: &mut gfx::Encoder<R,CB>,
                                           texture: &Texture<R, F>,
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
