use glium::glutin;
use glium::texture::{SrgbTexture2d, Texture2d};
use glium::{Display, Surface};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowBuilder;

use std::cell::{Cell, RefCell};
use std::convert::TryInto;

use crate::gfx::font::{FontCache, PositionedTextSpan};
use crate::gfx::images::{Image, Images};
use crate::gfx::{CircleVertex, LineVertex, QuadVertex, SystemData, TextVertex, UserEvent};
use crate::math;

mod shaders;
use shaders::*;

pub use async_std::task::spawn;

pub use std::time;

pub use async_std::fs::{read as read_file, write as write_file};

pub const ESI_IMAGE_SERVER: &'static str = "https://images.evetech.net/";
pub const USER_AGENT: Option<&'static str> =
    Some("EveMapper-Development v0.01: nickmass@nickmass.com");

pub fn file_exists<P: AsRef<std::path::Path>>(path: P) -> bool {
    std::path::Path::exists(path.as_ref())
}

pub type EventSender = EventLoopProxy<UserEvent>;
pub type EventReceiver = ();

pub fn create_event_proxy(event_loop: &EventLoop<UserEvent>) -> (EventSender, EventReceiver) {
    (event_loop.create_proxy(), ())
}

pub const DEFAULT_CONTROL_FLOW: ControlFlow = ControlFlow::Wait;

pub fn parse_http_date(s: &str) -> Option<time::SystemTime> {
    httpdate::parse_http_date(s).ok()
}

pub struct GraphicsBackend {
    display: Display,
    window_size: Cell<math::V2<f32>>,
    text_buffer: RefCell<Vec<TextVertex>>,
    system_program: RefCell<Option<Shader<SystemsShader>>>,
    jump_program: RefCell<Option<Shader<JumpsShader>>>,
    text_program: RefCell<Option<Shader<TextShader>>>,
    quad_program: RefCell<Option<Shader<QuadShader>>>,
    quad_indices: RefCell<Vec<u32>>,
    quad_index_buffer: RefCell<Option<glium::IndexBuffer<u32>>>,
    blend_draw_params: glium::DrawParameters<'static>,
    depth_blend_draw_params: glium::DrawParameters<'static>,
    shader_collection: RefCell<shaders::ShaderCollection>,
}

