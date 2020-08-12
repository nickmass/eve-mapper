mod cache;
mod error;
mod esi;
mod font;
mod gfx;
mod math;
mod oauth;
mod platform;
mod world;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    ConsoleLogger::initialize();

    let window = gfx::Window::new(1920, 1080);
    window.run();
}

#[cfg(target_arch = "wasm32")]
struct ConsoleLogger;

#[cfg(target_arch = "wasm32")]
impl ConsoleLogger {
    pub fn initialize() {
        let _ = log::set_logger(&ConsoleLogger).unwrap();
        log::set_max_level(log::LevelFilter::max());
    }
}

#[cfg(target_arch = "wasm32")]
impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let level = metadata.level();
        cfg!(debug_assertions)
            || (level == log::Level::Error)
            || (level == log::Level::Warn)
            || (level == log::Level::Info)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = record.level();
        let msg = JsValue::from_str(&format!("{}", record.args()));
        match level {
            log::Level::Error => web_sys::console::error_1(&msg),
            log::Level::Warn => web_sys::console::warn_1(&msg),
            log::Level::Info => web_sys::console::info_1(&msg),
            log::Level::Debug | log::Level::Trace => web_sys::console::debug_1(&msg),
        }
    }

    fn flush(&self) {}
}
