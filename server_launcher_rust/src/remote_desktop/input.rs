use super::{keymap::keysym, vnc::Input};
use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, Rect, Vec2};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Capture {
    active: bool,
    held: HashMap<Key, (u32, Option<Key>)>,
    modifiers: Modifiers,
    buttons: u8,
    extra_buttons: u8,
    position: [u16; 2],
    wheel: Vec2,
}

impl Capture {
    pub(crate) fn events(
        &mut self,
        events: &[Event],
        modifiers: Modifiers,
        focused: bool,
        canvas: Rect,
        clip: Rect,
        size: [usize; 2],
        hover: Option<Pos2>,
        mut send: impl FnMut(Input),
    ) {
        if !focused {
            self.release(&mut send);
            return;
        }
        let visible = canvas.intersect(clip);
        for event in events {
            match *event {
                Event::Copy | Event::Cut | Event::Paste(_) if self.active => {
                    let key = match event {
                        Event::Copy => b'c',
                        Event::Cut => b'x',
                        Event::Paste(_) => b'v',
                        _ => unreachable!(),
                    };
                    let previous = self.modifiers;
                    self.sync_modifiers(
                        Modifiers {
                            ctrl: true,
                            ..Modifiers::NONE
                        },
                        &mut send,
                    );
                    for pressed in [true, false] {
                        send(Input::Key {
                            keysym: u32::from(key),
                            physical_key: None,
                            pressed,
                        });
                    }
                    self.sync_modifiers(previous, &mut send);
                }
                Event::WindowFocused(false) => self.release(&mut send),
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } => {
                    if pressed && !visible.contains(pos) {
                        self.release(&mut send);
                    } else if let Some(button) = extra_mouse_button(button) {
                        if pressed {
                            self.active = true;
                            self.extra_buttons |= button;
                        } else if self.extra_buttons & button != 0 {
                            self.extra_buttons &= !button;
                        } else {
                            continue;
                        }
                        self.position = remote_position(pos, canvas, size);
                        send(Input::PointerExtra {
                            x: self.position[0],
                            y: self.position[1],
                            button,
                            pressed,
                        });
                    } else if let Some(mask) = mouse_button(button) {
                        if pressed {
                            self.active = true;
                            self.buttons |= mask;
                        } else if self.buttons & mask != 0 {
                            self.buttons &= !mask;
                        } else {
                            continue;
                        }
                        self.position = remote_position(pos, canvas, size);
                        send(self.pointer(self.buttons));
                    }
                }
                Event::PointerMoved(pos)
                    if visible.contains(pos) || self.buttons != 0 || self.extra_buttons != 0 =>
                {
                    self.position = remote_position(pos, canvas, size);
                    send(self.pointer(self.buttons));
                }
                Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } if self.active => {
                    self.sync_modifiers(modifiers, &mut send);
                    if pressed && !repeat {
                        if let Some(keysym) = keysym(key) {
                            self.held.insert(key, (keysym, physical_key));
                            send(Input::Key {
                                keysym,
                                physical_key,
                                pressed: true,
                            });
                        }
                    } else if !pressed {
                        if let Some((keysym, physical_key)) = self.held.remove(&key) {
                            send(Input::Key {
                                keysym,
                                physical_key,
                                pressed: false,
                            });
                        }
                    }
                }
                Event::MouseWheel { unit, delta, .. }
                    if self.active && hover.is_some_and(|pos| visible.contains(pos)) =>
                {
                    let multiplier = match unit {
                        MouseWheelUnit::Point => 1.0 / 40.0,
                        MouseWheelUnit::Line | MouseWheelUnit::Page => 1.0,
                    };
                    self.wheel += delta * multiplier;
                    let steps = [self.wheel.x.trunc(), self.wheel.y.trunc()];
                    self.wheel -= egui::vec2(steps[0], steps[1]);
                    for (step, positive, negative) in
                        [(steps[0], 1 << 5, 1 << 6), (steps[1], 1 << 3, 1 << 4)]
                    {
                        let bit = if step > 0.0 { positive } else { negative };
                        for _ in 0..(step.abs() as u8).min(16) {
                            send(self.pointer(self.buttons | bit));
                            send(self.pointer(self.buttons));
                        }
                    }
                }
                _ => {}
            }
        }
        if self.active {
            self.sync_modifiers(modifiers, &mut send);
        }
    }

    pub(crate) fn release(&mut self, mut send: impl FnMut(Input)) {
        self.active = false;
        for (_, (keysym, physical_key)) in self.held.drain() {
            send(Input::Key {
                keysym,
                physical_key,
                pressed: false,
            });
        }
        for (button, bit) in [(1, 1), (2, 2)] {
            if self.extra_buttons & bit != 0 {
                send(Input::PointerExtra {
                    x: self.position[0],
                    y: self.position[1],
                    button,
                    pressed: false,
                });
            }
        }
        self.extra_buttons = 0;
        self.sync_modifiers(Modifiers::NONE, &mut send);
        if self.buttons != 0 {
            self.buttons = 0;
            send(self.pointer(0));
        }
        self.wheel = Vec2::ZERO;
    }

    fn sync_modifiers(&mut self, next: Modifiers, mut send: impl FnMut(Input)) {
        let previous = self.modifiers;
        for (keysym, old, pressed) in [
            (0xffe1, previous.shift, next.shift),
            (0xffe3, previous.ctrl, next.ctrl),
            (0xffe9, previous.alt, next.alt),
            (0xffeb, previous.mac_cmd, next.mac_cmd),
        ] {
            if old != pressed {
                send(Input::Key {
                    keysym,
                    physical_key: None,
                    pressed,
                });
            }
        }
        self.modifiers = next;
    }

    fn pointer(&self, buttons: u8) -> Input {
        Input::Pointer {
            x: self.position[0],
            y: self.position[1],
            buttons,
        }
    }
}

