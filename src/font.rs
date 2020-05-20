use std::cell::RefCell;
use std::collections::HashMap;

use crate::math;

pub const EVE_SANS_NEUE: &[u8] = include_bytes!("../fonts/evesansneue-regular.otf");

#[derive(Clone, Copy, Debug)]
pub struct TextVertex {
    position: math::V2<f32>,
    uv: math::V2<f32>,
    color: math::V3<f32>,
    alpha: f32,
}
glium::implement_vertex!(TextVertex, position, uv, color, alpha);

#[derive(Copy, Clone, Debug)]
pub struct FontId(pub usize);

impl From<FontId> for usize {
    fn from(other: FontId) -> usize {
        other.0
    }
}

pub struct FontCache {
    cache: RefCell<rusttype::gpu_cache::Cache<'static>>,
    cache_width: u32,
    cache_height: u32,
    cache_texture: glium::Texture2d,
    fonts: Vec<rusttype::Font<'static>>,
    font_ids: HashMap<&'static str, FontId>,
}

impl FontCache {
    pub fn new(display: &glium::Display, cache_width: u32, cache_height: u32) -> Self {
        let cache_texture = glium::Texture2d::empty(display, cache_width, cache_height).unwrap();
        let cache = rusttype::gpu_cache::Cache::builder()
            .dimensions(cache_width, cache_height)
            .position_tolerance(0.2)
            .pad_glyphs(true)
            .multithread(true)
            .build();

        FontCache {
            cache: RefCell::new(cache),
            cache_texture,
            cache_width,
            cache_height,
            fonts: Vec::new(),
            font_ids: HashMap::new(),
        }
    }

    pub fn load(&mut self, name: &'static str, bytes: &'static [u8]) -> Option<FontId> {
        if let Some(&font_id) = self.font_ids.get(name) {
            Some(font_id)
        } else {
            let font = rusttype::Font::try_from_bytes(bytes)?;
            let font_id = self.fonts.len();
            self.fonts.push(font);
            self.font_ids.insert(name, FontId(font_id));

            Some(FontId(font_id))
        }
    }

    pub fn get(&self, font: FontId) -> Option<rusttype::Font<'static>> {
        self.fonts.get(font.0).cloned()
    }

    pub fn texture(&self) -> &glium::Texture2d {
        &self.cache_texture
    }

    pub fn prepare(
        &self,
        font_id: FontId,
        text: &str,
        buffer: &mut Vec<TextVertex>,
        scale: f32,
        position: math::V2<f32>,
        color: math::V3<f32>,
        alpha: f32,
        window_size: math::V2<f32>,
    ) -> Option<()> {
        if let Some(font) = self.get(font_id) {
            let scale = rusttype::Scale::uniform(scale);
            let v_metrics = font.v_metrics(scale);
            let mut advance = math::v2(position.x + 0.0, position.y + v_metrics.ascent);
            let mut positioned_glyphs = Vec::new();

            let mut prev_glyph_id = None;
            for glyph in text.chars().map(|c| font.glyph(c)) {
                if let Some(prev_glyph) = prev_glyph_id {
                    let kerning = font.pair_kerning(scale, prev_glyph, glyph.id());
                    advance.x += kerning;
                }
                prev_glyph_id = Some(glyph.id());

                let glyph = glyph.scaled(scale);
                let h_metrics = glyph.h_metrics();

                let glyph = glyph.positioned(rusttype::point(advance.x, advance.y));
                self.cache
                    .borrow_mut()
                    .queue_glyph(font_id.0, glyph.clone());
                positioned_glyphs.push(glyph);

                advance.x += h_metrics.advance_width;
            }

            self.cache
                .borrow_mut()
                .cache_queued(|region, data| {
                    let rect = glium::Rect {
                        left: region.min.x,
                        bottom: region.min.y,
                        width: region.width(),
                        height: region.height(),
                    };

                    let img_data = glium::texture::RawImage2d {
                        data: data.into(),
                        width: region.width(),
                        height: region.height(),
                        format: glium::texture::ClientFormat::U8,
                    };
                    self.cache_texture.write(rect, img_data);
                })
                .unwrap();

            for glyph in &positioned_glyphs {
                if let Ok(Some((tex_coords, screen_coords))) =
                    self.cache.borrow().rect_for(font_id.0, glyph)
                {
                    let screen_coords_min = math::v2(
                        screen_coords.min.x as f32,
                        window_size.y - screen_coords.min.y as f32,
                    );
                    let screen_coords_max = math::v2(
                        screen_coords.max.x as f32,
                        window_size.y - screen_coords.max.y as f32,
                    );

                    let screen_coords_min = math::v2(screen_coords_min.x, screen_coords_min.y);
                    let screen_coords_max = math::v2(screen_coords_max.x, screen_coords_max.y);
                    let screen_coords_min_max = math::v2(screen_coords_min.x, screen_coords_max.y);
                    let screen_coords_max_min = math::v2(screen_coords_max.x, screen_coords_min.y);

                    let tex_coords_min = math::v2(tex_coords.min.x, tex_coords.min.y);
                    let tex_coords_max = math::v2(tex_coords.max.x, tex_coords.max.y);
                    let tex_coords_min_max = math::v2(tex_coords.min.x, tex_coords.max.y);
                    let tex_coords_max_min = math::v2(tex_coords.max.x, tex_coords.min.y);

                    buffer.push(TextVertex {
                        position: screen_coords_min,
                        uv: tex_coords_min,
                        color,
                        alpha,
                    });
                    buffer.push(TextVertex {
                        position: screen_coords_min_max,
                        uv: tex_coords_min_max,
                        color,
                        alpha,
                    });
                    buffer.push(TextVertex {
                        position: screen_coords_max_min,
                        uv: tex_coords_max_min,
                        color,
                        alpha,
                    });
                    buffer.push(TextVertex {
                        position: screen_coords_max,
                        uv: tex_coords_max,
                        color,
                        alpha,
                    });
                    buffer.push(TextVertex {
                        position: screen_coords_max_min,
                        uv: tex_coords_max_min,
                        color,
                        alpha,
                    });
                    buffer.push(TextVertex {
                        position: screen_coords_min_max,
                        uv: tex_coords_min_max,
                        color,
                        alpha,
                    });
                }
            }

            Some(())
        } else {
            None
        }
    }
}
