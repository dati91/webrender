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
    // device state
    device_pixel_ratio: f32,
    // debug
    inside_frame: bool,
    // misc.
    max_texture_size: u32,
    frame_id: FrameId,
}

impl Device {
    pub fn new(window: &glutin::Window) -> Device {
        let (mut device, mut factory, main_color, main_depth) =
            gfx_window_glutin::init_existing::<ColorFormat, DepthFormat>(window);

        Device {
            // This is initialized to 1 by default, but it is set
            // every frame by the call to begin_frame().
            device_pixel_ratio: 1.0,
            inside_frame: false,
            max_texture_size: 1024,
            frame_id: FrameId(0),
        }
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    pub fn begin_frame(&mut self, device_pixel_ratio: f32) -> FrameId {
        debug_assert!(!self.inside_frame);
        self.inside_frame = true;
        self.device_pixel_ratio = device_pixel_ratio;

        self.frame_id
    }

    pub fn clear_target(&mut self,
                        color: Option<[f32; 4]>,
                        depth: Option<f32>) {
        /*if let Some(color) = color {
            println!("clear:{:?}", color);
            self.encoder.clear(&self.data.out_color, color);
        }

        if let Some(depth) = depth {
            self.encoder.clear_depth(&self.data.out_depth, depth);
        }*/
    }
}
