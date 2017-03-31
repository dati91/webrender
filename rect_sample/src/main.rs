/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate app_units;
extern crate euclid;
extern crate gleam;
extern crate glutin;
extern crate webrender;
extern crate webrender_traits;

use app_units::Au;
use euclid::Point2D;
use gleam::gl;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use webrender_traits::{BlobImageResult, BlobImageError, BlobImageDescriptor};
use webrender_traits::{ColorF, Epoch, GlyphInstance, ClipRegion, ImageRendering};
use webrender_traits::{ImageDescriptor, ImageData, ImageFormat, PipelineId};
use webrender_traits::{ImageKey, BlobImageData, BlobImageRenderer, RasterizedBlobImage};
use webrender_traits::{LayoutSize, LayoutPoint, LayoutRect, LayoutTransform, DeviceUintSize};

struct Notifier {
    window_proxy: glutin::WindowProxy,
}

impl Notifier {
    fn new(window_proxy: glutin::WindowProxy) -> Notifier {
        Notifier {
            window_proxy: window_proxy,
        }
    }
}

impl webrender_traits::RenderNotifier for Notifier {
    fn new_frame_ready(&mut self) {
        #[cfg(not(target_os = "android"))]
        self.window_proxy.wakeup_event_loop();
    }

    fn new_scroll_frame_ready(&mut self, _composite_needed: bool) {
        #[cfg(not(target_os = "android"))]
        self.window_proxy.wakeup_event_loop();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let window = glutin::WindowBuilder::new()
                .with_title("WebRender Rect Sample")
                .build()
                .unwrap();

    unsafe {
        window.make_current().ok();
    }
    gl::load_with(|symbol| window.get_proc_address(symbol) as *const _);

    println!("OpenGL version {}", gl::get_string(gl::VERSION));

    let (width, height) = window.get_inner_size().unwrap();

    let opts = webrender::RendererOptions {
        debug: true,
        .. Default::default()
    };

    let (mut renderer, sender) = webrender::renderer::Renderer::new(&window, opts).unwrap();
    let api = sender.create_api();

    let notifier = Box::new(Notifier::new(window.create_window_proxy()));
    renderer.set_render_notifier(notifier);

    let epoch = Epoch(0);
    let root_background_color = ColorF::new(0.0, 0.75, 1.0, 1.0);

    let pipeline_id = PipelineId(0, 0);
    let mut builder = webrender_traits::DisplayListBuilder::new(pipeline_id);

    let bounds = LayoutRect::new(LayoutPoint::new(0.0, 0.0), LayoutSize::new(width as f32, height as f32));

    builder.push_stacking_context(webrender_traits::ScrollPolicy::Scrollable,
                                  bounds,
                                  ClipRegion::simple(&bounds),
                                  0,
                                  LayoutTransform::identity().into(),
                                  LayoutTransform::identity(),
                                  webrender_traits::MixBlendMode::Normal,
                                  Vec::new());

    builder.push_rect(
        LayoutRect::new(LayoutPoint::new(100.0, 150.0), LayoutSize::new(250.0, 200.0)),
        ClipRegion::simple(&bounds),
        ColorF::new(1.0, 1.0, 0.0, 1.0)
    );

    api.set_root_display_list(
        Some(root_background_color),
        epoch,
        LayoutSize::new(width as f32, height as f32),
        builder.finalize(),
        true);
    api.set_root_pipeline(pipeline_id);
    api.generate_frame(None);

    for event in window.wait_events() {
        renderer.update();

        renderer.render(DeviceUintSize::new(width, height));

        window.swap_buffers().ok();

        match event {
            glutin::Event::Closed |
            glutin::Event::KeyboardInput(_, _, Some(glutin::VirtualKeyCode::Escape)) |
            glutin::Event::KeyboardInput(_, _, Some(glutin::VirtualKeyCode::Q)) => break,
            _ => ()
        }
    }
}
