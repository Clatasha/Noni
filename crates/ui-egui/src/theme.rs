//! BananaV's look: the colors of each interface type of Options ›
//! Preferences › Display (Dark, Medium, Light, White), banana yellow as the
//! accent, and the egui style made from them. The colors are read through
//! functions (`theme::panel()`), so a change of interface type shows at
//! once.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

use crate::egui::{self, Color32, CornerRadius, Stroke};
use crate::i18n::{tr, trc};

/// Options › Preferences › Display › "Interface Type".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interface {
    #[default]
    Dark,
    Medium,
    Light,
    White,
}

impl Interface {
    pub const ALL: [Interface; 4] = [
        Interface::Dark,
        Interface::Medium,
        Interface::Light,
        Interface::White,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Interface::Dark => tr("Dark"),
            Interface::Medium => tr("Medium"),
            Interface::Light => trc("interface", "Light"),
            Interface::White => tr("White"),
        }
    }

    pub fn palette(self) -> &'static Palette {
        match self {
            Interface::Dark => &DARK,
            Interface::Medium => &MEDIUM,
            Interface::Light => &LIGHT,
            Interface::White => &WHITE,
        }
    }
}

/// A visual skin is independent of Classic's existing interface brightness.
/// Choosing Premium never changes a saved Classic Interface Type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeVariant {
    #[default]
    Classic,
    PremiumDark,
    PremiumLight,
}

impl ThemeVariant {
    pub const ALL: [Self; 3] = [Self::Classic, Self::PremiumDark, Self::PremiumLight];

    pub fn name(self) -> &'static str {
        match self {
            Self::Classic => tr("Noni Classic"),
            Self::PremiumDark => tr("Premium Dark"),
            Self::PremiumLight => tr("Premium Light"),
        }
    }

    pub fn palette(self, classic: Interface) -> &'static Palette {
        match self {
            Self::Classic => classic.palette(),
            Self::PremiumDark => &PREMIUM_DARK,
            Self::PremiumLight => &PREMIUM_LIGHT,
        }
    }
}

/// Primary action buttons, including Export.
pub(crate) const ACTION_BLUE: Color32 = Color32::from_rgb(24, 100, 214);

/// The colors of an interface type.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Dark text on light panels.
    pub light: bool,
    pub banana: Color32,
    pub banana_dim: Color32,
    pub bg: Color32,
    pub panel: Color32,
    pub panel_dark: Color32,
    pub hover: Color32,
    pub separator: Color32,
    pub knob: Color32,
    pub track_video: Color32,
    pub track_audio: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    /// Text that stands out (a selected row's).
    pub text_strong: Color32,
    pub cursor: Color32,
    /// The accent of open tabs and focused lists.
    pub snap: Color32,
    pub loop_bar: Color32,
    pub marker: Color32,
    pub region: Color32,
    pub error: Color32,
    /// A track header that is selected, and selected with the keys on its
    /// list.
    pub header_selected: Color32,
    pub header_selected_focused: Color32,
    /// The open tab of a tabbed window.
    pub tab_on: Color32,
    /// The selected row of lists and the chosen category of the plug-in
    /// windows (VEGAS's blue).
    pub selection: Color32,
    pub check_border: Color32,
    /// Faint lines: rows of long lists.
    pub faint_line: Color32,
    /// The timeline's dotted grid lines (VEGAS's #505050 on its dark
    /// timeline).
    pub grid: Color32,
    /// Buttons and other widgets: at rest, under the pointer, open; their
    /// text.
    pub widget: Color32,
    pub widget_hover: Color32,
    pub widget_open: Color32,
    pub widget_text: Color32,
}

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Neutral greys, as VEGAS's dark interface (its panels #282828, the bars
/// between them #141414, the timeline #1E1E1E).
const DARK: Palette = Palette {
    light: false,
    banana: rgb(0xF7C948),
    banana_dim: rgb(0x8A7022),
    bg: rgb(0x1E1E1E),
    panel: rgb(0x282828),
    panel_dark: rgb(0x141414),
    hover: rgb(0x3A3A3A),
    separator: rgb(0x3C3C3C),
    knob: rgb(0xBBBBBB),
    track_video: rgb(0x292929),
    track_audio: rgb(0x262927),
    text: rgb(0xE6E6E6),
    text_dim: rgb(0x9A9A9A),
    text_disabled: rgb(0x5A5A5A),
    text_strong: rgb(0xFFFFFF),
    cursor: rgb(0xFFFFFF),
    snap: rgb(0x5CC8FF),
    loop_bar: rgb(0x6C8ED8),
    marker: rgb(0xF09A3E),
    region: rgb(0x5DC07A),
    error: rgb(0xFF7A6E),
    header_selected: rgb(0x3E3E3E),
    header_selected_focused: rgb(0x34465E),
    tab_on: rgb(0x2C6E8F),
    selection: rgb(0x3C8CDC),
    check_border: rgb(0x8C8C8C),
    faint_line: Color32::from_rgba_premultiplied(14, 14, 14, 14),
    grid: rgb(0x505050),
    widget: rgb(0x3C3C3C),
    widget_hover: rgb(0x464646),
    widget_open: rgb(0x2D2D2D),
    widget_text: rgb(0xB4B4B4),
};

