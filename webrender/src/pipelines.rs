/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */


use device::{Device, TextureId, DEVICE_PIXEL_RATIO, MAX_INSTANCE_COUNT};
use euclid::{Matrix4D, Transform3D};
use gfx;
use gfx::state::{Blend, BlendChannel, BlendValue, Comparison, Depth, Equation, Factor};
use gfx::memory::Typed;
use gfx::Factory;
use gfx::traits::FactoryExt;
use gfx::format::DepthStencil as DepthFormat;
use backend::Resources as R;
use gfx::format::Format;
use tiling::{BlurCommand, CacheClipInstance, PrimitiveInstance};
use renderer::BlendMode;
use internal_types::{DebugFontVertex, DebugColorVertex};

const ALPHA: Blend = Blend {
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

const PREM_ALPHA: Blend = Blend {
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

const SUBPIXEL: Blend = Blend {
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

const MULTIPLY: Blend = Blend {
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

const MAX: Blend = Blend {
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

gfx_defines! {
    vertex Position {
        pos: [f32; 3] = "aPosition",
    }

    vertex PrimitiveInstances {
            data0: [i32; 4] = "aDataA",
            data1: [i32; 4] = "aDataB",
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
        resource_address: i32 = "aClipResourceAddress",
    }

    constant Locals {
        transform: [[f32; 4]; 4] = "uTransform",
        device_pixel_ratio: f32 = "uDevicePixelRatio",
    }

    pipeline primitive {
        locals: gfx::ConstantBuffer<Locals> = "Locals",
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<Position> = (),
        ibuf: gfx::InstanceBuffer<PrimitiveInstances> = (),

        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        color1: gfx::TextureSampler<[f32; 4]> = "sColor1",
        color2: gfx::TextureSampler<[f32; 4]> = "sColor2",
        dither: gfx::TextureSampler<f32> = "sDither",
        cache_a8: gfx::TextureSampler<[f32; 4]> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

        layers: gfx::TextureSampler<[f32; 4]> = "sLayers",
        render_tasks: gfx::TextureSampler<[f32; 4]> = "sRenderTasks",
        resource_cache: gfx::TextureSampler<[f32; 4]> = "sResourceCache",

        out_color: gfx::RawRenderTarget = ("Target0",
                                           Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
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
        dither: gfx::TextureSampler<f32> = "sDither",
        cache_a8: gfx::TextureSampler<[f32; 4]> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

        layers: gfx::TextureSampler<[f32; 4]> = "sLayers",
        render_tasks: gfx::TextureSampler<[f32; 4]> = "sRenderTasks",
        resource_cache: gfx::TextureSampler<[f32; 4]> = "sResourceCache",

        out_color: gfx::RawRenderTarget = ("Target0",
                                           Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                                           gfx::state::MASK_ALL,
                                           None),
    }

    pipeline blur {
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<Position> = (),
        ibuf: gfx::InstanceBuffer<BlurInstances> = (),

        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        cache_a8: gfx::TextureSampler<[f32; 4]> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

        layers: gfx::TextureSampler<[f32; 4]> = "sLayers",
        render_tasks: gfx::TextureSampler<[f32; 4]> = "sRenderTasks",
        resource_cache: gfx::TextureSampler<[f32; 4]> = "sResourceCache",

        out_color: gfx::RawRenderTarget = ("Target0",
                                           Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                                           gfx::state::MASK_ALL,
                                           None),
        out_depth: gfx::DepthTarget<DepthFormat> = Depth{fun: Comparison::Never , write: false},
    }

    pipeline clip {
        locals: gfx::ConstantBuffer<Locals> = "Locals",
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<Position> = (),
        ibuf: gfx::InstanceBuffer<ClipInstances> = (),

        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        cache_a8: gfx::TextureSampler<[f32; 4]> = "sCacheA8",
        cache_rgba8: gfx::TextureSampler<[f32; 4]> = "sCacheRGBA8",

        layers: gfx::TextureSampler<[f32; 4]> = "sLayers",
        render_tasks: gfx::TextureSampler<[f32; 4]> = "sRenderTasks",
        resource_cache: gfx::TextureSampler<[f32; 4]> = "sResourceCache",

        out_color: gfx::RawRenderTarget = ("Target0",
                                           Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                                           gfx::state::MASK_ALL,
                                           None),
    }

    vertex DebugColorVertices {
        pos: [f32; 2] = "aPosition",
        color: [f32; 4] = "aColor",
    }

    pipeline debug_color {
        locals: gfx::ConstantBuffer<Locals> = "Locals",
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<DebugColorVertices> = (),
        out_color: gfx::RawRenderTarget = ("Target0",
                                           Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                                           gfx::state::MASK_ALL,
                                           Some(ALPHA)),
    }

    vertex DebugFontVertices {
        pos: [f32; 2] = "aPosition",
        color: [f32; 4] = "aColor",
        tex_coord: [f32; 2] = "aColorTexCoord",
    }

    pipeline debug_font {
        locals: gfx::ConstantBuffer<Locals> = "Locals",
        transform: gfx::Global<[[f32; 4]; 4]> = "uTransform",
        device_pixel_ratio: gfx::Global<f32> = "uDevicePixelRatio",
        vbuf: gfx::VertexBuffer<DebugFontVertices> = (),
        color0: gfx::TextureSampler<[f32; 4]> = "sColor0",
        out_color: gfx::RawRenderTarget = ("Target0",
                                           Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                                           gfx::state::MASK_ALL,
                                           Some(ALPHA)),
    }
}

type PrimPSO = gfx::PipelineState<R, primitive::Meta>;
type CachePSO = gfx::PipelineState<R, cache::Meta>;
type ClipPSO = gfx::PipelineState<R, clip::Meta>;
type BlurPSO = gfx::PipelineState<R, blur::Meta>;
type DebugColorPSO = gfx::PipelineState<R, debug_color::Meta>;
type DebugFontPSO = gfx::PipelineState<R, debug_font::Meta>;

impl Position {
    pub fn new(p: [f32; 2]) -> Position {
        Position {
            pos: [p[0], p[1], 0.0],
        }
    }
}

impl DebugColorVertices {
    pub fn new(pos: [f32; 2], color: [f32; 4]) -> DebugColorVertices {
        DebugColorVertices {
            pos: pos,
            color: color,
        }
    }
}

impl DebugFontVertices {
    pub fn new(pos: [f32; 2], color: [f32; 4], tex_coord: [f32; 2]) -> DebugFontVertices {
        DebugFontVertices {
            pos: pos,
            color: color,
            tex_coord: tex_coord,
        }
    }
}

impl PrimitiveInstances {
    pub fn new() -> PrimitiveInstances {
        PrimitiveInstances {
            data0: [0; 4],
            data1: [0; 4],
        }
    }

    pub fn update(&mut self, instance: &PrimitiveInstance) {
        self.data0 = [instance.data[0], instance.data[1], instance.data[2], instance.data[3]];
        self.data1 = [instance.data[4], instance.data[5], instance.data[6], instance.data[7]];
    }
}

impl BlurInstances {
    pub fn new() -> BlurInstances {
        BlurInstances {
            render_task_index: 0,
            source_task_index: 0,
            direction: 0,
        }
    }

    pub fn update(&mut self, blur_command: &BlurCommand) {
        self.render_task_index = blur_command.task_id;
        self.source_task_index = blur_command.src_task_id;
        self.direction = blur_command.blur_direction;
    }
}

impl ClipInstances {
    pub fn new() -> ClipInstances {
        ClipInstances {
            render_task_index: 0,
            layer_index: 0,
            data_index: 0,
            segment_index: 0,
            resource_address: 0,
        }
    }

    pub fn update(&mut self, instance: &CacheClipInstance) {
        self.render_task_index = instance.task_id;
        self.layer_index = instance.layer_index;
        self.data_index = instance.address;
        self.segment_index = instance.segment;
        self.resource_address = instance.resource_address;
    }
}

pub struct Program {
    pub data: primitive::Data<R>,
    pub pso: (PrimPSO, PrimPSO),
    pub pso_alpha: (PrimPSO, PrimPSO),
    pub pso_prem_alpha: (PrimPSO, PrimPSO),
    pub pso_subpixel: (PrimPSO, PrimPSO),
    pub slice: gfx::Slice<R>,
    pub upload: (gfx::handle::Buffer<R, PrimitiveInstances>, usize),
}

impl Program {
    pub fn new(data: primitive::Data<R>,
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
            upload: (upload, 0),
        }
    }

    pub fn get_pso(&self, blend: &BlendMode, depth_write: bool) -> &PrimPSO {
        match *blend {
            BlendMode::Alpha => if depth_write { &self.pso_alpha.0 } else { &self.pso_alpha.1 },
            BlendMode::PremultipliedAlpha => if depth_write { &self.pso_prem_alpha.0 } else { &self.pso_prem_alpha.1 },
            BlendMode::Subpixel(..) => if depth_write { &self.pso_subpixel.0 } else { &self.pso_subpixel.1 },
            _ => if depth_write { &self.pso.0 } else { &self.pso.1 },
        }
    }

    pub fn reset_upload_offset(&mut self) {
        self.upload.1 = 0;
    }
}

#[allow(dead_code)]
pub struct CacheProgram {
    pub data: cache::Data<R>,
    pub pso: CachePSO,
    pub pso_alpha: CachePSO,
    pub slice: gfx::Slice<R>,
    pub upload: (gfx::handle::Buffer<R, PrimitiveInstances>, usize),
}

#[allow(dead_code)]
impl CacheProgram {
    pub fn new(data: cache::Data<R>,
           psos: (CachePSO, CachePSO),
           slice: gfx::Slice<R>,
           upload: gfx::handle::Buffer<R, PrimitiveInstances>)
           -> CacheProgram {
        CacheProgram {
            data: data,
            pso: psos.0,
            pso_alpha: psos.1,
            slice: slice,
            upload: (upload, 0),
        }
    }

    pub fn get_pso(&self, blend: &BlendMode) -> &CachePSO {
        match *blend {
            BlendMode::Alpha => &self.pso_alpha,
            _ => &self.pso,
        }
    }

    pub fn reset_upload_offset(&mut self) {
        self.upload.1 = 0;
    }
}

#[allow(dead_code)]
pub struct BlurProgram {
    pub data: blur::Data<R>,
    pub pso: BlurPSO,
    pub slice: gfx::Slice<R>,
    pub upload: (gfx::handle::Buffer<R, BlurInstances>, usize),
}

#[allow(dead_code)]
impl BlurProgram {
    pub fn new(data: blur::Data<R>,
           pso: BlurPSO,
           slice: gfx::Slice<R>,
           upload: gfx::handle::Buffer<R, BlurInstances>)
           -> BlurProgram {
        BlurProgram {
            data: data,
            pso: pso,
            slice: slice,
            upload: (upload, 0),
        }
    }

    pub fn reset_upload_offset(&mut self) {
        self.upload.1 = 0;
    }
}

#[allow(dead_code)]
pub struct ClipProgram {
    pub data: clip::Data<R>,
    pub pso: ClipPSO,
    pub pso_multiply: ClipPSO,
    pub pso_max: ClipPSO,
    pub slice: gfx::Slice<R>,
    pub upload: (gfx::handle::Buffer<R, ClipInstances>, usize),
}

#[allow(dead_code)]
impl ClipProgram {
    pub fn new(data: clip::Data<R>,
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
            upload: (upload, 0),
        }
    }

    pub fn get_pso(&self, blend: &BlendMode) -> &ClipPSO {
        match *blend {
            BlendMode::Multiply => &self.pso_multiply,
            BlendMode::Max => &self.pso_max,
            _ => &self.pso,
        }
    }

    pub fn reset_upload_offset(&mut self) {
        self.upload.1 = 0;
    }
}

pub struct DebugColorProgram {
    pub data: debug_color::Data<R>,
    pub pso: DebugColorPSO,
    pub slice: gfx::Slice<R>,
}

impl DebugColorProgram {
    pub fn new(data: debug_color::Data<R>,
               pso: DebugColorPSO,
               slice: gfx::Slice<R>)
           -> DebugColorProgram {
        DebugColorProgram {
            data: data,
            pso: pso,
            slice: slice,
        }
    }

    pub fn get_pso(&self) -> &DebugColorPSO {
        &self.pso
    }
}

pub struct DebugFontProgram {
    pub data: debug_font::Data<R>,
    pub pso: DebugFontPSO,
    pub slice: gfx::Slice<R>,
}

impl DebugFontProgram {
    pub fn new(data: debug_font::Data<R>,
               pso: DebugFontPSO,
               slice: gfx::Slice<R>)
           -> DebugFontProgram {
        DebugFontProgram {
            data: data,
            pso: pso,
            slice: slice,
        }
    }

    pub fn get_pso(&self) -> &DebugFontPSO {
        &self.pso
    }
}

impl Device {
    pub fn create_prim_psos(&mut self, vert_src: &[u8], frag_src: &[u8]) -> (PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO, PrimPSO) {
        let pso_depth_write = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::new()
        ).unwrap();

        let pso = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_depth: gfx::preset::depth::LESS_EQUAL_TEST,
                .. primitive::new()
            }
        ).unwrap();

        let pso_alpha_depth_write = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                            gfx::state::MASK_ALL,
                            Some(ALPHA)),
                .. primitive::new()
            }
        ).unwrap();

        let pso_alpha = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                            gfx::state::MASK_ALL,
                            Some(ALPHA)),
                out_depth: gfx::preset::depth::LESS_EQUAL_TEST,
                .. primitive::new()
            }
        ).unwrap();

        let pso_prem_alpha_depth_write = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                            gfx::state::MASK_ALL,
                            Some(PREM_ALPHA)),
                .. primitive::new()
            }
        ).unwrap();

        let pso_prem_alpha = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                            gfx::state::MASK_ALL,
                            Some(PREM_ALPHA)),
            out_depth: gfx::preset::depth::LESS_EQUAL_TEST,
                .. primitive::new()
            }
        ).unwrap();

        let pso_subpixel_depth_write = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                            gfx::state::MASK_ALL,
                            Some(SUBPIXEL)),
                .. primitive::new()
            }
        ).unwrap();

        let pso_subpixel = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            primitive::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                            gfx::state::MASK_ALL,
                            Some(SUBPIXEL)),
                out_depth: gfx::preset::depth::LESS_EQUAL_TEST,
                .. primitive::new()
            }
        ).unwrap();

        (pso_depth_write, pso, pso_alpha_depth_write, pso_alpha, pso_prem_alpha_depth_write,
         pso_prem_alpha, pso_subpixel_depth_write, pso_subpixel)
    }

    pub fn create_cache_psos(&mut self, vert_src: &[u8], frag_src: &[u8]) -> (CachePSO, CachePSO) {
        let pso = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            cache::new()
        ).unwrap();


        let pso_alpha = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            cache::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                            gfx::state::MASK_ALL,
                            Some(ALPHA)),
                .. cache::new()
            }
        ).unwrap();

        (pso, pso_alpha)
    }

    pub fn create_clip_psos(&mut self, vert_src: &[u8], frag_src: &[u8]) -> (ClipPSO, ClipPSO, ClipPSO) {
        let pso = self.factory.create_pipeline_simple(vert_src, frag_src, clip::new()).unwrap();

        let pso_multiply = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            clip::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
                            gfx::state::MASK_ALL,
                            Some(MULTIPLY)),
                .. clip::new()
            }
        ).unwrap();

        let pso_max = self.factory.create_pipeline_simple(
            vert_src,
            frag_src,
            clip::Init {
                out_color: ("Target0",
                            Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
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
            locals: self.factory.create_constant_buffer(1),
            transform: [[0f32; 4]; 4],
            device_pixel_ratio: DEVICE_PIXEL_RATIO,
            vbuf: self.vertex_buffer.clone(),
            ibuf: instances,
            color0: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            color1: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            color2: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            dither: (self.dither.view.clone(), self.dither.sampler.clone()),
            cache_a8: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            cache_rgba8: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            layers: (self.layers.view.clone(), self.layers.clone().sampler),
            render_tasks: (self.render_tasks.view.clone(), self.render_tasks.sampler.clone()),
            resource_cache: (self.resource_cache.view.clone(), self.resource_cache.sampler.clone()),
            out_color: self.main_color.raw().clone(),
            out_depth: self.main_depth.clone(),
            blend_value: [0.0, 0.0, 0.0, 0.0]
        };
        let psos = self.create_prim_psos(vert_src, frag_src);
        Program::new(data, psos, self.slice.clone(), upload)
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
            locals: self.factory.create_constant_buffer(1),
            transform: [[0f32; 4]; 4],
            device_pixel_ratio: DEVICE_PIXEL_RATIO,
            vbuf: self.vertex_buffer.clone(),
            ibuf: cache_instances,
            color0: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            cache_a8: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            cache_rgba8: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            layers: (self.layers.clone().view, self.layers.clone().sampler),
            render_tasks: (self.render_tasks.clone().view, self.render_tasks.clone().sampler),
            resource_cache: (self.resource_cache.clone().view, self.resource_cache.clone().sampler),
            out_color: self.main_color.raw().clone(),
        };
        let psos = self.create_clip_psos(vert_src, frag_src);
        ClipProgram::new(data, psos, self.slice.clone(), upload)
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
            color0: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            dither: (self.dither.clone().view, self.dither.clone().sampler),
            cache_a8: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            cache_rgba8: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            layers: (self.layers.clone().view, self.layers.clone().sampler),
            render_tasks: (self.render_tasks.clone().view, self.render_tasks.clone().sampler),
            resource_cache: (self.resource_cache.clone().view, self.resource_cache.clone().sampler),
            out_color: self.main_color.raw().clone(),
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
            color0: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            cache_a8: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            cache_rgba8: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            layers: (self.layers.clone().view, self.layers.clone().sampler),
            render_tasks: (self.render_tasks.clone().view, self.render_tasks.clone().sampler),
            resource_cache: (self.resource_cache.clone().view, self.resource_cache.clone().sampler),
            out_color: self.main_color.raw().clone(),
            out_depth: self.main_depth.clone(),
        };
        let pso = self.factory.create_pipeline_simple(vert_src, frag_src, blur::new()).unwrap();
        BlurProgram {data: data, pso: pso, slice: self.slice.clone(), upload:(upload,0)}
    }

    pub fn create_debug_color_program(&mut self, vert_src: &[u8], frag_src: &[u8]) -> DebugColorProgram {
        // Creating a dummy vertexbuffer here. This is replaced in the draw_debug_color call.
        let quad_indices: &[u16] = &[0];
        let quad_vertices = [DebugColorVertices::new([0.0, 0.0], [0.0, 0.0, 0.0, 0.0])];
        let (vertex_buffer, mut slice) = self.factory.create_vertex_buffer_with_slice(&quad_vertices, quad_indices);

        let data = debug_color::Data {
            locals: self.factory.create_constant_buffer(1),
            transform: [[0f32; 4]; 4],
            device_pixel_ratio: DEVICE_PIXEL_RATIO,
            vbuf: vertex_buffer,
            out_color: self.main_color.raw().clone(),
        };
        let pso = self.factory.create_pipeline_simple(vert_src, frag_src, debug_color::new()).unwrap();
        DebugColorProgram::new(data, pso, self.slice.clone())
    }

    pub fn create_debug_font_program(&mut self, vert_src: &[u8], frag_src: &[u8]) -> DebugFontProgram {
        // Creating a dummy vertexbuffer here. This is replaced in the draw_debug_font call.
        let quad_indices: &[u16] = &[ 0,];
        let quad_vertices = [DebugFontVertices::new([0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0])];
        let (vertex_buffer, mut slice) = self.factory.create_vertex_buffer_with_slice(&quad_vertices, quad_indices);

        let data = debug_font::Data {
            locals: self.factory.create_constant_buffer(1),
            transform: [[0f32; 4]; 4],
            device_pixel_ratio: DEVICE_PIXEL_RATIO,
            vbuf: vertex_buffer,
            color0: (self.dummy_tex.srv.clone(), self.sampler.clone()),
            out_color: self.main_color.raw().clone(),
        };
        let pso = self.factory.create_pipeline_simple(vert_src, frag_src, debug_font::new()).unwrap();
        DebugFontProgram::new(data, pso, slice)
    }

    pub fn draw(&mut self,
                program: &mut Program,
                proj: &Transform3D<f32>,
                instances: &[PrimitiveInstance],
                blendmode: &BlendMode,
                enable_depth_write: bool) {
        program.data.transform = proj.to_row_arrays();

        {
            let mut writer = self.factory.write_mapping(&program.upload.0).unwrap();
            for (i, inst) in instances.iter().enumerate() {
                writer[i + program.upload.1].update(inst);
            }
        }

        {
            program.slice.instances = Some((instances.len() as u32, 0));
        }

        if let &BlendMode::Subpixel(ref color) = blendmode {
            program.data.blend_value = [color.r, color.g, color.b, color.a];
        }

        let locals = Locals {
            transform: program.data.transform,
            device_pixel_ratio: program.data.device_pixel_ratio,
        };
        if !self.cache_a8_tex_id.is_skipable() {
            println!("set a8");
            program.data.cache_a8 = (self.textures.get(&self.cache_a8_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }
        if !self.cache_rgba8_tex_id.is_skipable() {
            println!("set rgba8");
            program.data.cache_rgba8 = (self.textures.get(&self.cache_rgba8_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }
        if !self.color0_tex_id.is_skipable() {
            println!("set c0");
            program.data.color0 = (self.textures.get(&self.color0_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }
        if !self.color1_tex_id.is_skipable() {
            println!("set c1");
            program.data.color1 = (self.textures.get(&self.color1_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }
        if !self.color2_tex_id.is_skipable() {
            println!("set c2");
            program.data.color2 = (self.textures.get(&self.color2_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }
        self.encoder.update_buffer(&program.data.locals, &[locals], 0).unwrap();
        self.encoder.copy_buffer(&program.upload.0, &program.data.ibuf, program.upload.1, 0, instances.len()).unwrap();
        self.encoder.draw(&program.slice, &program.get_pso(blendmode, enable_depth_write), &program.data);
        program.upload.1 += instances.len();
    }

    pub fn draw_clip(&mut self,
                     program: &mut ClipProgram,
                     proj: &Transform3D<f32>,
                     instances: &[CacheClipInstance],
                     blendmode: &BlendMode,
                     texture_id: TextureId) {
        println!("draw_clip render_target={:?}", texture_id);
        program.data.transform = proj.to_row_arrays();
        let tex = self.textures.get(&texture_id).unwrap().clone();
        program.data.out_color = tex.rtv.unwrap().raw().clone();
        //program.data.out_depth = self.textures.get(&texture_id).unwrap().dsv.clone();
        {
            let mut writer = self.factory.write_mapping(&program.upload.0).unwrap();
            for (i, inst) in instances.iter().enumerate() {
                writer[i + program.upload.1].update(inst);
            }
        }

        {
            program.slice.instances = Some((instances.len() as u32, 0));
        }

        let locals = Locals {
            transform: program.data.transform,
            device_pixel_ratio: program.data.device_pixel_ratio,
        };
        if !self.cache_a8_tex_id.is_skipable() {
            println!("set a8");
            program.data.cache_a8 = (self.textures.get(&self.cache_a8_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }
        if !self.cache_rgba8_tex_id.is_skipable() {
            println!("set rgba8");
            program.data.cache_rgba8 = (self.textures.get(&self.cache_rgba8_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }
        if !self.color0_tex_id.is_skipable() {
            println!("set c0");
            program.data.color0 = (self.textures.get(&self.color0_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }
        self.encoder.update_buffer(&program.data.locals, &[locals], 0).unwrap();
        self.encoder.copy_buffer(&program.upload.0, &program.data.ibuf, program.upload.1, 0, instances.len()).unwrap();
        self.encoder.draw(&program.slice, &program.get_pso(blendmode), &program.data);
        program.upload.1 += instances.len();
    }

    pub fn draw_cache(&mut self, program: &mut CacheProgram, proj: &Matrix4D<f32>, instances: &[PrimitiveInstance], blendmode: &BlendMode) {
        program.data.transform = proj.to_row_arrays();

        {
            let mut writer = self.factory.write_mapping(&program.upload.0).unwrap();
            for (i, inst) in instances.iter().enumerate() {
                writer[i].update(inst);
            }
        }

        {
            program.slice.instances = Some((instances.len() as u32, 0));
        }

        self.encoder.copy_buffer(&program.upload.0, &program.data.ibuf, program.upload.1, 0, instances.len()).unwrap();
        self.encoder.draw(&program.slice, &program.get_pso(blendmode), &program.data);
    }

    pub fn draw_blur(&mut self, program: &mut BlurProgram, proj: &Matrix4D<f32>, blur_commands: &[BlurCommand]) {
        program.data.transform = proj.to_row_arrays();

        {
            let mut writer = self.factory.write_mapping(&program.upload.0).unwrap();
            for (i, blur_command) in blur_commands.iter().enumerate() {
                writer[i].update(blur_command);
            }
        }

        {
            program.slice.instances = Some((blur_commands.len() as u32, 0));
        }

        self.encoder.copy_buffer(&program.upload.0, &program.data.ibuf, program.upload.1, 0, blur_commands.len()).unwrap();
        self.encoder.draw(&program.slice, &program.pso, &program.data);
    }

    pub fn draw_debug_color(&mut self,
                            program: &mut DebugColorProgram,
                            proj: &Matrix4D<f32>,
                            indices: &Vec<u32>,
                            vertices: &Vec<DebugColorVertex>) {
        program.data.transform = proj.to_row_arrays();
        let quad_vertices: Vec<DebugColorVertices> = vertices.iter().map(|v| DebugColorVertices::new([v.x, v.y], v.color.to_array())).collect();

        let (vbuf, slice) = self.factory.create_vertex_buffer_with_slice(&quad_vertices, indices.as_slice());

        {
            program.data.vbuf = vbuf;
            program.slice = slice;
        }

        let locals = Locals {
            transform: program.data.transform,
            device_pixel_ratio: program.data.device_pixel_ratio,
        };
        self.encoder.update_buffer(&program.data.locals, &[locals], 0).unwrap();
        self.encoder.draw(&program.slice, &program.get_pso(), &program.data);
    }

    pub fn draw_debug_font(&mut self,
                            program: &mut DebugFontProgram,
                            proj: &Matrix4D<f32>,
                            indices: &Vec<u32>,
                            vertices: &Vec<DebugFontVertex>) {
        program.data.transform = proj.to_row_arrays();
        let quad_vertices: Vec<DebugFontVertices> = vertices.iter().map(|v| DebugFontVertices::new([v.x, v.y], v.color.to_array(),[v.u, v.v])).collect();

        let (vbuf, slice) = self.factory.create_vertex_buffer_with_slice(&quad_vertices, indices.as_slice());

        {
            program.data.vbuf = vbuf;
            program.slice = slice;
        }

        if !self.color0_tex_id.is_skipable() {
            println!("set c0");
            program.data.color0 = (self.textures.get(&self.color0_tex_id).unwrap().srv.clone(), self.sampler.clone());
        }

        let locals = Locals {
            transform: program.data.transform,
            device_pixel_ratio: program.data.device_pixel_ratio,
        };
        self.encoder.update_buffer(&program.data.locals, &[locals], 0).unwrap();
        self.encoder.draw(&program.slice, &program.get_pso(), &program.data);
    }
}
