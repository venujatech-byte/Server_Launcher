use super::vnc::Input;
use ironrdp::pdu::input::{
    fast_path::{FastPathInputEvent, KeyboardFlags},
    mouse::{MousePdu, PointerFlags},
    mouse_x::{MouseXPdu, PointerXFlags},
};

#[derive(Default)]
pub(crate) struct Encoder {
    buttons: u8,
    position: Option<(u16, u16)>,
}

impl Encoder {
    pub(crate) fn encode(&mut self, input: Input, out: &mut Vec<FastPathInputEvent>) {
        match input {
            Input::VncResize { .. } | Input::ClipboardText(_) => {}
            Input::PointerExtra {
                x,
                y,
                button,
                pressed,
            } => {
                let flag = match button {
                    1 => PointerXFlags::BUTTON1,
                    2 => PointerXFlags::BUTTON2,
                    _ => return,
                };
                out.push(FastPathInputEvent::MouseEventEx(MouseXPdu {
                    flags: flag
                        | if pressed {
                            PointerXFlags::DOWN
                        } else {
                            PointerXFlags::empty()
                        },
                    x_position: x,
                    y_position: y,
                }));
            }
            Input::Key { keysym, pressed } => {
                if let Some((code, extended)) = scancode(keysym) {
                    let mut flags = KeyboardFlags::empty();
                    flags.set(KeyboardFlags::RELEASE, !pressed);
                    flags.set(KeyboardFlags::EXTENDED, extended);
                    out.push(FastPathInputEvent::KeyboardEvent(flags, code));
                }
            }
            Input::Pointer { x, y, buttons } => {
                let mut add = |flags, units| {
                    out.push(FastPathInputEvent::MouseEvent(MousePdu {
                        flags,
                        number_of_wheel_rotation_units: units,
                        x_position: x,
                        y_position: y,
                    }));
                };
                if self.position.replace((x, y)) != Some((x, y)) {
                    add(PointerFlags::MOVE, 0);
                }
                let changed = buttons ^ self.buttons;
                for (bit, flags) in [
                    (1, PointerFlags::LEFT_BUTTON),
                    (2, PointerFlags::MIDDLE_BUTTON_OR_WHEEL),
                    (4, PointerFlags::RIGHT_BUTTON),
                ] {
                    if changed & bit != 0 {
                        add(
                            flags
                                | if buttons & bit != 0 {
                                    PointerFlags::DOWN
                                } else {
                                    PointerFlags::empty()
                                },
                            0,
                        );
                    }
                }
                for (bit, flags, units) in [
                    (8, PointerFlags::VERTICAL_WHEEL, 120),
                    (16, PointerFlags::VERTICAL_WHEEL, -120),
                    (32, PointerFlags::HORIZONTAL_WHEEL, -120),
                    (64, PointerFlags::HORIZONTAL_WHEEL, 120),
                ] {
                    if changed & buttons & bit != 0 {
                        add(flags, units);
                    }
                }
                self.buttons = buttons;
            }
        }
    }
}

fn scancode(keysym: u32) -> Option<(u8, bool)> {
    let result = match keysym {
        0xff61 => (0x37, true),
        0xff08 => (0x0e, false),
        0xff09 => (0x0f, false),
        0xff0d => (0x1c, false),
        0xff1b => (0x01, false),
        0xff50 => (0x47, true),
        0xff51 => (0x4b, true),
        0xff52 => (0x48, true),
        0xff53 => (0x4d, true),
        0xff54 => (0x50, true),
        0xff55 => (0x49, true),
        0xff56 => (0x51, true),
        0xff57 => (0x4f, true),
        0xff63 => (0x52, true),
        0xffff => (0x53, true),
        0xffe1 => (0x2a, false),
        0xffe3 => (0x1d, false),
        0xffe9 => (0x38, false),
        0xffeb => (0x5b, true),
        0xffbe..=0xffc7 => ((keysym - 0xffbe) as u8 + 0x3b, false),
        0xffc8 => (0x57, false),
        0xffc9 => (0x58, false),
        32 => (0x39, false),
        _ => {
            let ascii = u8::try_from(keysym).ok()?;
            for (keys, shifted, origin) in [
                (&b"1234567890-="[..], &b"!@#$%^&*()_+"[..], 0x02),
                (b"qwertyuiop[]", b"QWERTYUIOP{}", 0x10),
                (b"asdfghjkl;'\x60", b"ASDFGHJKL:\"~", 0x1e),
                (b"zxcvbnm,./", b"ZXCVBNM<>?", 0x2c),
            ] {
                if let Some(index) = keys
                    .iter()
                    .zip(shifted)
                    .position(|(&base, &upper)| ascii == base || ascii == upper)
                {
                    return Some((origin + index as u8, false));
                }
            }
            match ascii {
                b'\\' | b'|' => (0x2b, false),
                _ => return None,
            }
        }
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_extended_key_release_and_button_release() {
        let mut encoder = Encoder::default();
        let mut out = Vec::new();
        encoder.encode(
            Input::Key {
                keysym: 0xff51,
                pressed: false,
            },
            &mut out,
        );
        assert_eq!(
            out,
            [FastPathInputEvent::KeyboardEvent(
                KeyboardFlags::EXTENDED | KeyboardFlags::RELEASE,
                0x4b
            )]
        );
        assert_eq!(scancode(b'A'.into()), Some((0x1e, false)));
        out.clear();
        encoder.encode(
            Input::Pointer {
                x: 10,
                y: 12,
                buttons: 1 | 8,
            },
            &mut out,
        );
        encoder.encode(
            Input::Pointer {
                x: 10,
                y: 12,
                buttons: 0,
            },
            &mut out,
        );
        assert_eq!(out.len(), 4);
        assert!(matches!(
            out[3],
            FastPathInputEvent::MouseEvent(MousePdu {
                flags: PointerFlags::LEFT_BUTTON,
                ..
            })
        ));
    }

    #[test]
    fn encodes_extended_mouse_button_press_and_release() {
        let mut encoder = Encoder::default();
        let mut out = Vec::new();
        for pressed in [true, false] {
            encoder.encode(
                Input::PointerExtra {
                    x: 12,
                    y: 34,
                    button: 2,
                    pressed,
                },
                &mut out,
            );
        }
        assert_eq!(
            out,
            [
                FastPathInputEvent::MouseEventEx(MouseXPdu {
                    flags: PointerXFlags::BUTTON2 | PointerXFlags::DOWN,
                    x_position: 12,
                    y_position: 34,
                }),
                FastPathInputEvent::MouseEventEx(MouseXPdu {
                    flags: PointerXFlags::BUTTON2,
                    x_position: 12,
                    y_position: 34,
                }),
            ]
        );
    }
}