const MEDIUM: Palette = Palette {
    light: false,
    banana: rgb(0xF7C948),
    banana_dim: rgb(0x9A7D26),
    bg: rgb(0x3E3E3E),
    panel: rgb(0x4B4B4B),
    panel_dark: rgb(0x363636),
    hover: rgb(0x5B5B5B),
    separator: rgb(0x646464),
    knob: rgb(0xD2D2D2),
    track_video: rgb(0x474747),
    track_audio: rgb(0x454946),
    text: rgb(0xF0F0F0),
    text_dim: rgb(0xBFBFBF),
    text_disabled: rgb(0x808080),
    text_strong: rgb(0xFFFFFF),
    cursor: rgb(0xFFFFFF),
    snap: rgb(0x6ED0FF),
    loop_bar: rgb(0x7E9EE6),
    marker: rgb(0xF5A54E),
    region: rgb(0x6FD08C),
    error: rgb(0xFF8B80),
    header_selected: rgb(0x606060),
    header_selected_focused: rgb(0x52647E),
    tab_on: rgb(0x3A7FA2),
    selection: rgb(0x3C8CDC),
    check_border: rgb(0xA8A8A8),
    faint_line: Color32::from_rgba_premultiplied(18, 18, 18, 18),
    grid: rgb(0x6E6E6E),
    widget: rgb(0x5E5E5E),
    widget_hover: rgb(0x6B6B6B),
    widget_open: rgb(0x4E4E4E),
    widget_text: rgb(0xDCDCDC),
};

const LIGHT: Palette = Palette {
    light: true,
    banana: rgb(0x5A3F00),
    banana_dim: rgb(0xD8B23E),
    bg: rgb(0x8F8F8F),
    panel: rgb(0xA0A0A0),
    panel_dark: rgb(0x888888),
    hover: rgb(0xB1B1B1),
    separator: rgb(0x757575),
    knob: rgb(0x474747),
    track_video: rgb(0x979797),
    track_audio: rgb(0x959995),
    text: rgb(0x111111),
    text_dim: rgb(0x323232),
    text_disabled: rgb(0x666666),
    text_strong: rgb(0x000000),
    cursor: rgb(0x000000),
    snap: rgb(0x175F92),
    loop_bar: rgb(0x3A5CB0),
    marker: rgb(0xB8620E),
    region: rgb(0x23804A),
    error: rgb(0x9E1C14),
    header_selected: rgb(0xB8B8B8),
    header_selected_focused: rgb(0x9DB2D2),
    tab_on: rgb(0x86BADA),
    selection: rgb(0x3C8CDC),
    check_border: rgb(0x474747),
    faint_line: Color32::from_black_alpha(26),
    grid: rgb(0x6E6E6E),
    widget: rgb(0xB3B3B3),
    widget_hover: rgb(0xC1C1C1),
    widget_open: rgb(0xA7A7A7),
    widget_text: rgb(0x151515),
};

