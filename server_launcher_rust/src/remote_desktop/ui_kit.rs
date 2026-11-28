//! Shared UI kit for the "Edit Remote PC" dialog.
//!
//! Surfaces are deliberately borderless: separation comes from tonal fills,
//! spacing and radius only. The exception is the 2px keyboard focus ring.

use crate::theme::ThemePalette;
use egui::{
    Color32, Frame, Key, Label, Rect, Response, RichText, Rounding, Sense, Stroke, TextStyle, Ui,
    Vec2, WidgetText,
};

pub(crate) const WINDOW: Color32 = Color32::from_rgb(0x16, 0x19, 0x22);
pub(crate) const CARD: Color32 = Color32::from_rgb(0x12, 0x15, 0x1d);
pub(crate) const CONTROL: Color32 = Color32::from_rgb(0x25, 0x28, 0x33);
pub(crate) const INPUT: Color32 = Color32::from_rgb(0x0a, 0x0c, 0x11);
pub(crate) const DISABLED: Color32 = Color32::from_rgb(0x2f, 0x32, 0x3d);
pub(crate) const DISABLED_TEXT: Color32 = Color32::from_rgb(0x7d, 0x83, 0x94);
pub(crate) const TEXT: Color32 = Color32::from_rgb(0xeb, 0xee, 0xf5);
pub(crate) const LABEL: Color32 = Color32::from_rgb(0xcd, 0xd1, 0xd9);
pub(crate) const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xb0);
pub(crate) const PLACEHOLDER: Color32 = Color32::from_rgb(0x6f, 0x75, 0x86);
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x35, 0x84, 0xe4);
pub(crate) const SWITCH_OFF: Color32 = Color32::from_rgb(0x3a, 0x3e, 0x4c);
pub(crate) const SWITCH_KNOB_OFF: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xb0);
pub(crate) const THUMB_HOVER: Color32 = Color32::from_rgb(0x4f, 0x54, 0x63);
pub(crate) const SUCCESS: Color32 = Color32::from_rgb(0x4f, 0xd1, 0xa1);
pub(crate) const WARNING: Color32 = Color32::from_rgb(0xf2, 0xb4, 0x5b);
pub(crate) const DANGER: Color32 = Color32::from_rgb(0xf5, 0x8a, 0x95);
pub(crate) const DANGER_BG: Color32 = Color32::from_rgb(0x2a, 0x1d, 0x27);

const CARD_RADIUS: f32 = 12.0;
const CONTROL_RADIUS: f32 = 9.0;
const FIELD_HEIGHT: f32 = 33.0;

fn accent(palette: &ThemePalette) -> Color32 {
    let _ = palette;
    ACCENT
}

pub(crate) fn focus_ring(ui: &Ui, response: &Response, rect: Rect, radius: f32) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            Rounding::same(radius + 2.0),
            Stroke::new(2.0_f32, ACCENT),
        );
        response.scroll_to_me(None);
    }
}

/// Card surface: 14px radius, 16px inner padding, fill only.
pub(crate) fn card_frame() -> Frame {
    Frame::none()
        .fill(CARD)
        .rounding(Rounding::same(CARD_RADIUS))
        .stroke(Stroke::NONE)
        .inner_margin(egui::Margin::same(16.0))
}

/// 26px accent icon chip followed by a compact semibold title at `title_size`.
fn card_header(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    title_size: f32,
    chevron: Option<bool>,
    palette: &ThemePalette,
) {
    ui.horizontal(|ui| {
        let (chip_rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
        ui.painter().rect_filled(
            chip_rect,
            Rounding::same(7.0),
            Color32::from_rgba_unmultiplied(
                accent(palette).r(),
                accent(palette).g(),
                accent(palette).b(),
                38,
            ),
        );
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(chip_rect), |ui| {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(icon)
                        .size(12.0)
                        .color(accent(palette))
                        .strong(),
                );
            });
        });

        ui.add_space(8.0);
        ui.label(RichText::new(title).size(title_size).strong().color(TEXT));

        if let Some(is_open) = chevron {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(if is_open {
                        crate::lucide::CHEVRON_DOWN
                    } else {
                        crate::lucide::CHEVRON_RIGHT
                    })
                    .size(12.0)
                    .color(MUTED),
                );
            });
        }
    });
}