impl GraphicsBackend {
    pub fn new(
        window_builder: WindowBuilder,
        event_loop: &EventLoop<UserEvent>,
        width: u32,
        height: u32,
    ) -> GraphicsBackend {
        let context_builder = glutin::ContextBuilder::new()
            .with_vsync(true)
            .with_srgb(true)
            .with_gl_profile(glutin::GlProfile::Core)
            .with_gl(glutin::GlRequest::Specific(glutin::Api::OpenGl, (4, 2)));

        let display = glium::Display::new(window_builder, context_builder, &event_loop).unwrap();

        let window_size = Cell::new(math::V2::new(width, height).as_f32());

        let shader_collection = shaders::ShaderCollection::new("shaders/");

        let system_program = RefCell::new(None);
        let jump_program = RefCell::new(None);
        let text_program = RefCell::new(None);
        let quad_program = RefCell::new(None);

        let blend = glium::Blend {
            color: glium::BlendingFunction::Addition {
                source: glium::LinearBlendingFactor::SourceAlpha,
                destination: glium::LinearBlendingFactor::OneMinusSourceAlpha,
            },
            alpha: glium::BlendingFunction::Addition {
                source: glium::LinearBlendingFactor::Zero,
                destination: glium::LinearBlendingFactor::One,
            },
            constant_value: (1.0, 1.0, 1.0, 1.0),
        };

        let blend_draw_params = glium::DrawParameters {
            blend,
            ..Default::default()
        };

        let depth_blend_draw_params = glium::DrawParameters {
            blend,
            depth: glium::Depth {
                test: glium::DepthTest::IfMoreOrEqual,
                write: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let quad_indices = RefCell::new(Vec::new());
        let quad_index_buffer = RefCell::new(None);

        GraphicsBackend {
            display,
            window_size,
            text_buffer: RefCell::new(Vec::new()),
            text_program,
            quad_program,
            quad_indices,
            quad_index_buffer,
            system_program,
            jump_program,
            blend_draw_params,
            depth_blend_draw_params,
            shader_collection: RefCell::new(shader_collection),
        }
    }

    pub fn request_redraw(&self) {
        self.display.gl_window().window().request_redraw();
    }

    pub fn create_texture<T: Texture>(&self, width: u32, height: u32) -> T {
        T::create(&self.display, width, height)
    }

    pub fn fill_buffer<T: glium::Vertex>(&self, buffer: &[T]) -> Buffer<T> {
        let buffer = glium::VertexBuffer::new(&self.display, buffer)
            .expect("unable to create vertex buffer");
        Buffer { buffer }
    }

    pub fn update_texture<T: Texture>(&self, texture: &T, region: math::Rect<u32>, data: &[u8]) {
        texture.update(region, data);
    }

    pub fn window_size(&self) -> math::V2<f32> {
        let size = self.display.gl_window().window().inner_size();
        math::v2(size.width, size.height).as_f32()
    }

    pub fn update_window_size(&self, window_size: math::V2<f32>) {
        self.window_size.set(window_size);
    }

    pub fn begin(&self) -> Frame {
        let mut shader_collection = self.shader_collection.borrow_mut();
        shader_collection.load_if_newer(&self.display, &mut self.system_program.borrow_mut());
        shader_collection.load_if_newer(&self.display, &mut self.jump_program.borrow_mut());
        shader_collection.load_if_newer(&self.display, &mut self.text_program.borrow_mut());
        shader_collection.load_if_newer(&self.display, &mut self.quad_program.borrow_mut());

        Frame {
            frame: self.display.draw(),
        }
    }

    pub fn end(&self, frame: Frame) {
        let res = frame.frame.finish();
        if let Err(error) = res {
            log::error!("frame finish error: {:?}", error);
        }
    }

    fn fill_quad_indices(&self, num_vertexes: usize) -> usize {
        let mut quad_indices = self.quad_indices.borrow_mut();
        let mut quad_index_buffer = self.quad_index_buffer.borrow_mut();
        let end = num_vertexes / 4;
        let start = quad_indices.len() / 6;
        let num_indices = end * 6;

        if quad_index_buffer.is_some() && start >= end {
            return num_indices;
        }

        if start < end {
            quad_indices.reserve(end - start);

            let start: u32 = start.try_into().expect("overflowed quad index buffer");
            let end: u32 = end.try_into().expect("overflowed quad index buffer");

            for n in start..end {
                quad_indices.push(n * 4);
                quad_indices.push(n * 4 + 1);
                quad_indices.push(n * 4 + 2);
                quad_indices.push(n * 4 + 1);
                quad_indices.push(n * 4 + 2);
                quad_indices.push(n * 4 + 3);
            }
        }

        let buffer = glium::IndexBuffer::new(
            &self.display,
            glium::index::PrimitiveType::TrianglesList,
            &quad_indices,
        )
        .expect("unable to create quad index buffer");
        *quad_index_buffer = Some(buffer);

        num_indices
    }

    pub fn draw_system(
        &self,
        frame: &mut Frame,
        circle_buffer: &Buffer<CircleVertex>,
        system_data: &Buffer<SystemData>,
        zoom: f32,
        scale_matrix: math::M3<f32>,
        view_matrix: math::M3<f32>,
    ) {
        if system_data.buffer.len() == 0 {
            return;
        }

        let uniforms = glium::uniform! {
            map_scale_matrix: scale_matrix,
            map_view_matrix: view_matrix,
            zoom: zoom
        };

        let draw_res = frame.frame.draw(
            (
                &circle_buffer.buffer,
                system_data.buffer.per_instance().unwrap(),
            ),
            &glium::index::NoIndices(glium::index::PrimitiveType::TriangleFan),
            &self.system_program.borrow().as_ref().unwrap(),
            &uniforms,
            &self.blend_draw_params,
        );

        if let Err(error) = draw_res {
            log::error!("system draw error: {:?}", error);
        }
    }

    pub fn draw_jump(
        &self,
        frame: &mut Frame,
        jump_buffer: &Buffer<LineVertex>,
        zoom: f32,
        scale_matrix: math::M3<f32>,
        view_matrix: math::M3<f32>,
    ) {
        if jump_buffer.buffer.len() == 0 {
            return;
        }

        let uniforms = glium::uniform! {
            map_scale_matrix: scale_matrix,
            map_view_matrix: view_matrix,
            zoom: zoom
        };

        let end = self.fill_quad_indices(jump_buffer.buffer.len());

        let draw_res = frame.frame.draw(
            &jump_buffer.buffer,
            self.quad_index_buffer
                .borrow()
                .as_ref()
                .unwrap()
                .slice(0..end)
                .expect("index buffer incorrect length"),
            &self.jump_program.borrow().as_ref().unwrap(),
            &uniforms,
            &self.depth_blend_draw_params,
        );

        if let Err(error) = draw_res {
            log::error!("jump draw error: {:?}", error);
        }
    }

    pub fn draw_text(
        &self,
        frame: &mut Frame,
        font_cache: &FontCache,
        text: &[PositionedTextSpan],
        ui_scale: f32,
    ) {
        if text.len() == 0 {
            return;
        }

        let uniforms = glium::uniform! {
            window_size: self.window_size.get(),
            font_atlas: font_cache.texture().texture
            .sampled()
            .magnify_filter(glium::uniforms::MagnifySamplerFilter::Nearest)
            .minify_filter(glium::uniforms::MinifySamplerFilter::Nearest)
        };

        let mut text_buf = self.text_buffer.borrow_mut();
        text_buf.clear();

        for text in text {
            font_cache.draw(text, &mut text_buf, ui_scale);
        }

        let end = self.fill_quad_indices(text_buf.len());

        let text_data_buf = glium::VertexBuffer::new(&self.display, &text_buf)
            .expect("unable to create font vertex buffer");

        let draw_res = frame.frame.draw(
            &text_data_buf,
            self.quad_index_buffer
                .borrow()
                .as_ref()
                .unwrap()
                .slice(0..end)
                .expect("index buffer incorrect length"),
            &self.text_program.borrow().as_ref().unwrap(),
            &uniforms,
            &self.blend_draw_params,
        );

        if let Err(error) = draw_res {
            log::error!("text draw error: {:?}", error);
        }
    }

    pub fn draw_image(
        &self,
        frame: &mut Frame,
        images: &Images,
        image: Image,
        position: math::Rect<f32>,
    ) {
        let uniforms = glium::uniform! {
            window_size: self.window_size.get(),
            texture_atlas: images.texture().texture
            .sampled()
            .magnify_filter(glium::uniforms::MagnifySamplerFilter::Linear)
            .minify_filter(glium::uniforms::MinifySamplerFilter::Linear),
            textured: true,
            color: math::V4::fill(1.0)
        };

        let mut image_buf = Vec::new();
        images.draw(&mut image_buf, image, position);

        let data_buf = glium::VertexBuffer::new(&self.display, &image_buf)
            .expect("unable to create quad vertex buffer");

        let draw_res = frame.frame.draw(
            &data_buf,
            &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
            &self.quad_program.borrow().as_ref().unwrap(),
            &uniforms,
            &self.blend_draw_params,
        );

        if let Err(error) = draw_res {
            log::error!("image draw error: {:?}", error);
        }
    }

    pub fn draw_quad(
        &self,
        frame: &mut Frame,
        images: &Images,
        color: math::V4<f32>,
        position: math::Rect<f32>,
    ) {
        let uniforms = glium::uniform! {
            window_size: self.window_size.get(),
            texture_atlas: images.texture().texture
            .sampled()
            .magnify_filter(glium::uniforms::MagnifySamplerFilter::Linear)
            .minify_filter(glium::uniforms::MinifySamplerFilter::Linear),
            textured: false,
            color: color
        };

        let mut rect_buf = Vec::new();
        for v in position.triangle_list_iter() {
            rect_buf.push(QuadVertex {
                position: v,
                uv: math::v2(0.0, 0.0),
            })
        }

        let data_buf = glium::VertexBuffer::new(&self.display, &rect_buf)
            .expect("unable to create quad vertex buffer");

        let draw_res = frame.frame.draw(
            &data_buf,
            &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
            &self.quad_program.borrow().as_ref().unwrap(),
            &uniforms,
            &self.blend_draw_params,
        );

        if let Err(error) = draw_res {
            log::error!("quad draw error: {:?}", error);
        }
    }
}

pub struct Frame {
    frame: glium::Frame,
}

impl Frame {
    pub fn clear_color(&mut self, color: math::V4<f32>) {
        self.frame.clear_color(color.x, color.y, color.z, color.w);
    }

    pub fn clear_depth(&mut self, value: f32) {
        self.frame.clear_depth(value);
    }
}

pub trait Texture {
    fn create(display: &Display, width: u32, height: u32) -> Self;
    fn update(&self, region: math::Rect<u32>, data: &[u8]);
}

pub struct RgbTexture<T: TextureFormat> {
    texture: Texture2d,
    marker: std::marker::PhantomData<T>,
}

impl<T: TextureFormat> Texture for RgbTexture<T> {
    fn create(display: &Display, width: u32, height: u32) -> Self {
        RgbTexture {
            texture: Texture2d::empty(display, width, height).expect("unable to create texture"),
            marker: Default::default(),
        }
    }

    fn update(&self, region: math::Rect<u32>, data: &[u8]) {
        let rect = glium::Rect {
            left: region.min.x,
            bottom: region.min.y,
            width: region.width(),
            height: region.height(),
        };

        let img_data = glium::texture::RawImage2d {
            data: data.into(),
            width: rect.width,
            height: rect.height,
            format: T::FORMAT,
        };
        self.texture.write(rect, img_data);
    }
}

pub struct SrgbTexture<T: TextureFormat> {
    texture: SrgbTexture2d,
    marker: std::marker::PhantomData<T>,
}

impl<T: TextureFormat> Texture for SrgbTexture<T> {
    fn create(display: &Display, width: u32, height: u32) -> Self {
        SrgbTexture {
            texture: SrgbTexture2d::empty(display, width, height)
                .expect("unable to create texture"),
            marker: Default::default(),
        }
    }

    fn update(&self, region: math::Rect<u32>, data: &[u8]) {
        let rect = glium::Rect {
            left: region.min.x,
            bottom: region.min.y,
            width: region.width(),
            height: region.height(),
        };

        let img_data = glium::texture::RawImage2d {
            data: data.into(),
            width: rect.width,
            height: rect.height,
            format: T::FORMAT,
        };
        self.texture.write(rect, img_data);
    }
}

pub struct U8;

impl TextureFormat for U8 {
    const FORMAT: glium::texture::ClientFormat = glium::texture::ClientFormat::U8;
}

pub struct U8U8U8U8;

impl TextureFormat for U8U8U8U8 {
    const FORMAT: glium::texture::ClientFormat = glium::texture::ClientFormat::U8U8U8U8;
}

pub trait TextureFormat {
    const FORMAT: glium::texture::ClientFormat;
}

pub struct Buffer<T: Copy> {
    buffer: glium::VertexBuffer<T>,
}

glium::implement_vertex!(CircleVertex, position);

glium::implement_vertex!(LineVertex, position, normal, color);

glium::implement_vertex!(SystemData, color, highlight, center, scale, radius);

glium::implement_vertex!(QuadVertex, position, uv);

glium::implement_vertex!(TextVertex, position, uv, color);

unsafe impl glium::vertex::Attribute for math::V2<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32F32
    }
}