const WHITE: Palette = Palette {
    light: true,
    banana: rgb(0x5A3F00),
    banana_dim: rgb(0xE6C458),
    bg: rgb(0xBFBFBF),
    panel: rgb(0xD0D0D0),
    panel_dark: rgb(0xB8B8B8),
    hover: rgb(0xE1E1E1),
    separator: rgb(0xA2A2A2),
    knob: rgb(0x505050),
    track_video: rgb(0xC4C4C4),
    track_audio: rgb(0xC2C6C3),
    text: rgb(0x111111),
    text_dim: rgb(0x3E3E3E),
    text_disabled: rgb(0x8A8A8A),
    text_strong: rgb(0x000000),
    cursor: rgb(0x000000),
    snap: rgb(0x175F92),
    loop_bar: rgb(0x3A5CB0),
    marker: rgb(0xB8620E),
    region: rgb(0x23804A),
    error: rgb(0x9E1C14),
    header_selected: rgb(0xE4E4E4),
    header_selected_focused: rgb(0xBACFEB),
    tab_on: rgb(0xA8D0E8),
    selection: rgb(0x3C8CDC),
    check_border: rgb(0x606060),
    faint_line: Color32::from_black_alpha(20),
    grid: rgb(0x9C9C9C),
    widget: rgb(0xDBDBDB),
    widget_hover: rgb(0xE8E8E8),
    widget_open: rgb(0xCFCFCF),
    widget_text: rgb(0x151515),
};

// Noni premium styles use its green accent and separated surface depths.
// Classic retains every original color without modification.
const PREMIUM_DARK: Palette = Palette {
    light: false,
    banana: rgb(0x6DC2FF),
    banana_dim: rgb(0x1759B1),
    bg: rgb(0x101722),
    panel: rgb(0x1A2432),
    panel_dark: rgb(0x0C121C),
    hover: rgb(0x2B4052),
    separator: rgb(0x344657),
    knob: rgb(0xC8D6DB),
    track_video: rgb(0x1C2836),
    track_audio: rgb(0x1B2B31),
    text: rgb(0xE7F1F2),
    text_dim: rgb(0xA0B4C0),
    text_disabled: rgb(0x647C8B),
    text_strong: rgb(0xFFFFFF),
    cursor: rgb(0xFFFFFF),
    snap: rgb(0x67C9F3),
    loop_bar: rgb(0x7E9BE5),
    marker: rgb(0xF0B66B),
    region: rgb(0x7BCFA1),
    error: rgb(0xFF918B),
    header_selected: rgb(0x26394A),
    header_selected_focused: rgb(0x245D63),
    tab_on: rgb(0x146DD3),
    selection: rgb(0x146DD3),
    check_border: rgb(0x8095A4),
    faint_line: Color32::from_black_alpha(28),
    grid: rgb(0x314354),
    widget: rgb(0x263548),
    widget_hover: rgb(0x344B60),
    widget_open: rgb(0x1B2B3D),
    widget_text: rgb(0xE7F1F2),
};