/// Standard card with a header and 10px gap before its contents.
pub(crate) fn card<R>(
    ui: &mut Ui,
    palette: &ThemePalette,
    icon: &str,
    title: &str,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let width = ui.available_width();
    card_frame()
        .show(ui, |ui| {
            ui.set_width(width - 32.0);
            card_header(ui, icon, title, 12.0, None, palette);
            ui.add_space(10.0);
            contents(ui)
        })
        .inner
}

/// Compact borderless strip used for service actions and auto-detection.
pub(crate) fn strip_card<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width();
    Frame::none()
        .fill(CARD)
        .rounding(Rounding::same(CARD_RADIUS))
        .stroke(Stroke::NONE)
        .inner_margin(egui::Margin::symmetric(12.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            contents(ui)
        })
        .inner
}

/// Collapsible card. `open` is toggled by clicking the header.
pub(crate) fn collapsible_card<R>(
    ui: &mut Ui,
    palette: &ThemePalette,
    icon: &str,
    title: &str,
    open: &mut bool,
    contents: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let mut result = None;
    let width = ui.available_width();
    card_frame().show(ui, |ui| {
        ui.set_width(width - 32.0);
        let width = ui.available_width();
        let (header_rect, header_response) =
            ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click());
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(header_rect), |ui| {
            // Non-obvious: collapsible headers were Button-styled before the
            // restyle, so keep them at 11.5 rather than the card-title 12.
            card_header(ui, icon, title, 11.5, Some(*open), palette);
        });
        let toggled = header_response.clicked()
            || (header_response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter)));
        if toggled {
            *open = !*open;
            ui.ctx().request_repaint();
        }
        focus_ring(ui, &header_response, header_rect, 8.0);
        if *open {
            ui.add_space(12.0);
            result = Some(contents(ui));
        }
    });
    result
}

/// 11px label above a field. Required fields get a blue asterisk.
pub(crate) fn field_label(ui: &mut Ui, label: &str, required: bool, palette: &ThemePalette) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(11.0).color(LABEL));
            if required {
                ui.label(
                    RichText::new("*")
                        .size(11.0)
                        .strong()
                        .color(accent(palette)),
                );
            }
        });
    });
}

/// Muted hint using the default Small size, 6px below its field.
pub(crate) fn hint(ui: &mut Ui, text: &str) {
    ui.add_space(5.0);
    ui.label(RichText::new(text).size(11.0).color(MUTED));
}

/// Muted one-line status used in service strips and port suggestions.
pub(crate) fn status_line(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(MUTED));
}

/// 33px single-line input with 12px horizontal padding.
pub(crate) fn text_field(
    ui: &mut Ui,
    text: &mut String,
    placeholder: &str,
    password: bool,
    palette: &ThemePalette,
) -> Response {
    text_field_with_disabled_style(
        ui,
        text,
        placeholder,
        password,
        palette,
        INPUT.linear_multiply(0.5),
        PLACEHOLDER,
    )
}

pub(crate) fn text_field_with_disabled_style(
    ui: &mut Ui,
    text: &mut String,
    placeholder: &str,
    password: bool,
    palette: &ThemePalette,
    disabled_fill: Color32,
    disabled_text: Color32,
) -> Response {
    let _ = palette;
    let enabled = ui.is_enabled();
    let fill = if enabled { INPUT } else { disabled_fill };
    let value_color = if enabled { TEXT } else { disabled_text };
    let hint_color = PLACEHOLDER;
    let width = ui.available_width().max(80.0);
    let frame = Frame::none()
        .fill(fill)
        .rounding(Rounding::same(CONTROL_RADIUS))
        .stroke(Stroke::NONE)
        .inner_margin(egui::Margin::symmetric(12.0, 4.0));
    let response = frame
        .show(ui, |ui| {
            let mut edit = egui::TextEdit::singleline(text)
                .frame(false)
                .hint_text(RichText::new(placeholder).color(hint_color))
                .text_color(value_color)
                .desired_width((width - 24.0).max(20.0))
                .vertical_align(egui::Align::Center)
                .min_size(Vec2::new(0.0, 18.0));
            if password {
                edit = edit.password(true);
            }
            ui.add(edit)
        })
        .inner;
    focus_ring(ui, &response, response.rect, CONTROL_RADIUS);
    response
}

