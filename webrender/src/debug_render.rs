/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use debug_font_data;
/*use device::{Device, GpuMarker, ProgramId, VAOId, TextureId, VertexFormat};
use device::{TextureFilter, VertexUsageHint, TextureTarget};*/
use device::{Device, TextureId, TextureFilter, TextureTarget, A_STRIDE};
use euclid::{Transform3D, Point2D, Size2D, Rect};
use internal_types::{ORTHO_NEAR_PLANE, ORTHO_FAR_PLANE, TextureSampler};
use internal_types::{DebugFontVertex, DebugColorVertex, RenderTargetMode, PackedColor};
use std::f32;
use webrender_traits::{ColorF, ImageFormat, DeviceUintSize};
use renderer::{create_debug_color_program, create_debug_font_program, transform_projection, BlendMode};
use pipelines::{DebugColorProgram, DebugFontProgram};

pub struct DebugRenderer {
    font_vertices: Vec<DebugFontVertex>,
    font_indices: Vec<u32>,
    font_program: DebugFontProgram,
    font_texture_id: TextureId,
    tri_vertices: Vec<DebugColorVertex>,
    tri_indices: Vec<u32>,
    line_vertices: Vec<DebugColorVertex>,
    color_program: DebugColorProgram,
}

impl DebugRenderer {
    pub fn new(device: &mut Device) -> DebugRenderer {
        let font_program = create_debug_font_program(device, "debug_font");
        let color_program = create_debug_color_program(device, "debug_color");
        let font_texture_id = device.create_empty_texture(debug_font_data::BMP_WIDTH, debug_font_data::BMP_HEIGHT, TextureFilter::Linear, TextureTarget::Default);
        device.update_texture(font_texture_id, 0, 0, debug_font_data::BMP_WIDTH, debug_font_data::BMP_HEIGHT,  ImageFormat::A8, None, Some(&debug_font_data::FONT_BITMAP));

        DebugRenderer {
            font_vertices: Vec::new(),
            font_indices: Vec::new(),
            font_texture_id: font_texture_id,
            tri_vertices: Vec::new(),
            tri_indices: Vec::new(),
            line_vertices: Vec::new(),
            font_program: font_program,
            color_program: color_program,
        }
    }

    pub fn line_height(&self) -> f32 {
        debug_font_data::FONT_SIZE as f32 * 1.1
    }

    pub fn add_text(&mut self,
                    x: f32,
                    y: f32,
                    text: &str,
                    color: &ColorF) -> Rect<f32> {
        let mut x_start = x;
        let ipw = 1.0 / debug_font_data::BMP_WIDTH as f32;
        let iph = 1.0 / debug_font_data::BMP_HEIGHT as f32;

        let mut min_x = f32::MAX;
        let mut max_x = -f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_y = -f32::MAX;

        for c in text.chars() {
            let c = c as usize - debug_font_data::FIRST_GLYPH_INDEX as usize;
            if c < debug_font_data::GLYPHS.len() {
                let glyph = &debug_font_data::GLYPHS[c];

                let x0 = (x_start + glyph.xo + 0.5).floor();
                let y0 = (y + glyph.yo + 0.5).floor();

                let x1 = x0 + glyph.x1 as f32 - glyph.x0 as f32;
                let y1 = y0 + glyph.y1 as f32 - glyph.y0 as f32;

                let s0 = glyph.x0 as f32 * ipw;
                let t0 = glyph.y0 as f32 * iph;
                let s1 = glyph.x1 as f32 * ipw;
                let t1 = glyph.y1 as f32 * iph;

                x_start += glyph.xa;

                let vertex_count = self.font_vertices.len() as u32;

                self.font_vertices.push(DebugFontVertex::new(x0, y0, s0, t0, color.clone()));
                self.font_vertices.push(DebugFontVertex::new(x1, y0, s1, t0, color.clone()));
                self.font_vertices.push(DebugFontVertex::new(x0, y1, s0, t1, color.clone()));
                self.font_vertices.push(DebugFontVertex::new(x1, y1, s1, t1, color.clone()));

                self.font_indices.push(vertex_count + 0);
                self.font_indices.push(vertex_count + 1);
                self.font_indices.push(vertex_count + 2);
                self.font_indices.push(vertex_count + 2);
                self.font_indices.push(vertex_count + 1);
                self.font_indices.push(vertex_count + 3);

                min_x = min_x.min(x0);
                max_x = max_x.max(x1);
                min_y = min_y.min(y0);
                max_y = max_y.max(y1);
            }
        }

        Rect::new(Point2D::new(min_x, min_y), Size2D::new(max_x-min_x, max_y-min_y))
    }

    pub fn add_quad(&mut self,
                    x0: f32,
                    y0: f32,
                    x1: f32,
                    y1: f32,
                    color_top: &ColorF,
                    color_bottom: &ColorF) {
        let vertex_count = self.tri_vertices.len() as u32;

        self.tri_vertices.push(DebugColorVertex::new(x0, y0, color_top.clone()));
        self.tri_vertices.push(DebugColorVertex::new(x1, y0, color_top.clone()));
        self.tri_vertices.push(DebugColorVertex::new(x0, y1, color_bottom.clone()));
        self.tri_vertices.push(DebugColorVertex::new(x1, y1, color_bottom.clone()));

        self.tri_indices.push(vertex_count + 0);
        self.tri_indices.push(vertex_count + 1);
        self.tri_indices.push(vertex_count + 2);
        self.tri_indices.push(vertex_count + 2);
        self.tri_indices.push(vertex_count + 1);
        self.tri_indices.push(vertex_count + 3);
    }

    #[allow(dead_code)]
    pub fn add_line(&mut self,
                    x0: i32,
                    y0: i32,
                    color0: &ColorF,
                    x1: i32,
                    y1: i32,
                    color1: &ColorF) {
        self.line_vertices.push(DebugColorVertex::new(x0 as f32, y0 as f32, color0.clone()));
        self.line_vertices.push(DebugColorVertex::new(x1 as f32, y1 as f32, color1.clone()));
    }

    pub fn render(&mut self,
                  device: &mut Device,
                  viewport_size: &DeviceUintSize) {
        //let _gm = GpuMarker::new(device.rc_gl(), "debug");
        let projection = {
            let projection = Transform3D::ortho(0.0,
                                                viewport_size.width as f32,
                                                viewport_size.height as f32,
                                                0.0,
                                                ORTHO_NEAR_PLANE,
                                                ORTHO_FAR_PLANE);
            transform_projection(projection)
        };

        // Triangles
        if !self.tri_vertices.is_empty() {
            device.draw_debug_color(&mut self.color_program, &projection, &self.tri_indices, &self.tri_vertices);
        }

        // Lines
        /*if !self.line_vertices.is_empty() {
            device.draw_debug(&mut self.color_program, &projection, &[], &BlendMode::Alpha, false);
            /*device.bind_program(self.color_program_id, &projection);
            device.bind_vao(self.line_vao);
            device.update_vao_main_vertices(self.line_vao,
                                            &self.line_vertices,
                                            VertexUsageHint::Dynamic);
            device.draw_nonindexed_lines(0, self.line_vertices.len() as i32);*/
        }*/

        // Glyph
        if !self.font_indices.is_empty() {
            device.bind_texture(TextureSampler::Color0, self.font_texture_id);
            device.draw_debug_font(&mut self.font_program, &projection, &self.font_indices, &self.font_vertices);
        }

        device.flush();
        self.font_indices.clear();
        self.font_vertices.clear();
        self.line_vertices.clear();
        self.tri_vertices.clear();
        self.tri_indices.clear();
    }
}