const PREMIUM_LIGHT: Palette = Palette {
    light: true,
    banana: rgb(0x125BA9),
    banana_dim: rgb(0x8EC7F6),
    bg: rgb(0xE7EDF1),
    panel: rgb(0xFBFDFE),
    panel_dark: rgb(0xDCE7E9),
    hover: rgb(0xE3F3EC),
    separator: rgb(0xC5D3D9),
    knob: rgb(0x4B6270),
    track_video: rgb(0xE7EEF2),
    track_audio: rgb(0xE3EFE8),
    text: rgb(0x182C39),
    text_dim: rgb(0x526875),
    text_disabled: rgb(0x94A6AE),
    text_strong: rgb(0x10232B),
    cursor: rgb(0x203746),
    snap: rgb(0x1479A8),
    loop_bar: rgb(0x5375BC),
    marker: rgb(0xAA6527),
    region: rgb(0x26885A),
    error: rgb(0xB22F32),
    header_selected: rgb(0xD4EAE1),
    header_selected_focused: rgb(0xB5DCCB),
    tab_on: rgb(0x9ED0FA),
    selection: rgb(0x62A8E9),
    check_border: rgb(0x79918D),
    faint_line: Color32::from_black_alpha(16),
    grid: rgb(0xB5C4CC),
    widget: rgb(0xEDF4F7),
    widget_hover: rgb(0xDDECE7),
    widget_open: rgb(0xD2E4E2),
    widget_text: rgb(0x1D3640),
};

/// The interface type shown (an index in [`Interface::ALL`]).
static INTERFACE: AtomicU8 = AtomicU8::new(0);
/// The Noni skin; zero preserves Classic for existing settings.
static THEME_VARIANT: AtomicU8 = AtomicU8::new(0);
/// Options › Preferences › Display › "Button Highlight Color" (RGBA).
static BUTTON_HIGHLIGHT: AtomicU32 = AtomicU32::new(u32::from_be_bytes(DEFAULT_HIGHLIGHT));
/// Options › Preferences › Display › "Icon color strength" (0–100 %).
static ICON_STRENGTH: AtomicU8 = AtomicU8::new(100);

/// BananaV's highlight of active toolbar buttons: dim banana.
pub const DEFAULT_HIGHLIGHT: [u8; 4] = [0x6C, 0x5A, 0x24, 0xFF];

pub fn set_interface(kind: Interface) {
    let i = Interface::ALL.iter().position(|&k| k == kind).unwrap_or(0);
    INTERFACE.store(i as u8, Ordering::Relaxed);
}

pub fn interface() -> Interface {
    Interface::ALL[INTERFACE.load(Ordering::Relaxed) as usize % Interface::ALL.len()]
}

pub fn set_variant(variant: ThemeVariant) {
    let index = ThemeVariant::ALL.iter().position(|&v| v == variant).unwrap_or(0);
    THEME_VARIANT.store(index as u8, Ordering::Relaxed);
}

pub fn variant() -> ThemeVariant {
    ThemeVariant::ALL[THEME_VARIANT.load(Ordering::Relaxed) as usize % ThemeVariant::ALL.len()]
}

pub fn set_button_highlight(rgba: [u8; 4]) {
    BUTTON_HIGHLIGHT.store(u32::from_be_bytes(rgba), Ordering::Relaxed);
}

/// The fill of an active toolbar button.
pub fn button_highlight() -> Color32 {
    let [r, g, b, a] = BUTTON_HIGHLIGHT.load(Ordering::Relaxed).to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn set_icon_strength(percent: u8) {
    ICON_STRENGTH.store(percent.min(100), Ordering::Relaxed);
}

/// `color` as icons show it: its saturation scaled by the icon color
/// strength (grey at 0 %).
pub fn icon_color(color: Color32) -> Color32 {
    desaturate(color, ICON_STRENGTH.load(Ordering::Relaxed))
}

/// `color` with its saturation scaled to `percent`: the color mixed with
/// its own grey (Rec. 709 luma), alpha kept.
pub fn desaturate(color: Color32, percent: u8) -> Color32 {
    if percent >= 100 {
        return color;
    }
    let k = percent as f32 / 100.0;
    let [r, g, b, a] = color.to_array();
    let luma = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    let mix = |c: u8| (luma + (c as f32 - luma) * k).round().clamp(0.0, 255.0) as u8;
    // Premultiplied channels stay within alpha: the grey of a premultiplied
    // color is premultiplied too.
    Color32::from_rgba_premultiplied(mix(r), mix(g), mix(b), a)
}

fn palette() -> &'static Palette {
    variant().palette(interface())
}