fn apply_control_visuals(ui: &mut Ui, fill: Color32, text_color: Color32) {
    ui.visuals_mut().widgets.inactive.bg_fill = fill;
    ui.visuals_mut().widgets.hovered.bg_fill = fill;
    ui.visuals_mut().widgets.active.bg_fill = fill;
    ui.visuals_mut().widgets.inactive.weak_bg_fill = fill;
    ui.visuals_mut().widgets.hovered.weak_bg_fill = fill;
    ui.visuals_mut().widgets.active.weak_bg_fill = fill;
    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
    ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
    ui.visuals_mut().widgets.active.bg_stroke = Stroke::NONE;
    ui.visuals_mut().widgets.inactive.rounding = Rounding::same(CONTROL_RADIUS);
    ui.visuals_mut().widgets.hovered.rounding = Rounding::same(CONTROL_RADIUS);
    ui.visuals_mut().widgets.active.rounding = Rounding::same(CONTROL_RADIUS);
    ui.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(1.0_f32, text_color);
    ui.visuals_mut().widgets.hovered.fg_stroke = Stroke::new(1.0_f32, text_color);
    ui.visuals_mut().widgets.active.fg_stroke = Stroke::new(1.0_f32, text_color);
}

/// Label + dropdown field with a #252833 surface.
pub(crate) fn select_field<T>(
    ui: &mut Ui,
    palette: &ThemePalette,
    label: &str,
    value: &mut T,
    items: &[(T, &str)],
    field_hint: Option<&str>,
) -> Response
where
    T: Clone + PartialEq,
{
    field_label(ui, label, false, palette);
    let selected = items
        .iter()
        .find(|(candidate, _)| candidate == &*value)
        .map_or("Custom", |(_, title)| *title);
    let response = ui
        .scope(|ui| {
            let enabled = ui.is_enabled();
            let fill = if enabled {
                CONTROL
            } else {
                CONTROL.linear_multiply(0.5)
            };
            let text_color = if enabled { TEXT } else { PLACEHOLDER };
            apply_control_visuals(ui, fill, text_color);
            ui.spacing_mut().interact_size.y = FIELD_HEIGHT;
            let output = egui::ComboBox::from_id_salt(label)
                .selected_text(RichText::new(selected).color(text_color))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (candidate, title) in items {
                        ui.selectable_value(value, candidate.clone(), *title);
                    }
                });
            output.response
        })
        .inner;
    focus_ring(ui, &response, response.rect, CONTROL_RADIUS);
    if let Some(text) = field_hint {
        hint(ui, text);
    }
    response
}

/// Numeric field used for ports and dimensions.
pub(crate) fn drag_field<T>(
    ui: &mut Ui,
    palette: &ThemePalette,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    prefix: &str,
    required: bool,
    suffix: Option<&str>,
) -> Response
where
    T: egui::emath::Numeric,
{
    field_label(ui, label, required, palette);
    let enabled = ui.is_enabled();
    let fill = if enabled {
        INPUT
    } else {
        INPUT.linear_multiply(0.5)
    };
    let text_color = if enabled { TEXT } else { PLACEHOLDER };
    let response = ui
        .scope(|ui| {
            apply_control_visuals(ui, fill, text_color);
            let width = ui.available_width();
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(width, FIELD_HEIGHT), Sense::click_and_drag());
            ui.painter()
                .rect_filled(rect, Rounding::same(CONTROL_RADIUS), fill);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.set_clip_rect(rect);
            let value_text = format!("{prefix}{}", value.to_f64());
            let galley = child.fonts(|fonts| {
                fonts.layout_no_wrap(
                    value_text,
                    TextStyle::Body.resolve(child.style()),
                    text_color,
                )
            });
            child.painter().galley(
                egui::pos2(rect.left() + 11.0, rect.center().y - galley.size().y / 2.0),
                galley,
                text_color,
            );
            if response.dragged() {
                let delta = ui.input(|input| input.pointer.delta().x);
                let next = ((*value).to_f64() + f64::from(delta)).round();
                *value = T::from_f64(next.clamp(range.start().to_f64(), range.end().to_f64()));
            }
            focus_ring(ui, &response, rect, CONTROL_RADIUS);
            response
        })
        .inner;
    if let Some(text) = suffix {
        hint(ui, text);
    }
    response
}

