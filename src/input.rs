use ahash::AHashSet as HashSet;
use winit::event::{Event, MouseButton, VirtualKeyCode};
use winit::event_loop::EventLoopProxy;

use crate::gfx::UserEvent;
use crate::math;
use crate::platform::{EventReceiver, EventSender};

pub struct InputState {
    event_sender: EventSender,
    event_receiver: EventReceiver,
    closed: bool,
    text: String,
    pressed_keys: HashSet<winit::event::VirtualKeyCode>,
    released_keys: HashSet<winit::event::VirtualKeyCode>,
    mouse_wheel_delta: f32,
    window_size: math::V2<u32>,
    window_start_size: math::V2<u32>,
    mouse_position: math::V2<f32>,
    mouse_start_position: math::V2<f32>,
    pressed_mouse: HashSet<winit::event::MouseButton>,
    released_mouse: HashSet<winit::event::MouseButton>,
    user_events: Vec<UserEvent>,
}

impl InputState {
    pub fn new(
        event_sender: EventSender,
        event_receiver: EventReceiver,
        window_size: math::V2<u32>,
    ) -> InputState {
        InputState {
            event_sender,
            event_receiver,
            closed: false,
            text: String::new(),
            pressed_keys: HashSet::new(),
            released_keys: HashSet::new(),
            mouse_wheel_delta: 0.0,
            window_size,
            window_start_size: math::V2::fill(1024),
            mouse_position: math::V2::fill(0.0),
            mouse_start_position: math::V2::fill(0.0),
            pressed_mouse: HashSet::new(),
            released_mouse: HashSet::new(),
            user_events: Vec::new(),
        }
    }

    pub fn received_user_events(&mut self) -> impl Iterator<Item = UserEvent> {
        self.event_receiver.user_event_iter()
    }

    pub fn push_user_event(&mut self, event: UserEvent) {
        self.user_events.push(event);
    }

    pub fn send_user_event(&self, event: UserEvent) {
        self.event_sender.send_user_event(event);
    }

    pub fn reset(&mut self) {
        self.mouse_start_position = self.mouse_position;
        self.mouse_wheel_delta = 0.0;
        self.window_start_size = self.window_size;
        self.released_keys.clear();
        self.released_mouse.clear();
        self.text.clear();
        self.user_events.clear();
    }

    pub fn process(&mut self, event: Event<UserEvent>) {
        use winit::event::*;
        match event {
            Event::UserEvent(user_event) => self.user_events.push(user_event),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                self.closed = true;
            }
            Event::WindowEvent {
                event: WindowEvent::ReceivedCharacter(c),
                ..
            } => {
                if !c.is_control() {
                    self.text.push(c);
                }
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                state,
                                virtual_keycode: Some(key),
                                ..
                            },
                        ..
                    },
                ..
            } => match state {
                ElementState::Pressed => {
                    self.released_keys.remove(&key);
                    self.pressed_keys.insert(key);
                }
                ElementState::Released => {
                    self.pressed_keys.remove(&key);
                    self.released_keys.insert(key);
                }
            },
            Event::WindowEvent {
                event: WindowEvent::MouseWheel { delta, .. },
                ..
            } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_x, y) => y * 5.0,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                };

                self.mouse_wheel_delta += delta;
            }
            Event::WindowEvent {
                event: WindowEvent::MouseInput { state, button, .. },
                ..
            } => match state {
                ElementState::Pressed => {
                    self.released_mouse.remove(&button);
                    self.pressed_mouse.insert(button);
                }
                ElementState::Released => {
                    self.pressed_mouse.remove(&button);
                    self.released_mouse.insert(button);
                }
            },
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                let position = math::v2(position.x, position.y).as_f32();
                self.mouse_position = position;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                self.window_size = math::v2(size.width, size.height);
            }
            _ => (),
        }
    }

    pub fn window_resized(&self) -> Option<math::V2<u32>> {
        if self.window_start_size != self.window_size {
            Some(self.window_size)
        } else {
            None
        }
    }

    pub fn scroll(&self) -> f32 {
        self.mouse_wheel_delta
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn was_key_down(&self, key: VirtualKeyCode) -> bool {
        self.released_keys.contains(&key)
    }

    pub fn is_key_down(&self, key: VirtualKeyCode) -> bool {
        self.pressed_keys.contains(&key)
    }

    pub fn user_events(&self) -> impl Iterator<Item = &UserEvent> {
        self.user_events.iter()
    }

    pub fn closed(&self) -> bool {
        self.closed
    }

    pub fn mouse_move_delta(&self) -> math::V2<f32> {
        self.mouse_start_position - self.mouse_position
    }

    pub fn mouse_position(&self) -> math::V2<f32> {
        self.mouse_position
    }

    pub fn is_mouse_down(&self, button: MouseButton) -> bool {
        self.pressed_mouse.contains(&button)
    }
}

pub trait UserEventSender: Clone {
    fn send_user_event(&self, event: UserEvent);
}

pub trait UserEventReceiver {
    type Iter: Iterator<Item = UserEvent>;
    fn user_event_iter(&self) -> Self::Iter;
}

impl UserEventSender for std::sync::mpsc::Sender<UserEvent> {
    fn send_user_event(&self, event: UserEvent) {
        let _ = self.send(event);
    }
}

impl UserEventSender for EventLoopProxy<UserEvent> {
    fn send_user_event(&self, event: UserEvent) {
        let _ = self.send_event(event);
    }
}

impl UserEventReceiver for std::sync::mpsc::Receiver<UserEvent> {
    type Iter = std::vec::IntoIter<UserEvent>;
    fn user_event_iter(&self) -> Self::Iter {
        let items: Vec<UserEvent> = self.try_iter().collect();
        items.into_iter()
    }
}

impl UserEventReceiver for () {
    type Iter = std::iter::Empty<UserEvent>;
    fn user_event_iter(&self) -> Self::Iter {
        std::iter::empty()
    }
}