pub fn banana() -> Color32 {
    palette().banana
}
pub fn banana_dim() -> Color32 {
    palette().banana_dim
}
pub fn bg() -> Color32 {
    palette().bg
}
pub fn panel() -> Color32 {
    palette().panel
}
pub fn panel_dark() -> Color32 {
    palette().panel_dark
}
pub fn hover() -> Color32 {
    palette().hover
}
pub fn separator() -> Color32 {
    palette().separator
}
pub fn knob() -> Color32 {
    palette().knob
}
pub fn track_video() -> Color32 {
    palette().track_video
}
pub fn track_audio() -> Color32 {
    palette().track_audio
}
pub fn text() -> Color32 {
    palette().text
}
pub fn text_dim() -> Color32 {
    palette().text_dim
}
pub fn text_disabled() -> Color32 {
    palette().text_disabled
}
pub fn text_strong() -> Color32 {
    palette().text_strong
}
pub fn cursor() -> Color32 {
    palette().cursor
}
pub fn snap() -> Color32 {
    palette().snap
}
pub fn loop_bar() -> Color32 {
    palette().loop_bar
}
pub fn marker() -> Color32 {
    palette().marker
}
pub fn region() -> Color32 {
    palette().region
}
pub fn error() -> Color32 {
    palette().error
}
pub fn header_selected() -> Color32 {
    palette().header_selected
}
pub fn header_selected_focused() -> Color32 {
    palette().header_selected_focused
}
pub fn tab_on() -> Color32 {
    palette().tab_on
}
pub fn selection() -> Color32 {
    palette().selection
}
pub fn check_border() -> Color32 {
    palette().check_border
}
pub fn faint_line() -> Color32 {
    palette().faint_line
}

pub fn grid() -> Color32 {
    palette().grid
}

/// The edit cursor and the selected events while Auto Ripple is on.
pub fn ripple() -> Color32 {
    rgb(0xFF6A3D)
}
/// The outline of the selected events the user picked (clicked), as
/// VEGAS's: yellow in every look.
pub fn event_picked() -> Color32 {
    rgb(0xFFFF5A)
}
/// The outline of the events selected along with a picked one, for being
/// in its group: VEGAS's blue.
pub fn event_grouped() -> Color32 {
    rgb(0x3C8CDC)
}
/// The outline of the events linked to a selected one (sync links), as
/// VEGAS's: purple.
pub fn event_sync_linked() -> Color32 {
    rgb(0xB45AE6)
}
/// The speaker of audio generators' previews.
pub fn clip_audio() -> Color32 {
    rgb(0x2F7A64)
}
/// The bar over prerendered parts of the timeline.
pub fn prerendered() -> Color32 {
    rgb(0x3A86FF)
}


/// Whether the premium Noni widget language is active.
/// Classic continues to render its original controls and artwork.
pub fn premium() -> bool {
    variant() != ThemeVariant::Classic
}

/// A recessed or raised premium surface: soft drop shadow, glass inset,
/// subtle rim light and a cool border. Callers retain ownership of layout.
pub fn paint_premium_panel(painter: &egui::Painter, rect: egui::Rect, raised: bool) {
    if !premium() || rect.width() < 6.0 || rect.height() < 6.0 {
        return;
    }
    let p = palette();
    let rounding = CornerRadius::same(10);
    if raised {
        painter.rect_filled(
            rect.translate(egui::vec2(0.0, 4.0)),
            rounding,
            Color32::from_black_alpha(if p.light { 22 } else { 76 }),
        );
    }
    painter.rect_filled(rect, rounding, p.panel);
    let inset = rect.shrink(2.0);
    if inset.height() > 12.0 {
        let sheen = egui::Rect::from_min_max(
            inset.min,
            egui::pos2(inset.right(), inset.top() + inset.height() * 0.42),
        );
        painter.rect_filled(
            sheen,
            CornerRadius::same(7),
            Color32::from_white_alpha(if p.light { 18 } else { 9 }),
        );
    }
    painter.rect_stroke(
        rect,
        rounding,
        Stroke::new(1.0, p.separator),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            egui::pos2(rect.left() + 12.0, rect.top() + 1.5),
            egui::pos2(rect.right() - 12.0, rect.top() + 1.5),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(if p.light { 72 } else { 41 })),
    );
}