/// 32x18 switch with a 14px knob and a label to its right.
pub(crate) fn switch(
    ui: &mut Ui,
    on: &mut bool,
    label: &str,
    helper: Option<&str>,
    palette: &ThemePalette,
) -> Response {
    let track = Vec2::new(32.0, 18.0);
    let enabled = ui.is_enabled();
    let (track_fill, knob_fill) = if !enabled {
        (
            if *on { ACCENT } else { SWITCH_OFF }.linear_multiply(0.5),
            Color32::WHITE.linear_multiply(0.5),
        )
    } else if *on {
        (accent(palette), Color32::WHITE)
    } else {
        (SWITCH_OFF, SWITCH_KNOB_OFF)
    };
    let knob_radius = 7.0;
    let label_color = if enabled { LABEL } else { PLACEHOLDER };
    let hint_color = if enabled { MUTED } else { PLACEHOLDER };
    let mut response = None;
    ui.horizontal(|ui| {
        let (rect, mut switch_response) = ui.allocate_exact_size(track, Sense::click());
        if switch_response.clicked()
            || (switch_response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter)))
        {
            *on = !*on;
            switch_response.mark_changed();
            ui.ctx().request_repaint();
        }
        let track_fill = if enabled && switch_response.hovered() {
            track_fill.gamma_multiply(0.88)
        } else {
            track_fill
        };
        ui.painter()
            .rect_filled(rect, Rounding::same(9.0), track_fill);
        let knob_x = if *on {
            rect.max.x - 2.0 - knob_radius
        } else {
            rect.min.x + 2.0 + knob_radius
        };
        ui.painter()
            .circle_filled(egui::pos2(knob_x, rect.center().y), knob_radius, knob_fill);
        focus_ring(ui, &switch_response, rect, 10.0);
        response = Some(switch_response);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(label).size(12.0).color(label_color));
            if let Some(text) = helper {
                ui.label(RichText::new(text).size(11.0).color(hint_color));
            }
        });
    });
    response.expect("switch row always allocates its control")
}

/// 35px borderless collapsible row with a right chevron.
pub(crate) fn collapsible_row<R>(
    ui: &mut Ui,
    _palette: &ThemePalette,
    open: &mut bool,
    label: &str,
    contents: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 35.0), Sense::click());
    let enabled = ui.is_enabled();
    let fill = if enabled {
        WINDOW
    } else {
        WINDOW.linear_multiply(0.5)
    };
    ui.painter()
        .rect_filled(rect, Rounding::same(CONTROL_RADIUS), fill);
    ui.allocate_new_ui(
        egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(12.0, 0.0))),
        |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(11.5).strong().color(if enabled {
                    TEXT
                } else {
                    PLACEHOLDER
                }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(if *open {
                            crate::lucide::CHEVRON_DOWN
                        } else {
                            crate::lucide::CHEVRON_RIGHT
                        })
                        .size(12.0)
                        .color(if enabled { MUTED } else { PLACEHOLDER }),
                    );
                });
            });
        },
    );
    let toggled = response.clicked()
        || (response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter)));
    if toggled {
        *open = !*open;
        ui.ctx().request_repaint();
    }
    focus_ring(ui, &response, rect, CONTROL_RADIUS);
    if *open {
        ui.add_space(8.0);
        Some(
            Frame::none()
                .fill(fill)
                .rounding(Rounding::same(CONTROL_RADIUS))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    contents(ui)
                })
                .inner,
        )
    } else {
        None
    }
}

