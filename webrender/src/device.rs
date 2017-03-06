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
use gfx::traits::FactoryExt;
use gfx::format::{DepthStencil as DepthFormat, Rgba8 as ColorFormat};
use gfx_device_gl as device_gl;
use gfx_device_gl::{Resources as R, CommandBuffer as CB};
use gfx_window_glutin;

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
    gfx_device: device_gl::Device,
    gfx_factory: device_gl::Factory,
    gfx_encoder: gfx::Encoder<R,CB>,
    //gfx_data: pipe::Data<R>,
    //gfx_pso: gfx::PipelineState<R, pipe::Meta>,
    //gfx_slice: gfx::Slice<R>,
    max_texture_size: u32,
}

impl Device {
    pub fn new(window: &glutin::Window) -> Device {
        let (mut device, mut factory, color_view, ds_view) =
            gfx_window_glutin::init_existing::<ColorFormat, DepthFormat>(window);
        println!("Vendor: {:?}", device.get_info().platform_name.vendor);
        println!("Renderer: {:?}", device.get_info().platform_name.renderer);
        println!("Version: {:?}", device.get_info().version);
        println!("Shading Language: {:?}", device.get_info().shading_language);
        let encoder: gfx::Encoder<_,_> = factory.create_command_buffer().into();
        let max_texture_size = factory.get_capabilities().max_texture_size as u32;
        Device {
            gfx_device: device,
            gfx_factory: factory,
            gfx_encoder: encoder,
            max_texture_size: max_texture_size,
        }
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }
}
