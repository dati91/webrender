/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate app_units;
extern crate clap;
extern crate euclid;
extern crate image;
#[cfg(feature = "gl")]
extern crate gleam;
#[cfg(feature = "gl")]
extern crate glutin;
extern crate webrender;
extern crate winit;
extern crate yaml_rust;

#[path = "common/boilerplate.rs"]
mod boilerplate;
#[path = "common/yaml_reader.rs"]
mod yaml_reader;
#[path = "common/yaml_helper.rs"]
mod yaml_helper;
#[path = "common/parse_function.rs"]
mod parse_function;
#[path = "common/premultiply.rs"]
mod premultiply;

use boilerplate::{Example, HandyDandyRectBuilder};
#[cfg(feature = "gl")]
use gleam::gl;
use std::mem;
use webrender::api::*;
use yaml_reader::*;

struct App {
    frame_count: u32,
}

impl Example for App {
    fn render(
        &mut self,
        api: &RenderApi,
        builder: &mut DisplayListBuilder,
        txn: &mut Transaction,
        _framebuffer_size: DeviceUintSize,
        pipeline_id: PipelineId,
        _document_id: DocumentId,
    ) {
        println!("pipeline_id={:?} _document_id={:?}", pipeline_id, _document_id);
        let count = self.frame_count % 3;
        println!("frame_count={:?} count={:?}", self.frame_count, count);
        match count {
            0 => {
                println!("render tile-size.yaml");
                let mut renderer = YamlFrameReader::new_from_args("../wrench/reftests/image/tile-size.yaml");
                renderer.build(api, builder, pipeline_id);
                api.flush_scene_builder();
            },
            1 => {
                println!("render tile-with-spacing.yaml");
                let mut renderer = YamlFrameReader::new_from_args("../wrench/reftests/image/tile-with-spacing.yaml");
                renderer.build(api, builder, pipeline_id);
                api.flush_scene_builder();
            },
            2 => {
                println!("render rounded-corners");
                let mut renderer = YamlFrameReader::new_from_args("../wrench/reftests/mask/rounded-corners.yaml");
                renderer.build(api, builder, pipeline_id);
                api.flush_scene_builder();
            },
            _ => unreachable!(),
        }
        self.frame_count+=1;
    }

    fn on_event(
        &mut self,
        event: winit::WindowEvent,
        api: &RenderApi,
        _document_id: DocumentId,
    ) -> bool {
        match event {
            winit::WindowEvent::KeyboardInput {
                input: winit::KeyboardInput {
                    state: winit::ElementState::Pressed,
                    virtual_keycode: Some(key),
                    ..
                },
                ..
            } => {
                println!("key_pressed key={:?}", key);
                let mut txn = Transaction::new();

                match key {
                    winit::VirtualKeyCode::Key7 => {

                    }
                    winit::VirtualKeyCode::Key8 => {

                    },
                    winit::VirtualKeyCode::Key9 => {
                        
                    },
                    _ => {}
                }

                api.update_resources(txn.resource_updates);
                return true;
            }
            _ => {}
        }

        false
    }
}

fn main() {
    let mut app = App {
        frame_count: 0u32,
    };
    boilerplate::main_wrapper(&mut app, None);
}