unsafe impl glium::vertex::Attribute for math::V3<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32F32F32
    }
}

unsafe impl glium::vertex::Attribute for math::V4<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32F32F32F32
    }
}

unsafe impl glium::vertex::Attribute for math::M3<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32x3x3
    }
}

unsafe impl glium::vertex::Attribute for math::M4<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32x4x4
    }
}

impl glium::uniforms::AsUniformValue for math::V2<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Vec2([self.x, self.y])
    }
}

impl glium::uniforms::AsUniformValue for math::V3<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Vec3([self.x, self.y, self.z])
    }
}

impl glium::uniforms::AsUniformValue for math::V4<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Vec4([self.x, self.y, self.z, self.w])
    }
}

impl glium::uniforms::AsUniformValue for math::M3<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Mat3([
            [self.c0.x, self.c0.y, self.c0.z],
            [self.c1.x, self.c1.y, self.c1.z],
            [self.c2.x, self.c2.y, self.c2.z],
        ])
    }
}

impl glium::uniforms::AsUniformValue for math::M4<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Mat4([
            [self.c0.x, self.c0.y, self.c0.z, self.c0.w],
            [self.c1.x, self.c1.y, self.c1.z, self.c1.w],
            [self.c2.x, self.c2.y, self.c2.z, self.c2.w],
            [self.c3.x, self.c3.y, self.c3.z, self.c3.w],
        ])
    }
}