/// Two equal columns when the card has at least 440px of content width.
pub(crate) fn two_fields<A, B>(
    ui: &mut Ui,
    first: impl FnOnce(&mut Ui) -> A,
    second: impl FnOnce(&mut Ui) -> B,
) -> (A, B) {
    let width = ui.available_width();
    if width + 32.0 < 440.0 {
        let a = first(ui);
        ui.add_space(12.0);
        let b = second(ui);
        return (a, b);
    }

    let gap = 14.0;
    let column = (width - gap) / 2.0;
    let mut a_result = None;
    let mut b_result = None;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui_with_layout(
            Vec2::new(column, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(column);
                a_result = Some(first(ui));
            },
        );
        let (gap_rect, _) = ui.allocate_exact_size(Vec2::new(gap, 1.0), Sense::hover());
        let _ = gap_rect;
        ui.allocate_ui_with_layout(
            Vec2::new(column, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(column);
                b_result = Some(second(ui));
            },
        );
    });
    (
        a_result.expect("first column closure ran"),
        b_result.expect("second column closure ran"),
    )
}

fn button(
    ui: &mut Ui,
    text: impl Into<WidgetText>,
    fill: Color32,
    text_color: Color32,
) -> Response {
    let enabled = ui.is_enabled();
    ui.add_enabled(
        enabled,
        egui::Button::new(
            text.into()
                .color(if enabled { text_color } else { PLACEHOLDER }),
        )
        .fill(if enabled {
            fill
        } else {
            fill.linear_multiply(0.5)
        })
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(CONTROL_RADIUS))
        .min_size(Vec2::new(72.0, 29.0)),
    )
}

pub(crate) fn secondary_button(
    ui: &mut Ui,
    text: impl Into<WidgetText>,
    palette: &ThemePalette,
) -> Response {
    let _ = palette;
    button(ui, text, CONTROL, TEXT)
}

pub(crate) fn destructive_button(
    ui: &mut Ui,
    text: impl Into<WidgetText>,
    palette: &ThemePalette,
) -> Response {
    let _ = palette;
    button(ui, text, DANGER_BG, DANGER)
}

pub(crate) fn compact_button_widget_enabled(
    text: impl Into<WidgetText>,
    enabled: bool,
) -> egui::Button<'static> {
    egui::Button::new(text.into().color(if enabled { TEXT } else { PLACEHOLDER }))
        .fill(if enabled {
            CONTROL
        } else {
            CONTROL.linear_multiply(0.5)
        })
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(CONTROL_RADIUS))
        .min_size(Vec2::new(88.0, 29.0))
}

pub(crate) fn compact_button(
    ui: &mut Ui,
    text: impl Into<WidgetText>,
    _palette: &ThemePalette,
) -> Response {
    let enabled = ui.is_enabled();
    ui.add_enabled(enabled, compact_button_widget_enabled(text, enabled))
}

/// Status of a connection test. `None` leaves the status slot empty.
pub(crate) enum TestStatus<'a> {
    None,
    Untested,
    Checking,
    Unavailable(&'a str),
    Success(String),
    Failure { text: String, tooltip: String },
}

