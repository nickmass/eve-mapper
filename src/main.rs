mod cache;
mod error;
mod esi;
mod font;
mod gfx;
mod math;
mod oauth;
mod platform;
mod world;

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let window = gfx::Window::new(1024, 1024);
    window.run();
}
