/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate app_units;
extern crate euclid;
extern crate gleam;
extern crate glutin;
extern crate webrender;
extern crate webrender_traits;
extern crate lodepng;
extern crate rgb;
#[macro_use]
extern crate lazy_static;

use std::sync::Mutex;
use webrender_traits::*;
use rgb::*;
use app_units::Au;
use gleam::gl;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::rc::Rc;

fn load_file(name: &str) -> Vec<u8> {
    let mut file = File::open(name).unwrap();
    let mut buffer = vec![];
    file.read_to_end(&mut buffer).unwrap();
    buffer
}

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

lazy_static! {
    static ref TRANSFORM: Mutex<LayoutTransform> = Mutex::new(LayoutTransform::identity());
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let res_path = if args.len() > 1 {
        Some(PathBuf::from(&args[1]))
    } else {
        None
    };

    let window = Rc::new(glutin::WindowBuilder::new()
                         .with_title("WebRender Geco2017 with gfx's OpengGl")
                         .with_dimensions(650, 500)
                         .with_gl(glutin::GlRequest::GlThenGles {
                             opengl_version: (3, 2),
                             opengles_version: (3, 0)
                         })
                         .build()
                         .unwrap());

    unsafe {
        window.make_current().ok();
    }

    let gl = match gl::GlType::default() {
        gl::GlType::Gl => unsafe { gl::GlFns::load_with(|symbol| window.get_proc_address(symbol) as *const _) },
        gl::GlType::Gles => unsafe { gl::GlesFns::load_with(|symbol| window.get_proc_address(symbol) as *const _) },
    };

    println!("OpenGL version {}", gl.get_string(gl::VERSION));
    println!("Shader resource path: {:?}", res_path);

    let (width, height) = window.get_inner_size_pixels().unwrap();

    let opts = webrender::RendererOptions {
        resource_override_path: res_path,
        debug: true,
        device_pixel_ratio: window.hidpi_factor(),
        .. Default::default()
    };

    let size = DeviceUintSize::new(width, height);
    let (mut renderer, sender, _) = webrender::renderer::Renderer::new(window.clone(), opts, size).unwrap();
    let api = sender.create_api();

    let notifier = Box::new(Notifier::new(window.create_window_proxy()));
    renderer.set_render_notifier(notifier);

    let epoch = Epoch(0);
    //let root_background_color = ColorF::new(255.0, 255.0, 255.0, 1.0);
    let root_background_color = ColorF::new(0.0, 0.0, 0.0, 1.0);

    let pipeline_id = PipelineId(0, 0);
    let layout_size = LayoutSize::new(width as f32, height as f32);
    let mut builder = webrender_traits::DisplayListBuilder::new(pipeline_id, layout_size);

    let top_left_x = 100.0;
    let top_left_y = 100.0;
    let bg_width = 450.0;
    let bg_height = 300.0;
    let bg_rect = LayoutRect::new(LayoutPoint::new(top_left_x, top_left_y), LayoutSize::new(bg_width, bg_height));

    let bounds = LayoutRect::new(LayoutPoint::zero(), layout_size);
    builder.push_stacking_context(webrender_traits::ScrollPolicy::Scrollable,
                                  bounds,
                                  Some(PropertyBinding::Binding(PropertyBindingKey::new(42))),
                                  TransformStyle::Flat,
                                  None,
                                  webrender_traits::MixBlendMode::Normal,
                                  Vec::new());

    //let scrollbox = LayoutRect::new(LayoutPoint::new(top_left_x - 50.0, top_left_y - 50.0), LayoutSize::new(bg_width+50.0m bg_height+50.0);

    let complex = webrender_traits::ComplexClipRegion::new(
        bg_rect,
        webrender_traits::BorderRadius::uniform(20.0));

    let clip = builder.push_clip_region(&bounds, vec![complex], None);
    builder.push_rect(bg_rect,
                      clip,
                      ColorF::new(242.0/255.0, 233.0/255.0, 225.0/255.0, 0.8));

    
    let border_side = webrender_traits::BorderSide {
        color: ColorF::new(203.0/255.0, 232.0/255.0, 107.0/255.0, 0.8),
        style: webrender_traits::BorderStyle::Double,
    };
    let border_widths = webrender_traits::BorderWidths {
        top: 15.0,
        left: 15.0,
        bottom: 15.0,
        right: 15.0,
    };
    let border_details = webrender_traits::BorderDetails::Normal(webrender_traits::NormalBorder {
        top: border_side,
        right: border_side,
        bottom: border_side,
        left: border_side,
        radius: webrender_traits::BorderRadius::uniform(20.0),
    });

    let clip = builder.push_clip_region(&bounds, vec![], None);
    builder.push_border(bg_rect,
                        clip,
                        border_widths,
                        border_details);

    let image_data = match lodepng::decode32_file("logo.png") {
        Ok(image) => {
            println!("Decoded image {} x {}",
                image.width, image.height);
            image.buffer.as_ref().as_bytes().to_vec()
            }
        Err(reason) => {
            println!("Could not load, because: {}", reason);
            vec![0; 200*200*4]
        }
    };

    let logo_img = api.generate_image_key();
    api.add_image(
        logo_img,
        webrender_traits::ImageDescriptor::new(200, 200, webrender_traits::ImageFormat::BGRA8, true),
        webrender_traits::ImageData::new(image_data),
        None,
    );
    let complex = webrender_traits::ComplexClipRegion::new(
        LayoutRect::new(LayoutPoint::new(125.0, 127.0), LayoutSize::new(140.0, 140.0)),
        webrender_traits::BorderRadius::uniform(60.0));
    let clip = builder.push_clip_region(&bounds, vec![complex], None);
    builder.push_image(
        LayoutRect::new(LayoutPoint::new(100.0, 100.0), LayoutSize::new(200.0, 200.0)),
        clip,
        webrender_traits::LayoutSize::new(200.0, 200.0),
        webrender_traits::LayoutSize::new(0.0, 0.0),
        webrender_traits::ImageRendering::Auto,
        logo_img,
    );

    let font_key = api.generate_font_key();
    let font_bytes = load_file("FreeSans.ttf");
    api.add_raw_font(font_key, font_bytes, 0);

    let text_bounds = LayoutRect::new(LayoutPoint::new(200.0, 300.0), LayoutSize::new(400.0, 300.0));

    let glyphs = vec![
        GlyphInstance {
            index: 40,
            point: LayoutPoint::new(280.0, 250.0),
        },
        GlyphInstance {
            index: 38,
            point: LayoutPoint::new(360.0, 250.0),
        },
        GlyphInstance {
            index: 50,
            point: LayoutPoint::new(450.0, 250.0),
        },
    ];
    let clip = builder.push_clip_region(&bounds, Vec::new(), None);
    builder.push_text(text_bounds,
                        clip,
                        &glyphs,
                        font_key,
                        ColorF::new(1.0, 1.0, 1.0, 1.0),
                        Au::from_px(100),
                        0.0,
                        None);
    let glyphs2 = vec![
        GlyphInstance {
            index: 21,
            point: LayoutPoint::new(250.0, 370.0),
        },
        GlyphInstance {
            index: 19,
            point: LayoutPoint::new(310.0, 370.0),
        },
        GlyphInstance {
            index: 20,
            point: LayoutPoint::new(370.0, 370.0),
        },
        GlyphInstance {
            index: 26,
            point: LayoutPoint::new(430.0, 370.0),
        },
    ];
    let clip = builder.push_clip_region(&bounds, Vec::new(), None);
    builder.push_text(text_bounds,
                        clip,
                        &glyphs2,
                        font_key,
                        ColorF::new(0.0, 0.0, 0.0, 1.0),
                        Au::from_px(100),
                        0.0,
                        None);

    builder.pop_stacking_context();

    api.set_display_list(
        Some(root_background_color),
        epoch,
        LayoutSize::new(width as f32, height as f32),
        builder.finalize(),
        true);
    api.set_root_pipeline(pipeline_id);
    api.generate_frame(None);

    'outer: for event in window.wait_events() {
        let mut events = Vec::new();
        events.push(event);

        for event in window.poll_events() {
            events.push(event);
        }

        for event in events {
            match event {
                glutin::Event::Closed |
                glutin::Event::KeyboardInput(_, _, Some(glutin::VirtualKeyCode::Escape)) |
                glutin::Event::KeyboardInput(_, _, Some(glutin::VirtualKeyCode::Q)) => break 'outer,
                glutin::Event::KeyboardInput(glutin::ElementState::Pressed,
                                             _, Some(glutin::VirtualKeyCode::P)) => {
                    let enable_profiler = !renderer.get_profiler_enabled();
                    renderer.set_profiler_enabled(enable_profiler);
                    api.generate_frame(None);
                }
                glutin::Event::KeyboardInput(glutin::ElementState::Pressed, _, Some(key)) => {
                    let offset = match key {
                        glutin::VirtualKeyCode::Down => (0.0, 10.0),
                        glutin::VirtualKeyCode::Up => (0.0, -10.0),
                        glutin::VirtualKeyCode::Right => (10.0, 0.0),
                        glutin::VirtualKeyCode::Left => (-10.0, 0.0),
                        _ => return,
                    };
                    let new_transform = TRANSFORM.lock().unwrap().post_translate(LayoutVector3D::new(offset.0, offset.1, 0.0));
                    api.generate_frame(Some(DynamicProperties {
                        transforms: vec![
                        PropertyValue {
                            key: PropertyBindingKey::new(42),
                            value: new_transform,
                        },
                        ],
                        floats: vec![],
                    }));
                    *TRANSFORM.lock().unwrap() = new_transform;
                }
                _ => ()
            }
        }

        renderer.update();
        renderer.render(DeviceUintSize::new(width, height));
        window.swap_buffers().ok();
    }
}