/// Draw a distinct, slightly elevated 3D control. Selected or primary
/// buttons use Noni's luminous blue rather than a flat selection rectangle.
pub fn paint_premium_button(
    painter: &egui::Painter,
    rect: egui::Rect,
    selected: bool,
    hovered: bool,
    primary: bool,
) {
    if !premium() || rect.width() < 8.0 || rect.height() < 8.0 {
        return;
    }
    let p = palette();
    let blue = selected || primary;
    let base = if blue {
        if hovered { rgb(0x127CF7) } else { rgb(0x075ECB) }
    } else if hovered {
        p.hover
    } else {
        p.widget
    };
    let border = if blue {
        if hovered { rgb(0x9DE5FF) } else { rgb(0x55ABFF) }
    } else {
        p.separator
    };
    let radius = CornerRadius::same(8);
    painter.rect_filled(
        rect.translate(egui::vec2(0.0, 3.0)),
        radius,
        Color32::from_black_alpha(if p.light { 28 } else { 90 }),
    );
    if blue {
        painter.rect_stroke(
            rect.expand(1.5),
            CornerRadius::same(9),
            Stroke::new(2.0, Color32::from_rgba_unmultiplied(29, 143, 255, if selected { 120 } else { 46 })),
            egui::StrokeKind::Inside,
        );
    }
    painter.rect_filled(rect, radius, base);
    let interior = rect.shrink(2.0);
    let upper = egui::Rect::from_min_max(
        interior.min,
        egui::pos2(interior.right(), interior.center().y),
    );
    painter.rect_filled(
        upper,
        CornerRadius::same(6),
        Color32::from_white_alpha(if blue { 47 } else if p.light { 60 } else { 20 }),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(if blue { 1.5 } else { 1.0 }, border),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            egui::pos2(rect.left() + 9.0, rect.top() + 2.0),
            egui::pos2(rect.right() - 9.0, rect.top() + 2.0),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(if blue { 132 } else { 45 })),
    );
}

/// A clip background with subtle edge lighting. The timeline's selection,
/// trim, fade, thumbnail and waveform rendering continues unchanged above it.
pub fn paint_premium_clip(
    painter: &egui::Painter,
    rect: egui::Rect,
    fill: Color32,
    selected: bool,
) {
    if !premium() {
        painter.rect_filled(rect, 3.0, fill);
        return;
    }
    if rect.width() < 3.0 || rect.height() < 3.0 {
        painter.rect_filled(rect, 1.0, fill);
        return;
    }
    let round = CornerRadius::same(5);
    painter.rect_filled(rect, round, fill);
    if rect.width() >= 10.0 && rect.height() >= 12.0 {
        // A highlight within the event, not outside its editable bounds.
        let top = egui::Rect::from_min_max(
            rect.min + egui::vec2(1.0, 1.0),
            egui::pos2(rect.right() - 1.0, rect.top() + rect.height().min(12.0)),
        );
        painter.rect_filled(
            top,
            CornerRadius::same(3),
            Color32::from_white_alpha(if selected { 42 } else { 25 }),
        );
        painter.line_segment(
            [
                egui::pos2(rect.left() + 4.0, rect.top() + 1.5),
                egui::pos2(rect.right() - 4.0, rect.top() + 1.5),
            ],
            Stroke::new(1.0, Color32::from_white_alpha(78)),
        );
        painter.line_segment(
            [
                egui::pos2(rect.left() + 4.0, rect.bottom() - 1.5),
                egui::pos2(rect.right() - 4.0, rect.bottom() - 1.5),
            ],
            Stroke::new(1.0, Color32::from_black_alpha(75)),
        );
    }
    painter.rect_stroke(
        rect,
        round,
        Stroke::new(
            1.0,
            if selected {
                snap().gamma_multiply(0.55)
            } else {
                separator().gamma_multiply(0.85)
            },
        ),
        egui::StrokeKind::Inside,
    );
}

