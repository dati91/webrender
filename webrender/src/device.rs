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
use tiling::{Frame, PackedLayer, PrimitiveInstance};
use render_task::RenderTaskData;
use prim_store::{GpuBlock16, PrimitiveGeometry};

gfx_defines! {
    vertex min_vertex {
        pos: [f32; 3] = "aPosition",
    }

    vertex min_instance {
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
        vbuf: gfx::VertexBuffer<min_vertex> = (),
        ibuf: gfx::InstanceBuffer<min_instance> = (),
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
        }
    }
}

impl min_instance {
    fn new() -> min_instance {
        min_instance {
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
        self.z_index = instance.z_sort_index * -1;
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

#[derive(Debug, Copy, Clone)]
pub struct FrameId(usize);

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TextureTarget {
    Default,
    Array,
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
    pso: gfx::PipelineState<R, min_primitive::Meta>,
    data: min_primitive::Data<R>,
    slice: gfx::Slice<R>,
    upload: gfx::handle::Buffer<R, min_instance>,
    layers: Texture<R, Rgba32F>,
    render_tasks: Texture<R, Rgba32F>,
    prim_geo: Texture<R, Rgba32F>,
    data16: Texture<R, Rgba32F>,
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
        let max_texture_size = factory.get_capabilities().max_texture_size as u32;

        let pso = factory.create_pipeline_simple(
            include_bytes!("../res/min_ps_rectangle.vert"),
            include_bytes!("../res/min_ps_rectangle.frag"),
            min_primitive::new()
        ).unwrap();

        /*let x0 = -1.0;
        let y0 = -1.0;
        let x1 = 1.0;
        let y1 = 1.0;*/
        let x0 = 0.0;
        let y0 = 0.0;
        let x1 = 1.0;
        let y1 = 1.0;
        /*let x0 = -0.5;
        let y0 = -0.5;
        let x1 = 0.5;
        let y1 = 0.5;*/
        /*let x0 = 0.0;
        let y0 = 0.0;
        let x1 = 1.0;
        let y1 = 1.0;*/

        let quad_indices: &[u16] = &[ 0, 1, 2, 2, 1, 3 ];
        //let quad_indices: &[u16] = &[ 0, 1, 2, 2, 3, 0 ];
        let min_quad_vertices = [
            min_vertex::new([x0, y0]),
            min_vertex::new([x1, y0]),
            min_vertex::new([x0, y1]),
            min_vertex::new([x1, y1]),
        ];
        /*let min_quad_vertices = [
            min_vertex::new([x0, y1]),
            min_vertex::new([x0, y0]),
            min_vertex::new([x1, y0]),
            min_vertex::new([x1, y1]),
        ];*/

        let instance_count = 2;
        let upload = factory.create_upload_buffer(instance_count as usize).unwrap();
        {
            let mut writer = factory.write_mapping(&upload).unwrap();
            println!("writer len: {}", writer.len());
            writer[0] = min_instance::new();
            writer[1] = min_instance::new();
        }

        let instances = factory
            .create_buffer(instance_count as usize,
                           gfx::buffer::Role::Vertex,
                           gfx::memory::Usage::Data,
                           gfx::TRANSFER_DST).unwrap();

        let (vertex_buffer, mut slice) = factory.create_vertex_buffer_with_slice(&min_quad_vertices, quad_indices);
        slice.instances = Some((instance_count, 0));
        let layers_tex = Texture::empty(&mut factory, [13, 6]).unwrap();
        let render_tasks_tex = Texture::empty(&mut factory, [3, 1]).unwrap();
        let prim_geo_tex = Texture::empty(&mut factory, [2, 2]).unwrap();
        let data16_tex = Texture::empty(&mut factory, [16, 16]).unwrap();

        let data = min_primitive::Data {
            transform: [[0f32;4];4],
            device_pixel_ratio: 1f32,
            vbuf: vertex_buffer,
            ibuf: instances,
            layers: (layers_tex.clone().view, layers_tex.clone().sampler),
            render_tasks: (render_tasks_tex.clone().view, render_tasks_tex.clone().sampler),
            prim_geometry: (prim_geo_tex.clone().view, prim_geo_tex.clone().sampler),
            data16: (data16_tex.clone().view, data16_tex.clone().sampler),
            out_color: main_color,
            out_depth: main_depth,
        };

        Device {
            device: device,
            factory: factory,
            encoder: encoder,
            pso: pso,
            data: data,
            slice: slice,
            upload: upload,
            layers: layers_tex,
            render_tasks: render_tasks_tex,
            prim_geo: prim_geo_tex,
            data16: data16_tex,
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
        if frame.render_task_data.len() == 1 {
            println!("{:?}", frame.render_task_data);
            println!("{:?}", frame.layer_texture_data);
            println!("{:?}", frame.gpu_geometry[0]);
            println!("{:?}", frame.gpu_geometry[1]);
            //println!("{:?}", frame.gpu_data16.data);
        }
        Device::update_texture_f32(&mut self.encoder, &self.data16, unsafe { mem::transmute(frame.gpu_data16.as_slice()) });
        Device::update_texture_f32(&mut self.encoder, &self.layers, Device::convert_layer(frame.layer_texture_data.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.render_tasks, Device::convert_render_task(frame.render_task_data.clone()).as_slice());
        Device::update_texture_f32(&mut self.encoder, &self.prim_geo, Device::convert_prim_geo(frame.gpu_geometry.clone()).as_slice());
        //self.draw();
    }

    pub fn draw(&mut self, proj: &Matrix4D<f32>, instances: &[PrimitiveInstance]) {
        println!("draw!");
        println!("proj: {:?}", proj);
        println!("data: {:?}", instances);
        self.data.transform = proj.to_row_arrays();
        {
            let mut writer = self.factory.write_mapping(&self.upload).unwrap();
            println!("writer! {}", writer.len());
            for (i, inst) in instances.iter().enumerate() {
                println!("instance[{}]: {:?}", i, inst);
                writer[i].update(inst);
                println!("{:?}", writer[i]);
            }
            //writer[0].update(&instances[0]);
        }
        //println!("upload {:?}", &self.upload);
        println!("copy");
        self.encoder.copy_buffer(&self.upload, &self.data.ibuf,
                            0, 0, self.upload.len()).unwrap();
        println!("draw & flush");
        /*println!("vbuf {:?}", self.data.vbuf.get_info());
        println!("ibuf {:?}", self.data.ibuf);
        println!("layers {:?}", self.layers);
        println!("render_tasks {:?}", self.render_tasks);
        println!("prim_geo {:?}", self.prim_geo);
        println!("data16 {:?}", self.data16);*/
        self.encoder.draw(&self.slice, &self.pso, &self.data);
        self.encoder.flush(&mut self.device);
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

    fn convert_data16(data16: Vec<GpuBlock16>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for d in data16 {
            println!("{:?}", d.data);
            data.append(& mut d.data.to_vec());
        }
        println!("convert_data16 len {:?}", data.len());
        data
    }

    fn convert_layer(layers: Vec<PackedLayer>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for l in layers {
            println!("{:?}", l);
            data.append(& mut l.transform.to_row_major_array().to_vec());
            data.append(& mut l.inv_transform.to_row_major_array().to_vec());
            data.append(& mut l.local_clip_rect.origin.to_array().to_vec());
            data.append(& mut l.local_clip_rect.size.to_array().to_vec());
            data.append(& mut l.screen_vertices[0].to_array().to_vec());
            data.append(& mut l.screen_vertices[1].to_array().to_vec());
            data.append(& mut l.screen_vertices[2].to_array().to_vec());
            data.append(& mut l.screen_vertices[3].to_array().to_vec());
        }
        println!("convert_layer len {:?}", data.len());
        data
    }

    fn convert_render_task(render_tasks: Vec<RenderTaskData>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for rt in render_tasks {
            println!("{:?}", rt);
            data.append(& mut rt.data.to_vec());
        }
        println!("convert_render_task len {:?}", data.len());
        data
    }

    fn convert_prim_geo(prim_geo: Vec<PrimitiveGeometry>) -> Vec<f32> {
        let mut data: Vec<f32> = vec!();
        for pg in prim_geo {
            if data.len() == 16 {
                break;
            }
            println!("pg {:?}", pg);
            data.append(& mut pg.local_rect.origin.to_array().to_vec());
            data.append(& mut pg.local_rect.size.to_array().to_vec());
            data.append(& mut pg.local_clip_rect.origin.to_array().to_vec());
            data.append(& mut pg.local_clip_rect.size.to_array().to_vec());
            println!("data: {:?}", data);
        }
        println!("convert_prim_geo len {:?}", data.len());
        data
    }
}
