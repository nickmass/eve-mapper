use std::cell::RefCell;
use std::collections::HashMap;

use crate::gfx::TextVertex;
use crate::math;
use crate::platform::{GraphicsBackend, RgbTexture, U8};

pub trait FontData: std::any::Any {
    const DATA: &'static [u8];
}

pub struct EveSansNeue;
impl FontData for EveSansNeue {
    const DATA: &'static [u8] = include_bytes!("../../fonts/evesansneue-regular.otf");
}

pub struct EveSansNeueBold;
impl FontData for EveSansNeueBold {
    const DATA: &'static [u8] = include_bytes!("../../fonts/evesansneue-bold.otf");
}

pub struct NanumGothic;
impl FontData for NanumGothic {
    const DATA: &'static [u8] = include_bytes!("../../fonts/nanumgothic.ttf");
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontId(pub usize);

impl From<FontId> for usize {
    fn from(other: FontId) -> usize {
        other.0
    }
}

pub struct FontCache {
    cache: RefCell<rusttype::gpu_cache::Cache<'static>>,
    cache_texture: RgbTexture<U8>,
    fonts: Vec<rusttype::Font<'static>>,
    font_ids: HashMap<std::any::TypeId, FontId>,
}

impl FontCache {
    pub fn new(display: &GraphicsBackend, cache_width: u32, cache_height: u32) -> Self {
        let cache_texture = display.create_texture(cache_width, cache_height);
        let cache = rusttype::gpu_cache::Cache::builder()
            .dimensions(cache_width, cache_height)
            .position_tolerance(1.0)
            .pad_glyphs(true)
            .multithread(true)
            .build();

        FontCache {
            cache: RefCell::new(cache),
            cache_texture,
            fonts: Vec::new(),
            font_ids: HashMap::new(),
        }
    }

    pub fn load<F: FontData>(&mut self) -> Option<FontId> {
        let type_id = std::any::TypeId::of::<F>();
        if let Some(&font_id) = self.font_ids.get(&type_id) {
            Some(font_id)
        } else {
            let font = rusttype::Font::try_from_bytes(F::DATA)?;
            let font_id = self.fonts.len();
            self.fonts.push(font);
            self.font_ids.insert(type_id, FontId(font_id));

            Some(FontId(font_id))
        }
    }

    pub fn get(&self, font: FontId) -> Option<rusttype::Font<'static>> {
        self.fonts.get(font.0).cloned()
    }

    pub fn texture(&self) -> &RgbTexture<U8> {
        &self.cache_texture
    }

    pub fn layout(
        &self,
        text: TextSpan,
        anchor: TextAnchor,
        position: math::V2<f32>,
        shadow: bool,
    ) -> PositionedTextSpan {
        let scale = rusttype::Scale::uniform(text.scale);
        let mut x_advance = position.x;
        let mut text_bounds: Option<math::Rect<_>> = None;

        let mut positioned_nodes = Vec::new();
        let mut prev_glyph_id = None;
        let mut prev_font = None;

        for node in text.nodes {
            if let Some(font) = self.get(node.font) {
                if Some(node.font) != prev_font {
                    prev_glyph_id = None;
                    prev_font = Some(node.font);
                }
                let mut positioned_glyphs = Vec::new();

                let v_metrics = font.v_metrics(scale);
                let y_advance = position.y + v_metrics.ascent;

                for glyph in node.text.chars().map(|c| font.glyph(c)) {
                    if let Some(prev_glyph) = prev_glyph_id {
                        let kerning = font.pair_kerning(scale, prev_glyph, glyph.id());
                        x_advance += kerning;
                    }
                    prev_glyph_id = Some(glyph.id());

                    let glyph = glyph.scaled(scale);
                    let h_metrics = glyph.h_metrics();

                    let glyph = glyph.positioned(rusttype::point(x_advance, y_advance));

                    if let Some(bounds) = glyph.pixel_bounding_box() {
                        if let Some(text_area) = text_bounds.as_mut() {
                            if bounds.min.x < text_area.min.x {
                                text_area.min.x = bounds.min.x;
                            }
                            if bounds.min.y < text_area.min.y {
                                text_area.min.y = bounds.min.y;
                            }
                            if bounds.max.x > text_area.max.x {
                                text_area.max.x = bounds.max.x;
                            }
                            if bounds.max.y > text_area.max.y {
                                text_area.max.y = bounds.max.y;
                            }
                        } else {
                            text_bounds = Some(math::Rect::new(
                                math::v2(bounds.min.x, bounds.min.y),
                                math::v2(bounds.max.x, bounds.max.y),
                            ))
                        }
                    }

                    x_advance += h_metrics.advance_width;
                    positioned_glyphs.push(glyph);
                }

                positioned_nodes.push(PositionedTextNode {
                    glyphs: positioned_glyphs,
                    color: node.color,
                    font: node.font,
                });
            }
        }

        let bounds = text_bounds.unwrap_or(math::Rect::new(math::V2::fill(0), math::V2::fill(0)));

        for (font, glyph) in positioned_nodes
            .iter()
            .flat_map(|n| n.glyphs.iter().map(move |g| (n.font, g)))
        {
            self.cache.borrow_mut().queue_glyph(font.0, glyph.clone());
        }

        PositionedTextSpan {
            nodes: positioned_nodes,
            bounds,
            //wrong baseline
            baseline: position.y,
            anchor,
            shadow,
        }
    }