fn mouse_button(button: PointerButton) -> Option<u8> {
    match button {
        PointerButton::Primary => Some(1),
        PointerButton::Middle => Some(2),
        PointerButton::Secondary => Some(4),
        PointerButton::Extra1 | PointerButton::Extra2 => None,
    }
}

fn extra_mouse_button(button: PointerButton) -> Option<u8> {
    match button {
        PointerButton::Extra1 => Some(1),
        PointerButton::Extra2 => Some(2),
        _ => None,
    }
}

fn remote_position(pos: Pos2, canvas: Rect, [width, height]: [usize; 2]) -> [u16; 2] {
    let coordinate = |value: f32, span: f32, pixels: usize| {
        ((value / span) * pixels as f32).clamp(0.0, pixels.saturating_sub(1) as f32) as u16
    };
    [
        coordinate(pos.x - canvas.left(), canvas.width(), width),
        coordinate(pos.y - canvas.top(), canvas.height(), height),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_drag_coordinates_and_releases_buttons_and_modifiers() {
        let rect = Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
        let mut capture = Capture::default();
        let mut packets = Vec::new();
        capture.events(
            &[
                Event::PointerButton {
                    pos: rect.center(),
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                Event::PointerMoved(egui::pos2(300.0, 90.0)),
            ],
            Modifiers::CTRL,
            true,
            rect,
            rect,
            [200, 100],
            None,
            |event| packets.push(event),
        );
        capture.release(|event| packets.push(event));
        assert_eq!(
            packets,
            [
                Input::Pointer {
                    x: 100,
                    y: 50,
                    buttons: 1
                },
                Input::Pointer {
                    x: 199,
                    y: 99,
                    buttons: 1
                },
                Input::Key {
                    keysym: 0xffe3,
                    physical_key: None,
                    pressed: true
                },
                Input::Key {
                    keysym: 0xffe3,
                    physical_key: None,
                    pressed: false
                },
                Input::Pointer {
                    x: 199,
                    y: 99,
                    buttons: 0
                },
            ]
        );
    }

    #[test]
    fn releases_extra_mouse_buttons_when_capture_ends() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 50.0));
        let mut capture = Capture::default();
        let mut packets = Vec::new();
        capture.events(
            &[Event::PointerButton {
                pos: rect.center(),
                button: PointerButton::Extra1,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
            true,
            rect,
            rect,
            [200, 100],
            None,
            |event| packets.push(event),
        );
        capture.release(|event| packets.push(event));
        assert_eq!(
            packets,
            [
                Input::PointerExtra {
                    x: 100,
                    y: 50,
                    button: 1,
                    pressed: true,
                },
                Input::PointerExtra {
                    x: 100,
                    y: 50,
                    button: 1,
                    pressed: false,
                },
            ]
        );
    }
}