/// A track header with recessed boundaries and a light-catching top edge.
/// Caller still draws track color, meters, lock/mute and other controls.
pub fn paint_premium_track_header(
    painter: &egui::Painter,
    rect: egui::Rect,
    fill: Color32,
    selected: bool,
) {
    if !premium() {
        painter.rect_filled(rect, 0.0, fill);
        return;
    }
    if rect.width() < 8.0 || rect.height() < 8.0 {
        painter.rect_filled(rect, 0.0, fill);
        return;
    }
    painter.rect_filled(rect, CornerRadius::same(5), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(1.0, if selected { snap() } else { separator() }),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            egui::pos2(rect.left() + 6.0, rect.top() + 1.5),
            egui::pos2(rect.right() - 6.0, rect.top() + 1.5),
        ],
        Stroke::new(
            1.0,
            if selected {
                snap().gamma_multiply(0.70)
            } else {
                Color32::from_white_alpha(32)
            },
        ),
    );
    painter.line_segment(
        [
            egui::pos2(rect.left() + 5.0, rect.bottom() - 1.5),
            egui::pos2(rect.right() - 5.0, rect.bottom() - 1.5),
        ],
        Stroke::new(1.0, Color32::from_black_alpha(90)),
    );
}

/// A clickable premium button that preserves the normal egui accessibility
/// label and returns the same Response as a conventional button.
pub fn premium_button(
    ui: &mut egui::Ui,
    label: &str,
    size: egui::Vec2,
    primary: bool,
) -> egui::Response {
    if !premium() {
        return ui.add_sized(size, egui::Button::new(label));
    }
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    paint_premium_button(ui.painter(), rect, false, response.hovered(), primary);
    let text_color = if primary { Color32::WHITE } else { palette().text };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(14.0),
        text_color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// Sets egui's style from the interface type shown.
pub fn apply(ctx: &egui::Context) {
    apply_kind(ctx, interface());
}

/// Sets egui's style from the interface type `kind`.
pub fn apply_kind(ctx: &egui::Context, kind: Interface) {
    crate::timecode_font::install(ctx);
    let skin = variant();
    let p = skin.palette(kind);
    let premium = skin != ThemeVariant::Classic;
    // One style slot, holding the visuals of the selected Noni theme.
    ctx.set_theme(egui::Theme::Dark);
    ctx.global_style_mut(|style| {
        let mut v = if p.light {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        v.panel_fill = p.panel;
        v.window_fill = p.panel;
        v.window_stroke = Stroke::new(1.0, p.separator);
        if premium {
            v.window_corner_radius = CornerRadius::same(12);
            v.menu_corner_radius = CornerRadius::same(9);
            v.window_shadow = egui::Shadow {
                offset: [0, 7],
                blur: 20,
                spread: 0,
                color: Color32::from_black_alpha(if p.light { 45 } else { 90 }),
            };
            v.popup_shadow = egui::Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: Color32::from_black_alpha(if p.light { 28 } else { 65 }),
            };
        }
        v.extreme_bg_color = p.panel_dark;
        v.faint_bg_color = p.bg;
        v.code_bg_color = p.panel_dark;
        v.error_fg_color = p.error;
        v.selection.bg_fill = p.banana_dim;
        v.selection.stroke = Stroke::new(1.0, p.banana);
        v.hyperlink_color = p.banana;
        let w = &mut v.widgets;
        w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
        w.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
        w.noninteractive.bg_fill = p.panel;
        w.noninteractive.weak_bg_fill = p.panel;
        w.inactive.weak_bg_fill = p.widget;
        w.inactive.bg_fill = p.widget;
        w.inactive.fg_stroke = Stroke::new(1.0, p.widget_text);
        w.hovered.weak_bg_fill = p.widget_hover;
        w.hovered.bg_fill = p.widget_hover;
        w.hovered.bg_stroke = Stroke::new(1.0, p.banana_dim);
        w.hovered.fg_stroke = Stroke::new(1.5, p.text_strong);
        w.active.bg_fill = p.banana_dim;
        w.active.weak_bg_fill = p.banana_dim;
        w.active.fg_stroke = Stroke::new(2.0, p.text_strong);
        w.open.weak_bg_fill = p.widget_open;
        w.open.bg_fill = p.panel;
        w.open.fg_stroke = Stroke::new(1.0, p.text);
        v.slider_trailing_fill = true;
        if premium {
            w.inactive.bg_stroke = Stroke::new(1.0, p.separator);
            w.hovered.bg_stroke = Stroke::new(1.5, p.snap);
            w.active.bg_stroke = Stroke::new(1.5, p.banana);
        }
        let r = CornerRadius::same(if premium { 8 } else { 3 });
        w.noninteractive.corner_radius = r;
        w.inactive.corner_radius = r;
        w.hovered.corner_radius = r;
        w.active.corner_radius = r;
        style.visuals = v;
        style.spacing.item_spacing = if premium {
            egui::vec2(8.0, 6.0)
        } else {
            egui::vec2(6.0, 4.0)
        };
        style.spacing.slider_width = 140.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_preserve_classic_brightness_and_round_trip_by_name() {
        for classic in Interface::ALL {
            assert_eq!(ThemeVariant::Classic.palette(classic).panel, classic.palette().panel);
        }
        for skin in ThemeVariant::ALL {
            let json = serde_json::to_string(&skin).unwrap();
            assert_eq!(serde_json::from_str::<ThemeVariant>(&json).unwrap(), skin);
        }
        assert_eq!(ThemeVariant::default(), ThemeVariant::Classic);
        assert!(ThemeVariant::PremiumLight.palette(Interface::Dark).light);
        assert!(!ThemeVariant::PremiumDark.palette(Interface::White).light);
    }

    #[test]
    fn grey_keeps_its_brightness_and_full_strength_keeps_the_color() {
        let red = Color32::from_rgb(0xE0, 0x40, 0x30);
        assert_eq!(desaturate(red, 100), red);
        let grey = desaturate(red, 0);
        let [r, g, b, a] = grey.to_array();
        assert_eq!((r, g), (g, b), "grey");
        assert_eq!(a, 255);
        let luma = 0.2126 * 224.0 + 0.7152 * 64.0 + 0.0722 * 48.0;
        assert!((r as f32 - luma).abs() <= 1.0);
        let half = desaturate(red, 50).to_array();
        assert!(half[0] < 0xE0 && half[0] > r, "halfway: {half:?}");
    }

    #[test]
    fn text_stands_out_from_the_panels_of_every_interface_type() {
        // Relative luminance contrast (WCAG) of text on panels.
        fn lum(c: Color32) -> f32 {
            let f = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            let [r, g, b, _] = c.to_array();
            0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
        }
        fn contrast(a: Color32, b: Color32) -> f32 {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        }
        for kind in Interface::ALL {
            let p = kind.palette();
            for (what, fg, bg, least) in [
                ("text on panels", p.text, p.panel, 4.5),
                ("text on the dark panels", p.text, p.panel_dark, 4.5),
                ("dim text on panels", p.text_dim, p.panel, 3.0),
                ("button text", p.widget_text, p.widget, 4.5),
                ("accent text", p.banana, p.panel, 3.0),
                ("the cursor on the tracks", p.cursor, p.track_video, 3.0),
            ] {
                let c = contrast(fg, bg);
                assert!(c >= least, "{kind:?}: {what} {c:.2}");
            }
        }
    }

    #[test]
    fn the_interface_type_is_kept_by_name() {
        for kind in Interface::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_json::from_str::<Interface>(&json).unwrap(), kind);
        }
        assert_eq!(serde_json::to_string(&Interface::Dark).unwrap(), "\"Dark\"");
    }
}
