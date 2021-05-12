use std::any::TypeId;
use std::cell::{Cell, RefCell};

use crate::gfx::TextVertex;
use crate::math;
use crate::platform::{GraphicsBackend, RgbTexture, U8};

use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use fontdue::Font;

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

#[allow(dead_code)]
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

pub struct PositionedTextSpan {
    glyphs: Vec<fontdue::layout::GlyphPosition<math::V4<f32>>>,
    pub bounds: math::Rect<i32>,
    anchor: TextAnchor,
    shadow: bool,
}

struct CacheCursor {
    cache_width: u32,
    cache_height: u32,
    x: Cell<u32>,
    y: Cell<u32>,
    line_y: Cell<u32>,
}

impl CacheCursor {
    fn new(cache_width: u32, cache_height: u32) -> Self {
        CacheCursor {
            cache_width,
            cache_height,
            x: Cell::new(1),
            y: Cell::new(1),
            line_y: Cell::new(0),
        }
    }

    fn reset(&self) {
        self.x.set(1);
        self.y.set(1);
        self.line_y.set(0);
    }

    fn advance(&self, metrics: fontdue::Metrics) -> Option<math::Rect<u32>> {
        let width = metrics.width as u32;
        let height = metrics.height as u32;
        if self.x.get() + width + 1 > self.cache_width {
            self.x.set(1);
            self.y.set(self.y.get() + self.line_y.get() + 1);
            self.line_y.set(0);
        }

        if self.y.get() + height + 1 > self.cache_height {
            return None;
        }

        self.line_y.set(self.line_y.get().max(height));

        let corner = math::v2(self.x.get(), self.y.get());
        let dims = math::v2(width, height);

        self.x.set(self.x.get() + width + 1);

        Some(math::Rect::new(corner, corner + dims))
    }
}

pub struct FontCache {
    cache_texture: RgbTexture<U8>,
    cache_width: u32,
    cache_height: u32,
    fonts: Vec<Font>,
    font_ids: HashMap<TypeId, FontId>,
    layout: RefCell<fontdue::layout::Layout<math::V4<f32>>>,
    frame_glyphs: RefCell<HashSet<fontdue::layout::GlyphRasterConfig>>,
    cache_glyphs:
        RefCell<HashMap<fontdue::layout::GlyphRasterConfig, (math::Rect<f32>, math::Rect<f32>)>>,
    cache_cursor: CacheCursor,
}

impl FontCache {
    pub fn new(display: &GraphicsBackend, cache_width: u32, cache_height: u32) -> Self {
        let cache_texture = display.create_texture(cache_width, cache_height);
        let layout = RefCell::new(fontdue::layout::Layout::new(
            fontdue::layout::CoordinateSystem::PositiveYDown,
        ));
        FontCache {
            cache_texture,
            cache_width,
            cache_height,
            fonts: Vec::new(),
            font_ids: HashMap::new(),
            layout,
            frame_glyphs: RefCell::new(HashSet::new()),
            cache_glyphs: RefCell::new(HashMap::new()),
            cache_cursor: CacheCursor::new(cache_width, cache_height),
        }
    }

    pub fn load<F: FontData>(&mut self) -> Option<FontId> {
        let type_id = TypeId::of::<F>();
        if let Some(&font_id) = self.font_ids.get(&type_id) {
            Some(font_id)
        } else {
            let mut font_settings = fontdue::FontSettings::default();
            font_settings.scale = 40.0;
            let font = Font::from_bytes(F::DATA, font_settings).ok()?;
            let font_id = self.fonts.len();
            self.fonts.push(font);
            self.font_ids.insert(type_id, FontId(font_id));

            Some(FontId(font_id))
        }
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
        let mut layout = self.layout.borrow_mut();

        let mut settings = fontdue::layout::LayoutSettings::default();
        settings.x = position.x;
        settings.y = position.y;

        layout.reset(&settings);

        for node in text.nodes {
            let style = fontdue::layout::TextStyle::with_user_data(
                &node.text,
                text.scale * 0.75,
                node.font.0,
                node.color,
            );
            layout.append(&self.fonts, &style);
        }

        let glyphs = layout.glyphs().clone();

        let bounds_y = layout.height() as i32;
        let bounds_x = glyphs
            .iter()
            .map(|g| (g.x + g.width as f32) as i32)
            .max()
            .unwrap_or(0);

        let position = math::v2(position.x as i32, position.y as i32);
        let bounds = math::Rect::new(position, math::v2(bounds_x, bounds_y + position.y));

        let mut frame_glyphs = self.frame_glyphs.borrow_mut();
        for glyph in &glyphs {
            frame_glyphs.insert(glyph.key);
        }

        PositionedTextSpan {
            glyphs,
            bounds,
            anchor,
            shadow,
        }
    }

    pub fn fill_glyph_cache(&self, display: &GraphicsBackend) {
        let cache_size = math::v2(self.cache_width - 0, self.cache_height - 0).as_f32();

        let mut frame_glyphs = self.frame_glyphs.borrow_mut();
        let mut cache_glyphs = self.cache_glyphs.borrow_mut();

        for glyph in frame_glyphs.drain() {
            if cache_glyphs.contains_key(&glyph) {
                continue;
            }
            if let Some(font) = self.fonts.get(glyph.font_index) {
                let (metrics, data) = font.rasterize_indexed(glyph.glyph_index, glyph.px);
                if let Some(region) = self.cache_cursor.advance(metrics) {
                    display.update_texture(self.texture(), region, &data);

                    let uv = math::Rect::new(
                        region.min.as_f32() / cache_size,
                        region.max.as_f32() / cache_size,
                    );

                    let dimensions = math::Rect::new(
                        math::v2(0.0, 0.0),
                        math::v2(metrics.width as f32, metrics.height as f32),
                    );

                    cache_glyphs.insert(glyph, (uv, dimensions));
                } else {
                    log::error!("font cache full");
                    self.cache_cursor.reset();
                    cache_glyphs.clear();
                    let empty_data = vec![0; (self.cache_width * self.cache_height) as usize];
                    let region = math::Rect::new(
                        math::v2(0, 0),
                        math::v2(self.cache_width, self.cache_height),
                    );
                    display.update_texture(self.texture(), region, &empty_data);
                }
            }
        }
    }

    pub fn draw(&self, text: &PositionedTextSpan, buffer: &mut Vec<TextVertex>, ui_scale: f32) {
        let offset = text.bounds.offset(text.anchor);
        let shadow = text.shadow;

        for glyph in text.glyphs.iter() {
            if let Some((tex_coords, dimensions)) = self.cache_glyphs.borrow().get(&glyph.key) {
                let corner = math::v2(glyph.x, glyph.y) + offset.as_f32();
                let screen_coords = math::Rect::new(corner, corner + dimensions.max);

                let color = glyph.user_data;

                if shadow {
                    let positions = screen_coords.corners();
                    let uvs = tex_coords.corners();
                    let color = math::V3::fill(0.01).expand(color.w);

                    for i in 0..4 {
                        let position = positions[i];
                        let uv = uvs[i];

                        buffer.push(TextVertex {
                            position: position + (3.0 * ui_scale),
                            uv,
                            color,
                        });
                    }
                }

                let positions = screen_coords.corners();
                let uvs = tex_coords.corners();

                for i in 0..4 {
                    let position = positions[i];
                    let uv = uvs[i];

                    buffer.push(TextVertex {
                        position,
                        uv,
                        color,
                    });
                }
            }
        }
    }
}
