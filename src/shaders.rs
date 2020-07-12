use notify::Watcher;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use std::path::PathBuf;

pub struct ShaderCollection {
    pub version: Arc<AtomicUsize>,
    shaders: Arc<Mutex<HashMap<&'static str, Shader>>>,
    pub closed: Arc<AtomicBool>,
    watcher: notify::RecommendedWatcher,
    update_thread: Option<std::thread::JoinHandle<()>>,
}

pub struct Shader {
    pub source: String,
    pub version: usize,
    pub path: String,
}

impl ShaderCollection {
    pub fn new() -> ShaderCollection {
        let mut shaders = HashMap::new();

        shaders.insert(
            "systems_vert",
            Shader {
                source: String::from_utf8(include_bytes!("../shaders/systems_vert.glsl").to_vec())
                    .unwrap(),
                version: 0,
                path: String::from("shaders/systems_vert.glsl"),
            },
        );
        shaders.insert(
            "systems_frag",
            Shader {
                source: String::from_utf8(include_bytes!("../shaders/systems_frag.glsl").to_vec())
                    .unwrap(),
                version: 0,
                path: String::from("shaders/systems_frag.glsl"),
            },
        );
        shaders.insert(
            "jumps_vert",
            Shader {
                source: String::from_utf8(include_bytes!("../shaders/jumps_vert.glsl").to_vec())
                    .unwrap(),
                version: 0,
                path: String::from("shaders/jumps_vert.glsl"),
            },
        );
        shaders.insert(
            "jumps_frag",
            Shader {
                source: String::from_utf8(include_bytes!("../shaders/jumps_frag.glsl").to_vec())
                    .unwrap(),
                version: 0,
                path: String::from("shaders/jumps_frag.glsl"),
            },
        );
        shaders.insert(
            "text_vert",
            Shader {
                source: String::from_utf8(include_bytes!("../shaders/text_vert.glsl").to_vec())
                    .unwrap(),
                version: 0,
                path: String::from("shaders/text_vert.glsl"),
            },
        );
        shaders.insert(
            "text_frag",
            Shader {
                source: String::from_utf8(include_bytes!("../shaders/text_frag.glsl").to_vec())
                    .unwrap(),
                version: 0,
                path: String::from("shaders/text_frag.glsl"),
            },
        );

        let (tx, rx) = channel();
        let mut watcher = notify::watcher(tx, std::time::Duration::from_millis(100)).unwrap();

        for shader in shaders.values() {
            watcher
                .watch(&shader.path, notify::RecursiveMode::NonRecursive)
                .unwrap();
        }

        let shaders = Arc::new(Mutex::new(shaders));
        let closed = Arc::new(AtomicBool::new(false));
        let version = Arc::new(AtomicUsize::new(0));

        let update_thread = Some(std::thread::spawn({
            let shaders = shaders.clone();
            let closed = closed.clone();
            let version = version.clone();
            move || {
                while !closed.load(Ordering::Relaxed) {
                    use notify::DebouncedEvent;
                    match rx.try_recv() {
                        Ok(event) => match event {
                            DebouncedEvent::Write(path) | DebouncedEvent::Create(path) => {
                                let mut shaders = shaders.lock().unwrap();

                                for shader in shaders.values_mut() {
                                    let shader_path = PathBuf::from(&shader.path);
                                    if path.file_name() == shader_path.file_name() {
                                        let new_source = std::fs::read_to_string(path).unwrap();
                                        shader.source = new_source;
                                        shader.version += 1;
                                        log::info!(
                                            "updated shader source of: {}",
                                            shader_path.display(),
                                        );
                                        version.fetch_add(1, Ordering::Relaxed);
                                        break;
                                    }
                                }
                            }
                            _ => (),
                        },
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            std::thread::sleep(std::time::Duration::from_millis(50))
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            log::error!("shader update thread disconnected");
                            return;
                        }
                    }
                }
            }
        }));

        ShaderCollection {
            shaders,
            closed,
            version,
            watcher,
            update_thread,
        }
    }

    pub fn version(&self) -> usize {
        self.version.load(Ordering::Relaxed)
    }

    pub fn get(&self, name: &'static str) -> Option<String> {
        self.shaders
            .lock()
            .unwrap()
            .get(name)
            .map(|s| s.source.clone())
    }

    pub fn get_version(&self, name: &'static str) -> Option<usize> {
        self.shaders
            .lock()
            .unwrap()
            .get(name)
            .map(|s| s.version.clone())
    }
}

impl std::ops::Drop for ShaderCollection {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(thread) = self.update_thread.take() {
            thread.join().unwrap();
        }
    }
}
