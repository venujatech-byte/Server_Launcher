//! Lucide Icon Pack definitions for Server Launcher.
//!
//! Provides crisp, vector-rendered Lucide icon glyphs loaded directly from
//! assets/fonts/lucide.ttf so that icons render consistently without relying on
//! system emoji fonts or encountering missing square tofu boxes.

pub const FONT_NAME: &str = "lucide";
pub const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");

// Sidebar and Navigation Icons
pub const SERVER: &str = "\u{e1b6}"; // server-plus
pub const SERVERS: &str = "\u{e1b6}"; // server-plus
pub const SCRIPT: &str = "\u{e0b7}"; // file-code
pub const TERMINAL: &str = "\u{e125}"; // terminal
pub const COMMAND: &str = "\u{e79c}"; // command
pub const SYSTEMD: &str = "\u{e106}"; // sliders-vertical
pub const SETTINGS: &str = "\u{e106}"; // sliders-vertical
pub const SLIDERS: &str = "\u{e106}"; // sliders-vertical
pub const SENSORS: &str = "\u{e188}"; // gauge
pub const THERMOMETER: &str = "\u{e12a}"; // thermometer

// Hardware & Telemetry Icons
pub const CPU: &str = "\u{e04b}"; // cpu
pub const RAM: &str = "\u{e1b6}"; // server-plus
pub const GPU: &str = "\u{e211}"; // monitor-pc
pub const HARD_DRIVE: &str = "\u{e0c5}"; // hard-drive
pub const STORAGE: &str = "\u{e0c5}"; // hard-drive
pub const DATABASE: &str = "\u{e04f}"; // database
pub const NETWORK: &str = "\u{e4bb}"; // cast
pub const GLOBE: &str = "\u{e0c0}"; // globe
pub const BATTERY: &str = "\u{e455}"; // battery-full
pub const BATTERY_CHARGING: &str = "\u{e44f}"; // battery-charging
pub const PLUG: &str = "\u{e190}"; // zap
pub const ZAP: &str = "\u{e190}"; // zap
pub const ACTIVITY: &str = "\u{e3a7}"; // activity
pub const FLAME: &str = "\u{e0aa}"; // flame
pub const GAUGE: &str = "\u{e188}"; // gauge
pub const WIFI: &str = "\u{e178}"; // wifi
pub const BLUETOOTH: &str = "\u{e47f}"; // bluetooth
pub const HEADPHONES: &str = "\u{e0c9}"; // headphones
pub const LAYOUT_GRID: &str = "\u{e0df}"; // layout-grid

// Actions and UI Controls
pub const SEARCH: &str = "\u{e0a3}"; // file-search
pub const REFRESH_CW: &str = "\u{e101}"; // refresh-cw
pub const PLAY: &str = "\u{006b}"; // circle-play
pub const SQUARE: &str = "\u{e10b}"; // square
pub const STOP: &str = "\u{006e}"; // circle-stop
pub const PAUSE: &str = "\u{006a}"; // circle-pause
pub const PLUS: &str = "\u{006c}"; // circle-plus
pub const MINUS: &str = "\u{0069}"; // circle-minus
pub const TRASH: &str = "\u{e132}"; // trash-2
pub const PENCIL: &str = "\u{e116}"; // edit
pub const EDIT: &str = "\u{e116}"; // edit
pub const SAVE: &str = "\u{e75f}"; // save-check
pub const EXTERNAL_LINK: &str = "\u{e07b}"; // external-link
pub const TAG: &str = "\u{e123}"; // tag
pub const FOLDER: &str = "\u{e0af}"; // folder
pub const FOLDER_OPEN: &str = "\u{e75a}"; // folder-git
pub const FILE_CODE: &str = "\u{e0b7}"; // file-code
pub const FILE_TEXT: &str = "\u{e0a4}"; // file-text
pub const CODE: &str = "\u{e10f}"; // code-square
pub const LOCK: &str = "\u{e0ed}"; // lock
pub const CLOCK: &str = "\u{0072}"; // clock-4
pub const CHECK: &str = "\u{e4df}"; // check
pub const X: &str = "\u{e395}"; // x
pub const CLOSE: &str = "\u{e395}"; // x
pub const COPY: &str = "\u{e74f}"; // copy
pub const EYE: &str = "\u{e07c}"; // eye
pub const EYE_OFF: &str = "\u{e07d}"; // eye-off
pub const SPARKLES: &str = "\u{e1c4}"; // broom-sparkles
pub const LAYERS: &str = "\u{e18a}"; // layout-dashboard
pub const BOX: &str = "\u{e49d}"; // box
pub const PACKAGE: &str = "\u{e213}"; // door-closed-package
pub const SHIELD: &str = "\u{e1ac}"; // shield-ellipsis
pub const ALERT_TRIANGLE: &str = "\u{e142}"; // alert-triangle
pub const INFO: &str = "\u{e0d1}"; // info
pub const RADIO: &str = "\u{e754}"; // radio
pub const MAXIMIZE: &str = "\u{e0f4}"; // maximize
pub const MINIMIZE: &str = "\u{e18e}"; // move-diagonal-2
pub const LOG_OUT: &str = "\u{e0f0}"; // log-out
pub const MONITOR: &str = "\u{e211}"; // monitor-pc
pub const POWER: &str = "\u{e0fc}"; // power
pub const HELP_CIRCLE: &str = "\u{006d}"; // circle-help
pub const DOWNLOAD: &str = "\u{e79f}"; // download
pub const UPLOAD: &str = "\u{e168}"; // upload
pub const LINK: &str = "\u{e0e2}"; // link
pub const PALETTE: &str = "\u{e1dd}"; // playing-cards-fan
pub const ROCKET: &str = "\u{e190}"; // zap
pub const KEY: &str = "\u{e0dd}"; // key
pub const WRENCH: &str = "\u{e17b}"; // wrench
pub const CIRCLE_DOT: &str = "\u{e1f3}"; // circle-dashed-check
pub const CIRCLE: &str = "\u{0061}"; // circle
pub const MOON: &str = "\u{e760}"; // moon
pub const SUN: &str = "\u{e11c}"; // sun
pub const COMPASS: &str = "\u{e03d}"; // compass
pub const KEYBOARD: &str = "\u{e125}"; // terminal
pub const HASH: &str = "\u{e0c7}"; // hash

// Arrows and Directions
pub const ARROW_DOWN: &str = "\u{e766}"; // arrow-down
pub const ARROW_UP: &str = "\u{e76c}"; // arrow-up
pub const ARROW_LEFT: &str = "\u{e407}"; // arrow-left
pub const ARROW_RIGHT: &str = "\u{e40d}"; // arrow-right
pub const CHEVRON_DOWN: &str = "\u{e71f}"; // chevron-down
pub const CHEVRON_RIGHT: &str = "\u{e721}"; // chevron-right
pub const CHEVRON_UP: &str = "\u{e722}"; // chevron-up
pub const CHEVRON_LEFT: &str = "\u{e720}"; // chevron-left

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

    // Also add Hack as fallback for Proportional font family so symbols (bullets, arrows) render reliably
    fonts
        .families
        .entry(eframe::egui::FontFamily::Proportional)
        .or_default()
        .push("Hack".to_owned());

    // Add to Monospace font family as well
    fonts
        .families
        .entry(eframe::egui::FontFamily::Monospace)
        .or_default()
        .push(FONT_NAME.to_owned());

    ctx.set_fonts(fonts);
}
