/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate app_units;
extern crate euclid;
extern crate winit;
extern crate webrender;
extern crate webrender_traits;

use app_units::Au;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::rc::Rc;
use webrender_traits::{ColorF, Epoch, GlyphInstance};
use webrender_traits::{DeviceUintSize, LayoutPoint, LayoutRect, LayoutSize};
use webrender_traits::{PipelineId, TransformStyle, BoxShadowClipMode};
use euclid::vec2;
fn load_file(name: &str) -> Vec<u8> {
    let mut file = File::open(name).unwrap();
    let mut buffer = vec![];
    file.read_to_end(&mut buffer).unwrap();
    buffer
}

struct Notifier {
    proxy: winit::EventsLoopProxy,
}

impl Notifier {
    fn new(proxy: winit::EventsLoopProxy) -> Notifier {
        Notifier {
            proxy: proxy,
        }
    }
}

impl webrender_traits::RenderNotifier for Notifier {
    fn new_frame_ready(&mut self) {
        #[cfg(not(target_os = "android"))]
        self.proxy.wakeup();
    }

    fn new_scroll_frame_ready(&mut self, _composite_needed: bool) {
        #[cfg(not(target_os = "android"))]
        self.proxy.wakeup();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let res_path = if args.len() > 1 {
        Some(PathBuf::from(&args[1]))
    } else {
        None
    };
    let mut events_loop = winit::EventsLoop::new();
    let window = Rc::new(winit::WindowBuilder::new()
                         .with_title("WebRender Sample")
                         .with_dimensions(600, 600)
                         .build(&events_loop)
                         .unwrap());

    let (width, height) = window.get_inner_size_pixels().unwrap();

    let opts = webrender::RendererOptions {
        resource_override_path: res_path,
        debug: true,
        device_pixel_ratio: window.hidpi_factor(),
        .. Default::default()
    };

    let size = DeviceUintSize::new(width, height);
    let (mut renderer, sender, gfx_window) = webrender::renderer::Renderer::new(window.clone(), opts, size).unwrap();
    let api = sender.create_api();

    let notifier = Box::new(Notifier::new(events_loop.create_proxy()));
    renderer.set_render_notifier(notifier);

    let epoch = Epoch(0);
    //let root_background_color = ColorF::new(173.0/255.0, 173.0/255.0, 173.0/255.0, 1.0);
    let root_background_color = ColorF::new(0.0, 0.0, 0.0, 1.0);

    let pipeline_id = PipelineId(0, 0);
    let layout_size = LayoutSize::new(width as f32, height as f32);
    let mut builder = webrender_traits::DisplayListBuilder::new(pipeline_id, layout_size);

    let bounds = LayoutRect::new(LayoutPoint::zero(), layout_size);
    builder.push_stacking_context(webrender_traits::ScrollPolicy::Scrollable,
                                  bounds,
                                  None,
                                  TransformStyle::Flat,
                                  None,
                                  webrender_traits::MixBlendMode::Normal,
                                  Vec::new());

    let clip = builder.push_clip_region(&bounds, vec![], None);
    builder.push_rect(LayoutRect::new(LayoutPoint::new(50.0, 50.0), LayoutSize::new(500.0, 500.0)),
                      clip,
                      ColorF::new(0.5, 0.4, 0.1, 0.2));

    let clip = builder.push_clip_region(&bounds, vec![], None);
    builder.push_rect(LayoutRect::new(LayoutPoint::new(100.0, 100.0), LayoutSize::new(80.0, 400.0)),
                      clip,
                      ColorF::new(0.0, 0.8, 0.3, 0.8));
    
    let clip = builder.push_clip_region(&bounds, vec![], None);
    builder.push_rect(LayoutRect::new(LayoutPoint::new(100.0, 420.0), LayoutSize::new(280.0, 80.0)),
                      clip,
                      ColorF::new(0.0, 0.0, 1.0, 0.8));
    
    let clip = builder.push_clip_region(&bounds, vec![], None);
    builder.push_rect(LayoutRect::new(LayoutPoint::new(180.0, 100.0), LayoutSize::new(200.0, 80.0)),
                      clip,
                      ColorF::new(0.3, 0.0, 0.7, 1.0));

    let clip = builder.push_clip_region(&bounds, vec![], None);
    builder.push_rect(LayoutRect::new(LayoutPoint::new(380.0, 80.0), LayoutSize::new(80.0, 130.0)),
                      clip,
                      ColorF::new(0.3, 0.7, 0.2, 1.0));

    let clip = builder.push_clip_region(&bounds, vec![], None);
    builder.push_rect(LayoutRect::new(LayoutPoint::new(380.0, 380.0), LayoutSize::new(80.0, 130.0)),
                      clip,
                      ColorF::new(175.0/255.0, 95.0/255.0, 79.0/255.0, 1.0));

    let clip = builder.push_clip_region(&bounds, vec![], None);
    builder.push_rect(LayoutRect::new(LayoutPoint::new(330.0, 320.0), LayoutSize::new(160.0, 60.0)),
                      clip,
                      ColorF::new(220.0/255.0, 180.0/255.0, 40.0/255.0, 1.0));

    builder.pop_stacking_context();

    api.set_display_list(
        Some(root_background_color),
        epoch,
        LayoutSize::new(width as f32, height as f32),
        builder.finalize(),
        true);
    api.set_root_pipeline(pipeline_id);
    api.generate_frame(None);

    events_loop.run_forever(|event| {
        match event {
            winit::Event::WindowEvent { event: winit::WindowEvent::Closed, .. } => {
                winit::ControlFlow::Break
            },
            winit::Event::WindowEvent { 
                event: winit::WindowEvent::KeyboardInput { 
                    input: winit::KeyboardInput {state: winit::ElementState::Pressed,
                                                 virtual_keycode: Some(winit::VirtualKeyCode::P), .. }, .. }, .. } => {
                let enable_profiler = !renderer.get_profiler_enabled();
                renderer.set_profiler_enabled(enable_profiler);
                api.generate_frame(None);
                winit::ControlFlow::Continue
            },
            _ => {
                renderer.update();
                renderer.render(DeviceUintSize::new(width, height));
                gfx_window.swap_buffers(1);
                winit::ControlFlow::Continue
            },
        }
    });
}