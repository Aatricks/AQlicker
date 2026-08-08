use serde::Serialize;

use crate::LogicalKey;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAC_LETTERS: [u16; 26] = [
    0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6,
];
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WIN_LETTERS: [u16; 26] = [
    0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19,
    0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
];
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAC_DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WIN_DIGITS: [u16; 10] = [0x0b, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a];
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAC_PUNCTUATION: [u16; 11] = [50, 27, 24, 33, 30, 42, 41, 39, 43, 47, 44];
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WIN_PUNCTUATION: [u16; 11] = [
    0x29, 0x0c, 0x0d, 0x1a, 0x1b, 0x2b, 0x27, 0x28, 0x33, 0x34, 0x35,
];

pub trait InputSink: Send {
    fn key_down(&mut self, key: LogicalKey) -> Result<(), InputFailure>;
    fn key_up(&mut self, key: LogicalKey) -> Result<(), InputFailure>;
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InputFailure {
    pub message: String,
}

impl InputFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for InputFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InputFailure {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlatformCode {
    Raw(u16),
    Named(NamedKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NamedKey {
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Space,
    Return,
    Tab,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_code(key: LogicalKey) -> PlatformCode {
    platform_code(key, &MAC_LETTERS, &MAC_DIGITS, &MAC_PUNCTUATION)
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_code(key: LogicalKey) -> PlatformCode {
    platform_code(key, &WIN_LETTERS, &WIN_DIGITS, &WIN_PUNCTUATION)
}

fn platform_code(
    key: LogicalKey,
    letters: &[u16; 26],
    digits: &[u16; 10],
    punctuation: &[u16; 11],
) -> PlatformCode {
    let position = LogicalKey::ALL
        .iter()
        .position(|candidate| *candidate == key)
        .expect("LogicalKey::ALL must remain exhaustive");
    match position {
        0..=25 => PlatformCode::Raw(letters[position]),
        26..=35 => PlatformCode::Raw(digits[position - 26]),
        36..=54 => PlatformCode::Named(NAMED_KEYS[position - 36]),
        55..=65 => PlatformCode::Raw(punctuation[position - 55]),
        _ => unreachable!("LogicalKey::ALL contains exactly 66 supported keys"),
    }
}

const NAMED_KEYS: [NamedKey; 19] = [
    NamedKey::F1,
    NamedKey::F2,
    NamedKey::F3,
    NamedKey::F4,
    NamedKey::F5,
    NamedKey::F6,
    NamedKey::F7,
    NamedKey::F8,
    NamedKey::F9,
    NamedKey::F10,
    NamedKey::F11,
    NamedKey::F12,
    NamedKey::ArrowUp,
    NamedKey::ArrowDown,
    NamedKey::ArrowLeft,
    NamedKey::ArrowRight,
    NamedKey::Space,
    NamedKey::Return,
    NamedKey::Tab,
];

pub struct EnigoInputSink {
    inner: enigo::Enigo,
}

impl EnigoInputSink {
    pub fn new() -> Result<Self, InputFailure> {
        enigo::Enigo::new(&enigo_settings())
            .map(|inner| Self { inner })
            .map_err(|error| InputFailure::new(error.to_string()))
    }

    fn emit(&mut self, key: LogicalKey, direction: enigo::Direction) -> Result<(), InputFailure> {
        use enigo::Keyboard;

        let result = match current_platform_code(key) {
            PlatformCode::Raw(code) => self.inner.raw(code, direction),
            PlatformCode::Named(key) => self.inner.key(enigo_named_key(key), direction),
        };
        result.map_err(|error| InputFailure::new(error.to_string()))
    }
}

impl InputSink for EnigoInputSink {
    fn key_down(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
        self.emit(key, enigo::Direction::Press)
    }

    fn key_up(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
        self.emit(key, enigo::Direction::Release)
    }
}

#[cfg(target_os = "macos")]
fn current_platform_code(key: LogicalKey) -> PlatformCode {
    macos_code(key)
}

#[cfg(target_os = "windows")]
fn current_platform_code(key: LogicalKey) -> PlatformCode {
    windows_code(key)
}

fn enigo_named_key(key: NamedKey) -> enigo::Key {
    match key {
        NamedKey::F1 => enigo::Key::F1,
        NamedKey::F2 => enigo::Key::F2,
        NamedKey::F3 => enigo::Key::F3,
        NamedKey::F4 => enigo::Key::F4,
        NamedKey::F5 => enigo::Key::F5,
        NamedKey::F6 => enigo::Key::F6,
        NamedKey::F7 => enigo::Key::F7,
        NamedKey::F8 => enigo::Key::F8,
        NamedKey::F9 => enigo::Key::F9,
        NamedKey::F10 => enigo::Key::F10,
        NamedKey::F11 => enigo::Key::F11,
        NamedKey::F12 => enigo::Key::F12,
        NamedKey::ArrowUp => enigo::Key::UpArrow,
        NamedKey::ArrowDown => enigo::Key::DownArrow,
        NamedKey::ArrowLeft => enigo::Key::LeftArrow,
        NamedKey::ArrowRight => enigo::Key::RightArrow,
        NamedKey::Space => enigo::Key::Space,
        NamedKey::Return => enigo::Key::Return,
        NamedKey::Tab => enigo::Key::Tab,
    }
}

fn enigo_settings() -> enigo::Settings {
    enigo::Settings {
        release_keys_when_dropped: true,
        open_prompt_to_get_permissions: false,
        independent_of_keyboard_state: true,
        ..enigo::Settings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogicalKey;

    #[test]
    fn every_supported_key_has_the_required_macos_mapping() {
        let expected_letters = [
            0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7,
            16, 6,
        ];
        let expected_digits = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
        let expected_punctuation = [50, 27, 24, 33, 30, 42, 41, 39, 43, 47, 44];

        assert_eq!(
            raw_codes(&LogicalKey::ALL[..26], macos_code),
            expected_letters
        );
        assert_eq!(
            raw_codes(&LogicalKey::ALL[26..36], macos_code),
            expected_digits
        );
        assert_eq!(
            raw_codes(&LogicalKey::ALL[55..], macos_code),
            expected_punctuation
        );
        assert_named_keys(macos_code);
    }

    #[test]
    fn every_supported_key_has_the_required_windows_mapping() {
        let expected_letters = [
            0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31,
            0x18, 0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
        ];
        let expected_digits = [0x0b, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a];
        let expected_punctuation = [
            0x29, 0x0c, 0x0d, 0x1a, 0x1b, 0x2b, 0x27, 0x28, 0x33, 0x34, 0x35,
        ];

        assert_eq!(
            raw_codes(&LogicalKey::ALL[..26], windows_code),
            expected_letters
        );
        assert_eq!(
            raw_codes(&LogicalKey::ALL[26..36], windows_code),
            expected_digits
        );
        assert_eq!(
            raw_codes(&LogicalKey::ALL[55..], windows_code),
            expected_punctuation
        );
        assert_named_keys(windows_code);
    }

    #[test]
    fn enigo_settings_disable_prompts_and_retain_both_release_safeguards() {
        let settings = enigo_settings();
        assert!(settings.release_keys_when_dropped);
        assert!(!settings.open_prompt_to_get_permissions);
        assert!(settings.independent_of_keyboard_state);
    }

    fn raw_codes(keys: &[LogicalKey], map: fn(LogicalKey) -> PlatformCode) -> Vec<u16> {
        keys.iter()
            .map(|&key| match map(key) {
                PlatformCode::Raw(code) => code,
                PlatformCode::Named(name) => {
                    panic!("expected raw mapping for {key:?}, got {name:?}")
                }
            })
            .collect()
    }

    fn assert_named_keys(map: fn(LogicalKey) -> PlatformCode) {
        let expected = [
            NamedKey::F1,
            NamedKey::F2,
            NamedKey::F3,
            NamedKey::F4,
            NamedKey::F5,
            NamedKey::F6,
            NamedKey::F7,
            NamedKey::F8,
            NamedKey::F9,
            NamedKey::F10,
            NamedKey::F11,
            NamedKey::F12,
            NamedKey::ArrowUp,
            NamedKey::ArrowDown,
            NamedKey::ArrowLeft,
            NamedKey::ArrowRight,
            NamedKey::Space,
            NamedKey::Return,
            NamedKey::Tab,
        ];
        let actual: Vec<_> = LogicalKey::ALL[36..55]
            .iter()
            .map(|&key| match map(key) {
                PlatformCode::Named(name) => name,
                PlatformCode::Raw(code) => panic!("expected named mapping for {key:?}, got {code}"),
            })
            .collect();
        assert_eq!(actual, expected);
    }
}
