use std::sync::{Mutex, RwLock};

use super::QuadVertex;
use crate::math;
use crate::platform::{GraphicsBackend, SrgbTexture, U8U8U8U8};

use ahash::AHashMap as HashMap;

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum Image {
    AllianceLogo(i32),
}

pub struct Images {
    cache_width: u32,
    cache_height: u32,
    cache_texture: SrgbTexture<U8U8U8U8>,
    slots: RwLock<HashMap<Image, math::Rect<u32>>>,
    cursor: Mutex<math::V2<u32>>,
}

impl Images {
    pub fn new(display: &GraphicsBackend, cache_width: u32, cache_height: u32) -> Self {
        let cache_texture = display.create_texture(cache_width, cache_height);

        Images {
            cache_width,
            cache_height,
            cache_texture,
            slots: RwLock::new(HashMap::new()),
            cursor: Mutex::new(math::V2::fill(0)),
        }
    }

    pub fn texture(&self) -> &SrgbTexture<U8U8U8U8> {
        &self.cache_texture
    }

    pub fn contains(&self, image: Image) -> bool {
        self.slots.read().unwrap().contains_key(&image)
    }

    pub fn load(
        &self,
        display: &GraphicsBackend,
        image: Image,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.contains(image) {
            return Ok(());
        }

        let mut decoder = png::Decoder::new(data);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);

        let (info, mut reader) = decoder.read_info()?;
        let (width, height) = (info.width, info.height);

        let mut buf = vec![0; reader.output_buffer_size()];

        reader.next_frame(&mut buf)?;

        let image_data: Vec<u8> = match info.color_type {
            png::ColorType::Grayscale => {
                let mut data = Vec::with_capacity(buf.len() * 4);
                for b in buf {
                    data.push(b);
                    data.push(b);
                    data.push(b);
                    data.push(0xff);
                }
                data
            }
            png::ColorType::RGB => {
                let mut data = Vec::with_capacity((buf.len() / 3) * 4);
                for c in buf.chunks(3) {
                    data.push(c[0]);
                    data.push(c[1]);
                    data.push(c[2]);
                    data.push(0xff);
                }
                data
            }
            png::ColorType::Indexed => Err("indexed")?,
            png::ColorType::GrayscaleAlpha => {
                let mut data = Vec::with_capacity((buf.len() / 2) * 4);
                for c in buf.chunks(2) {
                    data.push(c[0]);
                    data.push(c[0]);
                    data.push(c[0]);
                    data.push(c[1]);
                }
                data
            }
            png::ColorType::RGBA => buf,
        };

        let mut cursor = self.cursor.lock().unwrap();
        if cursor.x + width > self.cache_width {
            if cursor.y + height > self.cache_height {
                Err("cache full")?;
            } else {
                cursor.x = 0;
                cursor.y += height;
            }
        }

        {
            let cursor = cursor.clone();
            display.update_texture(
                self.texture(),
                math::Rect::new(cursor, cursor + math::v2(width, height)),
                &image_data,
            );

            let mut slots = self.slots.write().unwrap();
            slots.insert(
                image,
                math::Rect::new(cursor.clone(), cursor.clone() + math::v2(width, height)),
            );
        }

        cursor.x += width;

        Ok(())
    }

    pub fn draw(&self, vertex_buf: &mut Vec<QuadVertex>, image: Image, position: math::Rect<f32>) {
        if let Some(uv_rect) = self.slots.read().unwrap().get(&image).cloned() {
            for (position, uv) in position
                .triangle_list_iter()
                .zip(uv_rect.triangle_list_iter())
            {
                vertex_buf.push(QuadVertex {
                    position,
                    uv: uv.as_f32() / math::v2(self.cache_width, self.cache_height).as_f32(),
                });
            }
        }
    }
}
