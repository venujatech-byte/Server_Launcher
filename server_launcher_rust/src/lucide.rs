//! Lucide Icon Pack definitions for Server Launcher.
//!
//! Provides crisp, vector-rendered Lucide icon glyphs loaded directly from
//! assets/fonts/lucide.ttf so that icons render consistently without relying on
//! system emoji fonts or encountering missing square tofu boxes.

pub const FONT_NAME: &str = "lucide";
pub const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");

// Sidebar and Navigation Icons
pub const SERVER: &str = "\u{e153}"; // server
pub const SERVERS: &str = "\u{e153}"; // server
pub const SCRIPT: &str = "\u{e2ed}"; // scroll
pub const TERMINAL: &str = "\u{e181}"; // terminal
pub const COMMAND: &str = "\u{e181}"; // terminal
pub const SYSTEMD: &str = "\u{e245}"; // settings-2
pub const SETTINGS: &str = "\u{e154}"; // settings
pub const SENSORS: &str = "\u{e186}"; // thermometer
pub const THERMOMETER: &str = "\u{e186}"; // thermometer

// Hardware & Telemetry Icons
pub const CPU: &str = "\u{e0a9}"; // cpu
pub const RAM: &str = "\u{e445}"; // memory-stick
pub const GPU: &str = "\u{e0df}"; // gamepad-2
pub const HARD_DRIVE: &str = "\u{e0ed}"; // hard-drive
pub const STORAGE: &str = "\u{e0ed}"; // hard-drive
pub const DATABASE: &str = "\u{e0ad}"; // database
pub const NETWORK: &str = "\u{e125}"; // network
pub const GLOBE: &str = "\u{e0e8}"; // globe
pub const BATTERY: &str = "\u{e055}"; // battery-full
pub const BATTERY_CHARGING: &str = "\u{e054}"; // battery-charging
pub const PLUG: &str = "\u{e37f}"; // plug
pub const ZAP: &str = "\u{e1b4}"; // zap
pub const ACTIVITY: &str = "\u{e038}"; // activity
pub const FLAME: &str = "\u{e0d2}"; // flame
pub const GAUGE: &str = "\u{e1bf}"; // gauge

// Actions and UI Controls
pub const SEARCH: &str = "\u{e151}"; // search
pub const REFRESH_CW: &str = "\u{e145}"; // refresh-cw
pub const PLAY: &str = "\u{e13c}"; // play
pub const SQUARE: &str = "\u{e167}"; // square
pub const STOP: &str = "\u{e167}"; // square
pub const PAUSE: &str = "\u{e12e}"; // pause
pub const PLUS: &str = "\u{e13d}"; // plus
pub const MINUS: &str = "\u{e11c}"; // minus
pub const TRASH: &str = "\u{e18e}"; // trash-2
pub const PENCIL: &str = "\u{e1f9}"; // pencil
pub const EDIT: &str = "\u{e1f9}"; // pencil
pub const SAVE: &str = "\u{e14d}"; // save
pub const EXTERNAL_LINK: &str = "\u{e0b9}"; // external-link
pub const TAG: &str = "\u{e17f}"; // tag
pub const FOLDER: &str = "\u{e0d7}"; // folder
pub const FOLDER_OPEN: &str = "\u{e247}"; // folder-open
pub const FILE_CODE: &str = "\u{e0c3}"; // file-code
pub const FILE_TEXT: &str = "\u{e0cc}"; // file-text
pub const CODE: &str = "\u{e093}"; // code
pub const LOCK: &str = "\u{e10b}"; // lock
pub const CLOCK: &str = "\u{e087}"; // clock
pub const CHECK: &str = "\u{e06c}"; // check
pub const X: &str = "\u{e1b2}"; // x
pub const CLOSE: &str = "\u{e1b2}"; // x
pub const COPY: &str = "\u{e09e}"; // copy
pub const EYE: &str = "\u{e0ba}"; // eye
pub const EYE_OFF: &str = "\u{e0bb}"; // eye-off
pub const SPARKLES: &str = "\u{e412}"; // sparkles
pub const LAYERS: &str = "\u{e529}"; // layers
pub const BOX: &str = "\u{e061}"; // box
pub const PACKAGE: &str = "\u{e129}"; // package
pub const SHIELD: &str = "\u{e158}"; // shield
pub const ALERT_TRIANGLE: &str = "\u{e193}"; // alert-triangle
pub const INFO: &str = "\u{e0f9}"; // info
pub const RADIO: &str = "\u{e142}"; // radio
pub const MAXIMIZE: &str = "\u{e113}"; // maximize-2
pub const MINIMIZE: &str = "\u{e11b}"; // minimize-2
pub const LOG_OUT: &str = "\u{e10e}"; // log-out
pub const MONITOR: &str = "\u{e11d}"; // monitor
pub const POWER: &str = "\u{e140}"; // power
pub const HELP_CIRCLE: &str = "\u{e082}"; // help-circle
pub const DOWNLOAD: &str = "\u{e0b2}"; // download
pub const UPLOAD: &str = "\u{e19e}"; // upload
pub const LINK: &str = "\u{e102}"; // link
pub const PALETTE: &str = "\u{e1dd}"; // palette
pub const ROCKET: &str = "\u{e286}"; // rocket
pub const KEY: &str = "\u{e0fd}"; // key
pub const WRENCH: &str = "\u{e1b1}"; // wrench
pub const CIRCLE_DOT: &str = "\u{e345}"; // circle-dot
pub const CIRCLE: &str = "\u{e076}"; // circle
pub const MOON: &str = "\u{e11e}"; // moon
pub const SUN: &str = "\u{e178}"; // sun
pub const COMPASS: &str = "\u{e09b}"; // compass
pub const KEYBOARD: &str = "\u{e284}"; // keyboard
pub const HASH: &str = "\u{e0ef}"; // hash

// Arrows and Directions
pub const ARROW_DOWN: &str = "\u{e042}"; // arrow-down
pub const ARROW_UP: &str = "\u{e04a}"; // arrow-up
pub const ARROW_LEFT: &str = "\u{e048}"; // arrow-left
pub const ARROW_RIGHT: &str = "\u{e049}"; // arrow-right
pub const CHEVRON_DOWN: &str = "\u{e06d}"; // chevron-down
pub const CHEVRON_RIGHT: &str = "\u{e06f}"; // chevron-right
pub const CHEVRON_UP: &str = "\u{e070}"; // chevron-up
pub const CHEVRON_LEFT: &str = "\u{e06e}"; // chevron-left

/// Configures and installs the Lucide font into egui's FontDefinitions.
pub fn setup_lucide_font(ctx: &eframe::egui::Context) {
    let mut fonts = eframe::egui::FontDefinitions::default();
    fonts.font_data.insert(
        FONT_NAME.to_owned(),
        eframe::egui::FontData::from_static(FONT_BYTES),
    );

    // Add to Proportional font family so it acts as fallback for icon glyphs in any normal text
    fonts
        .families
        .entry(eframe::egui::FontFamily::Proportional)
        .or_default()
        .push(FONT_NAME.to_owned());

    // Add to Monospace font family as well
    fonts
        .families
        .entry(eframe::egui::FontFamily::Monospace)
        .or_default()
        .push(FONT_NAME.to_owned());

    ctx.set_fonts(fonts);
}
