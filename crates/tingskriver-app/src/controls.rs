//! Tingskriver controls: gpui-component's `Button`, `Input` and `Switch`
//! with the sizes, weights and shadows of the Tauri app
//! (`frontend/src/components/ui/button.tsx`, `input.tsx`, `switch.tsx`), plus
//! the segmented control (`ThemeSelector.tsx`) and the recording pill button
//! (`RecordingControls.tsx`).
//!
//! Pages build buttons with these helpers instead of `Button::new`, so every
//! button is content-width (`inline-flex`) and 44 px high. Colors come from
//! `theme::apply`; nothing here writes a literal value.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{div, prelude::*, App, ElementId, Entity, FontWeight, SharedString, Window};
use gpui_component::{
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    switch::Switch,
    Selectable, Sizable,
};
use gpui_kit_assets::IconName;

use crate::theme::{self, layout, shadow, space, typography, Palette};

/// The Tauri button variants (`buttonVariants` in `ui/button.tsx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// `default` / `blue`: primary fill with `shadow`.
    Primary,
    /// `outline`: `input` border on the background, `shadow-sm`, hover accent.
    Outline,
    /// `secondary`: secondary fill, hover at 80 %.
    Secondary,
    /// `ghost`: transparent, hover accent.
    Ghost,
    /// `destructive` / `red`.
    Danger,
    /// `green`.
    Success,
    /// `link`: primary text, underlined.
    Link,
}

fn with_variant(button: Button, variant: Variant) -> Button {
    match variant {
        Variant::Primary => button.primary().shadow(shadow::base()),
        Variant::Outline => button.shadow(shadow::sm()),
        Variant::Secondary => button.secondary().shadow(shadow::sm()),
        Variant::Ghost => button.ghost(),
        Variant::Danger => button.danger().shadow(shadow::sm()),
        Variant::Success => button.success(),
        Variant::Link => button.link(),
    }
}

/// A button as Tauri's default size: 44 px high, 16 px side padding, 14 px
/// medium text, 6 px corners, content width. It does not stretch in a
/// column (`self_start`); in a row with taller neighbours call
/// `.self_center()` on it.
pub fn button(id: impl Into<ElementId>, variant: Variant) -> Button {
    with_variant(Button::new(id), variant)
        .h(layout::CONTROL_HEIGHT)
        .px(space::LG)
        .font_weight(FontWeight::MEDIUM)
        .self_start()
}

/// Tauri's `size="sm"`: 32 px high, 12 px padding, 12 px text.
pub fn small_button(id: impl Into<ElementId>, variant: Variant) -> Button {
    with_variant(Button::new(id), variant)
        .xsmall()
        .h(layout::CONTROL_HEIGHT_SM)
        .px(space::MD)
        .font_weight(FontWeight::MEDIUM)
        .self_start()
}

/// Tauri's `size="icon"`: a 36 px square with a 16 px icon. `label` is the
/// accessible name and the tooltip, since the button has no visible text.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    variant: Variant,
) -> Button {
    let label = label.into();
    with_variant(Button::new(id), variant)
        .icon(icon)
        .accessibility_label(label.clone())
        .tooltip(label)
        .size(layout::ICON_BUTTON)
        .self_start()
}

/// The 32 px icon button of list rows (rename and delete on a meeting).
pub fn small_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
) -> Button {
    icon_button(id, icon, label, Variant::Ghost).size(layout::ICON_BUTTON_SM)
}

/// A text input 36 px high (`h-9`), 14 px text and an `input` border.
pub fn input(state: &Entity<InputState>) -> Input {
    Input::new(state).h(layout::INPUT_HEIGHT)
}

/// A switch: 36×20 track and 16 px thumb, `primary` when on and `muted`
/// when off (the colors are set in `theme::apply`).
pub fn switch(id: impl Into<ElementId>) -> Switch {
    Switch::new(id)
}

/// One option of a [`segmented`] control.
#[derive(Debug, Clone)]
pub struct Segment {
    pub label: SharedString,
    pub icon: Option<IconName>,
}

impl Segment {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Segment {
            label: label.into(),
            icon: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// A segmented control (Udseende: Følg systemet / Lyst / Mørkt): a card with
/// an `input` border and 2 px padding, 36 px items, the selected one filled
/// with `primary`. `label` names the radio group for screen readers; each
/// item is a radio button whose checked state follows `selected`.
pub fn segmented(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    segments: Vec<Segment>,
    selected: usize,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let palette = palette(cx);
    let id = id.into();
    let on_select = Rc::new(on_select);
    h_flex()
        .id(id.clone())
        .role(gpui::Role::RadioGroup)
        .aria_label(label.into())
        .gap(space::XXS)
        .p(space::XXS)
        .rounded(theme::RADIUS_LG)
        .border_1()
        .border_color(palette.input)
        .bg(palette.card)
        .children(segments.into_iter().enumerate().map(|(index, segment)| {
            let active = index == selected;
            let on_select = on_select.clone();
            let item = Button::new(ElementId::NamedChild(
                Arc::new(id.clone()),
                SharedString::from(index.to_string()),
            ))
            .label(segment.label)
            .role(gpui::Role::RadioButton)
            .when_some(segment.icon, |this, icon| this.icon(icon))
            .when(active, |this| this.primary())
            .when(!active, |this| this.ghost())
            .selected(active)
            .toggled(active)
            .h(layout::INPUT_HEIGHT)
            .px(space::MD)
            .on_click(move |_, window, cx| on_select(index, window, cx));
            div().child(item)
        }))
}

/// The buttons of the floating recording pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pill {
    /// "Start optagelse": primary fill.
    Start,
    /// "Pause" / "Genoptag": outline on the card.
    Pause,
    /// "Stop og gem": foreground fill with background text.
    Stop,
}

/// A fully rounded 44 px pill button with 20 px padding and 14 px semibold
/// text (`RecordingControls.tsx`).
pub fn pill_button(id: impl Into<ElementId>, pill: Pill, cx: &App) -> Button {
    let palette = palette(cx);
    let button = Button::new(id);
    let button = match pill {
        Pill::Start => button.primary().shadow(shadow::base()),
        Pill::Pause => button.bg(palette.card).shadow(shadow::sm()),
        Pill::Stop => button
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(palette.foreground)
                    .foreground(palette.background)
                    .hover(theme::over(
                        palette.foreground,
                        palette.background,
                        theme::HOVER_ALPHA,
                    ))
                    .active(theme::over(
                        palette.foreground,
                        palette.background,
                        theme::PRESSED_ALPHA,
                    )),
            )
            // A custom variant draws a tint; the pill is a solid fill.
            .bg(palette.foreground)
            .shadow(shadow::base()),
    };
    button
        .rounded(ButtonRounded::Size(theme::RADIUS_FULL))
        .h(layout::CONTROL_HEIGHT)
        .px(space::XL2)
        .text_size(typography::SM)
        .font_weight(FontWeight::SEMIBOLD)
        .self_start()
}

fn palette(cx: &App) -> &'static Palette {
    use gpui_component::ActiveTheme;
    Palette::for_mode(cx.theme().mode)
}