/// Secondary test button plus an optional one-line monospace status.
pub(crate) fn test_status(
    ui: &mut Ui,
    palette: &ThemePalette,
    button_text: WidgetText,
    enabled: bool,
    status: TestStatus<'_>,
    on_test: impl FnOnce(),
) {
    let enabled = enabled && ui.is_enabled();
    ui.horizontal_wrapped(|ui| {
        let response = ui.add_enabled(
            enabled,
            egui::Button::new(button_text.color(if enabled { TEXT } else { PLACEHOLDER }))
                .fill(if enabled {
                    CONTROL
                } else {
                    CONTROL.linear_multiply(0.5)
                })
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(CONTROL_RADIUS))
                .min_size(Vec2::new(140.0, 29.0)),
        );
        if response.clicked() {
            on_test();
        }
        focus_ring(ui, &response, response.rect, CONTROL_RADIUS);
        let (dot_color, text, text_color, tooltip) = match status {
            TestStatus::None => return,
            TestStatus::Checking => (WARNING, "Testing…".to_owned(), MUTED, None),
            TestStatus::Untested => (SUCCESS, "Ready to register".to_owned(), MUTED, None),
            TestStatus::Unavailable(reason) => (WARNING, reason.to_owned(), WARNING, None),
            TestStatus::Success(message) => (SUCCESS, message.clone(), SUCCESS, Some(message)),
            TestStatus::Failure { text, tooltip } => (DANGER, text, DANGER, Some(tooltip)),
        };
        ui.label(RichText::new("●").size(9.0).color(dot_color));
        let response =
            ui.add(Label::new(RichText::new(text).size(11.0).monospace().color(text_color)).wrap());
        if let Some(tip) = tooltip {
            response.on_hover_text(tip);
        }
    });
    let _ = palette;
}

/// Paint a 6px fading overlay scrollbar for a hidden egui scroll area.
pub(crate) fn paint_overlay_scrollbar(
    ui: &Ui,
    output: &egui::containers::scroll_area::ScrollAreaOutput<()>,
) {
    let viewport = output.inner_rect;
    let content_height = output.content_size.y;
    let view_height = viewport.height();
    if content_height <= view_height + 1.0 || view_height <= 0.0 {
        return;
    }
    for step in 0..10 {
        let alpha = (150.0 * (1.0 - step as f32 / 10.0)) as u8;
        let top = Rect::from_min_max(
            egui::pos2(viewport.left(), viewport.top() + step as f32),
            egui::pos2(viewport.right(), viewport.top() + step as f32 + 1.0),
        );
        let bottom = Rect::from_min_max(
            egui::pos2(viewport.left(), viewport.bottom() - step as f32 - 1.0),
            egui::pos2(viewport.right(), viewport.bottom() - step as f32),
        );
        let fade = Color32::from_rgba_unmultiplied(WINDOW.r(), WINDOW.g(), WINDOW.b(), alpha);
        ui.painter().rect_filled(top, 0.0, fade);
        ui.painter().rect_filled(bottom, 0.0, fade);
    }

    let track = Rect::from_min_max(
        egui::pos2(viewport.right() - 10.0, viewport.top()),
        egui::pos2(viewport.right() - 4.0, viewport.bottom()),
    );
    let max_offset = (content_height - view_height).max(1.0);
    let thumb_height =
        ((view_height / content_height) * view_height).clamp(28.0, view_height.max(28.0));
    let progress = (output.state.offset.y / max_offset).clamp(0.0, 1.0);
    let thumb = Rect::from_min_size(
        egui::pos2(
            track.left(),
            viewport.top() + progress * (view_height - thumb_height),
        ),
        Vec2::new(6.0, thumb_height),
    );

    let pointer_over_track = ui.input(|input| {
        input
            .pointer
            .latest_pos()
            .is_some_and(|position| track.contains(position))
    });
    let fade = ui
        .ctx()
        .animate_bool_responsive(output.id, pointer_over_track);
    let color = if pointer_over_track {
        THUMB_HOVER
    } else {
        SWITCH_OFF
    };
    let alpha = 0.15 + 0.85 * fade;
    ui.painter().rect_filled(
        thumb,
        Rounding::same(3.0),
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (255.0 * alpha) as u8),
    );
}

/// Apply the dialog's compact base text sizes to a `Ui`.
pub(crate) fn apply_text_styles(ui: &mut Ui) {
    ui.style_mut()
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(12.0));
    ui.style_mut()
        .text_styles
        .insert(TextStyle::Button, egui::FontId::proportional(11.5));
    ui.style_mut().text_styles.remove(&TextStyle::Small);
    ui.style_mut()
        .text_styles
        .insert(TextStyle::Monospace, egui::FontId::monospace(11.0));
}
