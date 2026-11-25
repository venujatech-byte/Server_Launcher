use egui::Key;

pub(crate) fn keysym(key: Key) -> Option<u32> {
    let special = match key {
        Key::Escape => 0xff1b,
        Key::Tab => 0xff09,
        Key::Backspace => 0xff08,
        Key::Enter => 0xff0d,
        Key::Insert => 0xff63,
        Key::Delete => 0xffff,
        Key::Home => 0xff50,
        Key::End => 0xff57,
        Key::PageUp => 0xff55,
        Key::PageDown => 0xff56,
        Key::ArrowLeft => 0xff51,
        Key::ArrowUp => 0xff52,
        Key::ArrowRight => 0xff53,
        Key::ArrowDown => 0xff54,
        Key::Space => 0x20,
        _ => {
            let name = key.name();
            if let Some(number) = name
                .strip_prefix('F')
                .and_then(|value| value.parse::<u32>().ok())
            {
                return (1..=35).contains(&number).then_some(0xffbd + number);
            }
            let [byte] = name.as_bytes() else { return None };
            return Some(u32::from(byte.to_ascii_lowercase()));
        }
    };
    Some(special)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_keys_to_rfb_keysyms() {
        assert_eq!(keysym(Key::A), Some(b'a'.into()));
        assert_eq!(keysym(Key::F12), Some(0xffc9));
        assert_eq!(keysym(Key::ArrowLeft), Some(0xff51));
        assert_eq!(keysym(Key::Space), Some(0x20));
        assert_eq!(keysym(Key::Num0), Some(b'0'.into()));
    }
}
