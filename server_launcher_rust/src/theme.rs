use eframe::egui::{Color32, Rounding, Stroke, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    CatppuccinMocha,
    CatppuccinLatte,
    Light,
    Dracula,
    Nord,
    TokyoNight,
}

impl ThemeMode {
    pub fn all() -> &'static [ThemeMode] {
        &[
            ThemeMode::Dark,
            ThemeMode::CatppuccinMocha,
            ThemeMode::CatppuccinLatte,
            ThemeMode::Light,
            ThemeMode::Dracula,
            ThemeMode::Nord,
            ThemeMode::TokyoNight,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "Dark (Midnight)",
            ThemeMode::CatppuccinMocha => "Catppuccin Mocha",
            ThemeMode::CatppuccinLatte => "Catppuccin Latte",
            ThemeMode::Light => "Light Mode",
            ThemeMode::Dracula => "Dracula",
            ThemeMode::Nord => "Nord",
            ThemeMode::TokyoNight => "Tokyo Night",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "🌙",
            ThemeMode::CatppuccinMocha => "☕",
            ThemeMode::CatppuccinLatte => "🥛",
            ThemeMode::Light => "☀️",
            ThemeMode::Dracula => "🧛",
            ThemeMode::Nord => "❄️",
            ThemeMode::TokyoNight => "🌆",
        }
    }

    pub fn id_str(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::CatppuccinMocha => "catppuccin_mocha",
            ThemeMode::CatppuccinLatte => "catppuccin_latte",
            ThemeMode::Light => "light",
            ThemeMode::Dracula => "dracula",
            ThemeMode::Nord => "nord",
            ThemeMode::TokyoNight => "tokyo_night",
        }
    }

    pub fn from_id_str(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "dark" | "midnight" => ThemeMode::Dark,
            "catppuccin" | "catppuccin_mocha" | "mocha" => ThemeMode::CatppuccinMocha,
            "catppuccin_latte" | "latte" => ThemeMode::CatppuccinLatte,
            "light" | "light_mode" => ThemeMode::Light,
            "dracula" => ThemeMode::Dracula,
            "nord" => ThemeMode::Nord,
            "tokyo_night" | "tokyonight" => ThemeMode::TokyoNight,
            _ => ThemeMode::Dark,
        }
    }

    pub fn palette(&self) -> ThemePalette {
        match self {
            ThemeMode::Dark => ThemePalette {
                is_dark: true,
                bg_app: Color32::from_rgb(15, 17, 23),
                bg_header: Color32::from_rgb(15, 17, 23),
                bg_machines_nav: Color32::from_rgb(12, 14, 19),
                bg_sidebar: Color32::from_rgb(15, 17, 23),
                bg_main: Color32::from_rgb(15, 17, 23),
                bg_card: Color32::from_rgb(22, 25, 34),
                bg_card_hover: Color32::from_rgb(28, 32, 44),
                bg_card_active: Color32::from_rgb(32, 38, 55),
                bg_subtle: Color32::from_rgb(18, 21, 29),
                bg_input: Color32::from_rgb(10, 12, 17),
                bg_button: Color32::from_rgb(37, 40, 51),
                bg_button_hover: Color32::from_rgb(47, 51, 64),
                border: Color32::from_rgb(45, 50, 68),
                border_subtle: Color32::from_rgb(30, 34, 46),
                text_primary: Color32::from_rgb(235, 238, 245),
                text_secondary: Color32::from_rgb(200, 205, 216),
                text_muted: Color32::from_rgb(123, 131, 148),
                accent: Color32::from_rgb(108, 92, 231),
                accent_hover: Color32::from_rgb(124, 110, 240),
                accent_light: Color32::from_rgb(162, 155, 254),
                accent_subtle: Color32::from_rgb(40, 32, 68),
                accent_text: Color32::WHITE,
                success: Color32::from_rgb(0, 210, 160),
                success_subtle: Color32::from_rgb(20, 48, 40),
                warning: Color32::from_rgb(245, 158, 11),
                warning_subtle: Color32::from_rgb(48, 38, 18),
                danger: Color32::from_rgb(235, 87, 87),
                danger_subtle: Color32::from_rgb(48, 22, 22),
                info: Color32::from_rgb(96, 165, 250),
                info_subtle: Color32::from_rgb(20, 35, 55),
                terminal_bg: Color32::from_rgb(10, 12, 17),
                terminal_border: Color32::from_rgb(30, 34, 46),
                log_stdout: Color32::from_rgb(200, 205, 216),
                log_stderr: Color32::from_rgb(235, 87, 87),
                log_launcher: Color32::from_rgb(124, 138, 255),
                log_stdin: Color32::from_rgb(0, 210, 160),
                log_warning: Color32::from_rgb(245, 158, 11),
            },
            ThemeMode::CatppuccinMocha => ThemePalette {
                is_dark: true,
                bg_app: Color32::from_rgb(30, 30, 46),       // Base #1e1e2e
                bg_header: Color32::from_rgb(24, 24, 37),    // Mantle #181825
                bg_machines_nav: Color32::from_rgb(17, 17, 27), // Crust #11111b
                bg_sidebar: Color32::from_rgb(24, 24, 37),   // Mantle
                bg_main: Color32::from_rgb(30, 30, 46),      // Base
                bg_card: Color32::from_rgb(49, 50, 68),      // Surface0 #313244
                bg_card_hover: Color32::from_rgb(69, 71, 90), // Surface1 #45475a
                bg_card_active: Color32::from_rgb(88, 91, 112), // Surface2 #585b70
                bg_subtle: Color32::from_rgb(24, 24, 37),    // Mantle
                bg_input: Color32::from_rgb(17, 17, 27),     // Crust
                bg_button: Color32::from_rgb(49, 50, 68),    // Surface0
                bg_button_hover: Color32::from_rgb(69, 71, 90), // Surface1
                border: Color32::from_rgb(69, 71, 90),       // Surface1
                border_subtle: Color32::from_rgb(49, 50, 68), // Surface0
                text_primary: Color32::from_rgb(205, 214, 244), // Text #cdd6f4
                text_secondary: Color32::from_rgb(166, 173, 200), // Subtext0 #a6adc8
                text_muted: Color32::from_rgb(108, 112, 134),  // Overlay0 #6c7086
                accent: Color32::from_rgb(203, 166, 247),    // Mauve #cba6f7
                accent_hover: Color32::from_rgb(219, 188, 250),
                accent_light: Color32::from_rgb(180, 190, 254), // Lavender #b4befe
                accent_subtle: Color32::from_rgb(52, 42, 70),
                accent_text: Color32::from_rgb(17, 17, 27),
                success: Color32::from_rgb(166, 227, 161),   // Green #a6e3a1
                success_subtle: Color32::from_rgb(30, 50, 40),
                warning: Color32::from_rgb(250, 179, 135),   // Peach #fab387
                warning_subtle: Color32::from_rgb(55, 40, 30),
                danger: Color32::from_rgb(243, 139, 168),    // Red #f38ba8
                danger_subtle: Color32::from_rgb(55, 30, 40),
                info: Color32::from_rgb(137, 180, 250),      // Blue #89b4fa
                info_subtle: Color32::from_rgb(30, 40, 60),
                terminal_bg: Color32::from_rgb(17, 17, 27),  // Crust
                terminal_border: Color32::from_rgb(49, 50, 68),
                log_stdout: Color32::from_rgb(205, 214, 244),
                log_stderr: Color32::from_rgb(243, 139, 168),
                log_launcher: Color32::from_rgb(203, 166, 247),
                log_stdin: Color32::from_rgb(166, 227, 161),
                log_warning: Color32::from_rgb(250, 179, 135),
            },
            ThemeMode::CatppuccinLatte => ThemePalette {
                is_dark: false,
                bg_app: Color32::from_rgb(239, 241, 245),       // Base #eff1f5
                bg_header: Color32::from_rgb(230, 233, 239),    // Mantle #e6e9ef
                bg_machines_nav: Color32::from_rgb(220, 224, 232), // Crust #dce0e8
                bg_sidebar: Color32::from_rgb(230, 233, 239),   // Mantle
                bg_main: Color32::from_rgb(239, 241, 245),      // Base
                bg_card: Color32::from_rgb(255, 255, 255),      // Pure white card
                bg_card_hover: Color32::from_rgb(232, 235, 242),
                bg_card_active: Color32::from_rgb(215, 222, 238),
                bg_subtle: Color32::from_rgb(220, 224, 232),    // Crust
                bg_input: Color32::from_rgb(255, 255, 255),
                bg_button: Color32::from_rgb(204, 208, 218),    // Surface0 #ccd0da
                bg_button_hover: Color32::from_rgb(188, 192, 204), // Surface1 #bcc0cc
                border: Color32::from_rgb(204, 208, 218),
                border_subtle: Color32::from_rgb(220, 224, 232),
                text_primary: Color32::from_rgb(76, 79, 105),   // Text #4c4f69
                text_secondary: Color32::from_rgb(108, 111, 133), // Subtext0 #6c6f85
                text_muted: Color32::from_rgb(140, 143, 161),  // Overlay1
                accent: Color32::from_rgb(136, 57, 239),        // Mauve #8839ef
                accent_hover: Color32::from_rgb(114, 40, 215),
                accent_light: Color32::from_rgb(114, 135, 253), // Lavender #7287fd
                accent_subtle: Color32::from_rgb(238, 230, 252),
                accent_text: Color32::WHITE,
                success: Color32::from_rgb(64, 160, 43),        // Green #40a02b
                success_subtle: Color32::from_rgb(225, 245, 220),
                warning: Color32::from_rgb(230, 100, 10),       // Peach
                warning_subtle: Color32::from_rgb(255, 238, 220),
                danger: Color32::from_rgb(210, 15, 57),         // Red #d20f39
                danger_subtle: Color32::from_rgb(255, 228, 233),
                info: Color32::from_rgb(30, 102, 245),          // Blue #1e66f5
                info_subtle: Color32::from_rgb(225, 238, 255),
                terminal_bg: Color32::from_rgb(245, 246, 248),
                terminal_border: Color32::from_rgb(204, 208, 218),
                log_stdout: Color32::from_rgb(76, 79, 105),
                log_stderr: Color32::from_rgb(210, 15, 57),
                log_launcher: Color32::from_rgb(136, 57, 239),
                log_stdin: Color32::from_rgb(64, 160, 43),
                log_warning: Color32::from_rgb(230, 100, 10),
            },
            ThemeMode::Light => ThemePalette {
                is_dark: false,
                bg_app: Color32::from_rgb(241, 245, 249),       // Slate 100
                bg_header: Color32::from_rgb(255, 255, 255),    // Pure white header
                bg_machines_nav: Color32::from_rgb(235, 238, 245),
                bg_sidebar: Color32::from_rgb(248, 250, 252),   // Slate 50
                bg_main: Color32::from_rgb(241, 245, 249),
                bg_card: Color32::from_rgb(255, 255, 255),
                bg_card_hover: Color32::from_rgb(241, 245, 249),
                bg_card_active: Color32::from_rgb(224, 231, 255),
                bg_subtle: Color32::from_rgb(241, 245, 249),
                bg_input: Color32::from_rgb(255, 255, 255),
                bg_button: Color32::from_rgb(226, 232, 240),    // Slate 200
                bg_button_hover: Color32::from_rgb(203, 213, 225), // Slate 300
                border: Color32::from_rgb(218, 224, 233),
                border_subtle: Color32::from_rgb(232, 236, 242),
                text_primary: Color32::from_rgb(15, 23, 42),    // Slate 900
                text_secondary: Color32::from_rgb(51, 65, 85),  // Slate 700
                text_muted: Color32::from_rgb(100, 116, 139),   // Slate 500
                accent: Color32::from_rgb(79, 70, 229),         // Indigo 600
                accent_hover: Color32::from_rgb(67, 56, 202),
                accent_light: Color32::from_rgb(99, 102, 241),  // Indigo 500
                accent_subtle: Color32::from_rgb(238, 242, 255),
                accent_text: Color32::WHITE,
                success: Color32::from_rgb(16, 185, 129),       // Emerald 500
                success_subtle: Color32::from_rgb(236, 253, 245),
                warning: Color32::from_rgb(234, 88, 12),        // Amber 600
                warning_subtle: Color32::from_rgb(255, 247, 237),
                danger: Color32::from_rgb(225, 29, 72),         // Rose 600
                danger_subtle: Color32::from_rgb(255, 241, 242),
                info: Color32::from_rgb(37, 99, 235),           // Blue 600
                info_subtle: Color32::from_rgb(239, 246, 255),
                terminal_bg: Color32::from_rgb(248, 250, 252),
                terminal_border: Color32::from_rgb(226, 232, 240),
                log_stdout: Color32::from_rgb(30, 41, 59),
                log_stderr: Color32::from_rgb(225, 29, 72),
                log_launcher: Color32::from_rgb(79, 70, 229),
                log_stdin: Color32::from_rgb(16, 185, 129),
                log_warning: Color32::from_rgb(234, 88, 12),
            },
            ThemeMode::Dracula => ThemePalette {
                is_dark: true,
                bg_app: Color32::from_rgb(40, 42, 54),          // #282a36
                bg_header: Color32::from_rgb(33, 34, 44),       // #21222c
                bg_machines_nav: Color32::from_rgb(25, 26, 34),
                bg_sidebar: Color32::from_rgb(33, 34, 44),
                bg_main: Color32::from_rgb(40, 42, 54),
                bg_card: Color32::from_rgb(52, 55, 70),         // #343746
                bg_card_hover: Color32::from_rgb(68, 71, 90),   // Selection #44475a
                bg_card_active: Color32::from_rgb(80, 84, 106),
                bg_subtle: Color32::from_rgb(33, 34, 44),
                bg_input: Color32::from_rgb(25, 26, 34),
                bg_button: Color32::from_rgb(68, 71, 90),
                bg_button_hover: Color32::from_rgb(85, 89, 113),
                border: Color32::from_rgb(68, 71, 90),
                border_subtle: Color32::from_rgb(52, 55, 70),
                text_primary: Color32::from_rgb(248, 248, 242), // #f8f8f2
                text_secondary: Color32::from_rgb(215, 218, 226),
                text_muted: Color32::from_rgb(98, 114, 164),    // #6272a4
                accent: Color32::from_rgb(189, 147, 249),       // Purple #bd93f9
                accent_hover: Color32::from_rgb(205, 168, 255),
                accent_light: Color32::from_rgb(255, 121, 198), // Pink #ff79c6
                accent_subtle: Color32::from_rgb(60, 45, 80),
                accent_text: Color32::from_rgb(40, 42, 54),
                success: Color32::from_rgb(80, 250, 123),       // Green #50fa7b
                success_subtle: Color32::from_rgb(30, 55, 38),
                warning: Color32::from_rgb(255, 184, 108),      // Orange #ffb86c
                warning_subtle: Color32::from_rgb(60, 45, 30),
                danger: Color32::from_rgb(255, 85, 85),         // Red #ff5555
                danger_subtle: Color32::from_rgb(60, 30, 30),
                info: Color32::from_rgb(139, 233, 253),         // Cyan #8be9fd
                info_subtle: Color32::from_rgb(30, 50, 60),
                terminal_bg: Color32::from_rgb(25, 26, 34),
                terminal_border: Color32::from_rgb(68, 71, 90),
                log_stdout: Color32::from_rgb(248, 248, 242),
                log_stderr: Color32::from_rgb(255, 85, 85),
                log_launcher: Color32::from_rgb(189, 147, 249),
                log_stdin: Color32::from_rgb(80, 250, 123),
                log_warning: Color32::from_rgb(255, 184, 108),
            },
            ThemeMode::Nord => ThemePalette {
                is_dark: true,
                bg_app: Color32::from_rgb(46, 52, 64),          // Polar Night 0 #2e3440
                bg_header: Color32::from_rgb(40, 45, 56),
                bg_machines_nav: Color32::from_rgb(35, 40, 50),
                bg_sidebar: Color32::from_rgb(40, 45, 56),
                bg_main: Color32::from_rgb(46, 52, 64),
                bg_card: Color32::from_rgb(59, 66, 82),         // Polar Night 1 #3b4252
                bg_card_hover: Color32::from_rgb(67, 76, 94),   // Polar Night 2 #434c5e
                bg_card_active: Color32::from_rgb(76, 86, 106), // Polar Night 3 #4c566a
                bg_subtle: Color32::from_rgb(40, 45, 56),
                bg_input: Color32::from_rgb(35, 40, 50),
                bg_button: Color32::from_rgb(67, 76, 94),
                bg_button_hover: Color32::from_rgb(76, 86, 106),
                border: Color32::from_rgb(76, 86, 106),
                border_subtle: Color32::from_rgb(59, 66, 82),
                text_primary: Color32::from_rgb(236, 239, 244), // Snow Storm 2 #eceff4
                text_secondary: Color32::from_rgb(216, 222, 233), // Snow Storm 0 #d8dee9
                text_muted: Color32::from_rgb(140, 150, 170),
                accent: Color32::from_rgb(136, 192, 208),       // Frost 1 #88c0d0
                accent_hover: Color32::from_rgb(143, 188, 187), // Frost 0 #8fbcbb
                accent_light: Color32::from_rgb(129, 161, 193), // Frost 2 #81a1c1
                accent_subtle: Color32::from_rgb(45, 65, 75),
                accent_text: Color32::from_rgb(46, 52, 64),
                success: Color32::from_rgb(163, 190, 140),       // Aurora Green #a3be8c
                success_subtle: Color32::from_rgb(40, 55, 45),
                warning: Color32::from_rgb(235, 203, 139),      // Aurora Yellow #ebcb8b
                warning_subtle: Color32::from_rgb(55, 50, 35),
                danger: Color32::from_rgb(191, 97, 106),        // Aurora Red #bf616a
                danger_subtle: Color32::from_rgb(55, 35, 38),
                info: Color32::from_rgb(129, 161, 193),         // Frost 2
                info_subtle: Color32::from_rgb(40, 50, 65),
                terminal_bg: Color32::from_rgb(35, 40, 50),
                terminal_border: Color32::from_rgb(67, 76, 94),
                log_stdout: Color32::from_rgb(236, 239, 244),
                log_stderr: Color32::from_rgb(191, 97, 106),
                log_launcher: Color32::from_rgb(136, 192, 208),
                log_stdin: Color32::from_rgb(163, 190, 140),
                log_warning: Color32::from_rgb(235, 203, 139),
            },
            ThemeMode::TokyoNight => ThemePalette {
                is_dark: true,
                bg_app: Color32::from_rgb(26, 27, 38),          // #1a1b26
                bg_header: Color32::from_rgb(22, 22, 30),       // #16161e
                bg_machines_nav: Color32::from_rgb(18, 18, 26),
                bg_sidebar: Color32::from_rgb(22, 22, 30),
                bg_main: Color32::from_rgb(26, 27, 38),
                bg_card: Color32::from_rgb(36, 40, 59),         // #24283b
                bg_card_hover: Color32::from_rgb(41, 46, 66),   // #292e42
                bg_card_active: Color32::from_rgb(50, 56, 80),
                bg_subtle: Color32::from_rgb(22, 22, 30),
                bg_input: Color32::from_rgb(18, 18, 26),
                bg_button: Color32::from_rgb(41, 46, 66),
                bg_button_hover: Color32::from_rgb(52, 58, 84),
                border: Color32::from_rgb(52, 58, 84),
                border_subtle: Color32::from_rgb(36, 40, 59),
                text_primary: Color32::from_rgb(192, 202, 245), // #c0caf5
                text_secondary: Color32::from_rgb(169, 177, 214), // #a9b1d6
                text_muted: Color32::from_rgb(86, 95, 137),     // #565f89
                accent: Color32::from_rgb(122, 162, 247),       // Blue #7aa2f7
                accent_hover: Color32::from_rgb(139, 176, 255),
                accent_light: Color32::from_rgb(187, 154, 247), // Purple #bb9af7
                accent_subtle: Color32::from_rgb(35, 45, 75),
                accent_text: Color32::from_rgb(26, 27, 38),
                success: Color32::from_rgb(158, 206, 106),       // Green #9ece6a
                success_subtle: Color32::from_rgb(32, 50, 30),
                warning: Color32::from_rgb(255, 158, 100),      // Orange #ff9e64
                warning_subtle: Color32::from_rgb(55, 40, 25),
                danger: Color32::from_rgb(247, 118, 142),       // Red #f7768e
                danger_subtle: Color32::from_rgb(55, 30, 35),
                info: Color32::from_rgb(125, 207, 255),         // Cyan #7dcfff
                info_subtle: Color32::from_rgb(30, 48, 62),
                terminal_bg: Color32::from_rgb(18, 18, 26),
                terminal_border: Color32::from_rgb(41, 46, 66),
                log_stdout: Color32::from_rgb(192, 202, 245),
                log_stderr: Color32::from_rgb(247, 118, 142),
                log_launcher: Color32::from_rgb(187, 154, 247),
                log_stdin: Color32::from_rgb(158, 206, 106),
                log_warning: Color32::from_rgb(255, 158, 100),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThemePalette {
    pub is_dark: bool,
    pub bg_app: Color32,
    pub bg_header: Color32,
    pub bg_machines_nav: Color32,
    pub bg_sidebar: Color32,
    pub bg_main: Color32,
    pub bg_card: Color32,
    pub bg_card_hover: Color32,
    pub bg_card_active: Color32,
    pub bg_subtle: Color32,
    pub bg_input: Color32,
    pub bg_button: Color32,
    pub bg_button_hover: Color32,
    pub border: Color32,
    pub border_subtle: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_light: Color32,
    pub accent_subtle: Color32,
    pub accent_text: Color32,
    pub success: Color32,
    pub success_subtle: Color32,
    pub warning: Color32,
    pub warning_subtle: Color32,
    pub danger: Color32,
    pub danger_subtle: Color32,
    pub info: Color32,
    pub info_subtle: Color32,
    pub terminal_bg: Color32,
    pub terminal_border: Color32,
    pub log_stdout: Color32,
    pub log_stderr: Color32,
    pub log_launcher: Color32,
    pub log_stdin: Color32,
    pub log_warning: Color32,
}

impl ThemePalette {
    pub fn egui_visuals(&self) -> Visuals {
        let mut visuals = if self.is_dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.panel_fill = self.bg_app;
        visuals.window_fill = self.bg_card;
        visuals.faint_bg_color = self.bg_subtle;
        visuals.extreme_bg_color = self.bg_input;

        visuals.widgets.noninteractive.bg_fill = self.bg_card;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.text_secondary);
        visuals.widgets.noninteractive.rounding = Rounding::same(4.0);

        visuals.widgets.inactive.bg_fill = self.bg_button;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, self.text_secondary);
        visuals.widgets.inactive.rounding = Rounding::same(4.0);

        visuals.widgets.hovered.bg_fill = self.bg_button_hover;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, self.text_primary);
        visuals.widgets.hovered.rounding = Rounding::same(6.0);

        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, self.accent_text);
        visuals.widgets.active.rounding = Rounding::same(6.0);

        visuals.widgets.open.bg_fill = self.bg_card;
        visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, self.text_primary);

        visuals.selection.bg_fill = self.accent;
        visuals.selection.stroke = Stroke::new(1.0_f32, self.accent_text);

        visuals.override_text_color = Some(self.text_primary);
        visuals.window_stroke = Stroke::new(1.0_f32, self.border);

        visuals
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_themes_generate_visuals() {
        for theme in ThemeMode::all() {
            let palette = theme.palette();
            let _visuals = palette.egui_visuals();
            assert!(!theme.name().is_empty());
            assert!(!theme.emoji().is_empty());
        }
    }

    #[test]
    fn test_theme_from_id_str() {
        assert_eq!(ThemeMode::from_id_str("catppuccin-mocha"), ThemeMode::CatppuccinMocha);
        assert_eq!(ThemeMode::from_id_str("latte"), ThemeMode::CatppuccinLatte);
        assert_eq!(ThemeMode::from_id_str("light"), ThemeMode::Light);
        assert_eq!(ThemeMode::from_id_str("dracula"), ThemeMode::Dracula);
        assert_eq!(ThemeMode::from_id_str("nord"), ThemeMode::Nord);
        assert_eq!(ThemeMode::from_id_str("tokyo-night"), ThemeMode::TokyoNight);
        assert_eq!(ThemeMode::from_id_str("unknown"), ThemeMode::Dark);
    }
}