    pub fn fill_glyph_cache(&self, display: &GraphicsBackend) {
        let cache_err = self.cache.borrow_mut().cache_queued(|region, data| {
            let region = math::Rect::new(
                math::v2(region.min.x, region.min.y),
                math::v2(region.max.x, region.max.y),
            );

            display.update_texture(self.texture(), region, data);
        });

        if let Err(err) = cache_err {
            log::error!("failed to update font glyph cache: {:?}", err);
        }
    }

    pub fn draw(
        &self,
        text: &PositionedTextSpan,
        buffer: &mut Vec<TextVertex>,
        window_size: math::V2<f32>,
        ui_scale: f32,
    ) {
        let offset = text.bounds.offset(text.anchor);

        let shadow = text.shadow;
        for (shadow, color, font, glyph) in text
            .nodes
            .iter()
            .flat_map(|n| n.glyphs.iter().map(move |g| (shadow, n.color, n.font, g)))
        {
            if let Ok(Some((tex_coords, screen_coords))) =
                self.cache.borrow().rect_for(font.0, glyph)
            {
                let screen_coords_min =
                    (math::v2(screen_coords.min.x, screen_coords.min.y) + offset).as_f32();
                let screen_coords_max =
                    (math::v2(screen_coords.max.x, screen_coords.max.y) + offset).as_f32();

                let screen_coords_min = math::v2(screen_coords_min.x, screen_coords_min.y);
                let screen_coords_max = math::v2(screen_coords_max.x, screen_coords_max.y);

                let tex_coords_min = math::v2(tex_coords.min.x, tex_coords.min.y);
                let tex_coords_max = math::v2(tex_coords.max.x, tex_coords.max.y);

                let screen_rect = math::Rect::new(screen_coords_min, screen_coords_max);
                let tex_rect = math::Rect::new(tex_coords_min, tex_coords_max);

                if shadow {
                    for (position, uv) in screen_rect
                        .triangle_list_iter()
                        .zip(tex_rect.triangle_list_iter())
                    {
                        buffer.push(TextVertex {
                            position: math::v2(
                                position.x + (3.0 * ui_scale),
                                window_size.y - position.y - (3.0 * ui_scale),
                            ),
                            uv,
                            color: math::V3::fill(0.01).expand(color.w),
                        });
                    }
                }

                for (position, uv) in screen_rect
                    .triangle_list_iter()
                    .zip(tex_rect.triangle_list_iter())
                {
                    buffer.push(TextVertex {
                        position: math::v2(position.x, window_size.y - position.y),
                        uv,
                        color,
                    });
                }
            }
        }
    }
}

pub struct PositionedTextSpan {
    nodes: Vec<PositionedTextNode>,
    pub bounds: math::Rect<i32>,
    anchor: TextAnchor,
    shadow: bool,
    pub baseline: f32,
}

pub struct PositionedTextNode {
    glyphs: Vec<rusttype::PositionedGlyph<'static>>,
    color: math::V4<f32>,
    font: FontId,
}

pub struct TextSpan<'a> {
    scale: f32,
    font: FontId,
    color: math::V4<f32>,
    nodes: Vec<TextNode<'a>>,
}

impl<'a> TextSpan<'a> {
    pub fn new(scale: f32, font: FontId, color: math::V4<f32>) -> TextSpan<'a> {
        TextSpan {
            scale,
            font,
            color,
            nodes: Vec::new(),
        }
    }

    pub fn color(&mut self, color: math::V4<f32>) -> &mut Self {
        self.color = color;
        self
    }

    pub fn font(&mut self, font: FontId) -> &mut Self {
        self.font = font;
        self
    }

    pub fn push<S: Into<std::borrow::Cow<'a, str>>>(&mut self, text: S) -> &mut Self {
        self.nodes.push(TextNode {
            color: self.color.clone(),
            font: self.font.clone(),
            text: text.into(),
        });

        self
    }
}

pub struct TextNode<'a> {
    color: math::V4<f32>,
    font: FontId,
    text: std::borrow::Cow<'a, str>,
}

trait TextRectExt<T> {
    fn offset(&self, anchor: TextAnchor) -> math::V2<T>;
}

impl TextRectExt<i32> for math::Rect<i32> {
    fn offset(&self, anchor: TextAnchor) -> math::V2<i32> {
        let x = match anchor {
            TextAnchor::TopLeft | TextAnchor::Left | TextAnchor::BottomLeft => 0,
            TextAnchor::TopRight | TextAnchor::Right | TextAnchor::BottomRight => self.width(),
            TextAnchor::Top | TextAnchor::Center | TextAnchor::Bottom => self.width() / 2,
        };

        let y = match anchor {
            TextAnchor::TopLeft | TextAnchor::Top | TextAnchor::TopRight => 0,
            TextAnchor::BottomLeft | TextAnchor::Bottom | TextAnchor::BottomRight => self.height(),
            TextAnchor::Left | TextAnchor::Center | TextAnchor::Right => self.height() / 2,
        };

        math::v2(-x, -y)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum TextAnchor {
    Center,
    Top,
    TopLeft,
    Left,
    BottomLeft,
    Bottom,
    BottomRight,
    Right,
    TopRight,
}
