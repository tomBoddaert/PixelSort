use eframe::{
    egui::{
        Button, Checkbox, DragValue, NumExt, PointerButton, Pos2, Rangef, Rect, Response, Sense,
        TextStyle, Ui, Vec2, Widget, lerp, pos2, remap_clamp, style, vec2,
    },
    epaint,
};
use pixel_sort::config::{Config, ConfigBuffer, Sort};

pub struct Settings {
    values: Vec<(u8, bool, bool)>,
    selected: Option<usize>,
}

pub struct SettingsWidget<'a> {
    values: &'a mut Vec<(u8, bool, bool)>,
    selected: &'a mut Option<usize>,
    dummy: (u8, bool, bool),
}

impl Settings {
    pub fn new() -> Self {
        Self {
            values: vec![(0, false, false), (u8::MAX / 2 + 1, true, false)],
            selected: Some(1),
        }
    }

    pub const fn widget(&mut self) -> SettingsWidget<'_> {
        let Self { values, selected } = self;
        SettingsWidget {
            values,
            selected,
            dummy: (0, false, false),
        }
    }

    pub fn get_config(&self) -> Config {
        let mut config = ConfigBuffer::new();
        for (value, sort, reversed) in self.values.iter().copied() {
            config
                .insert(
                    value,
                    match (sort, reversed) {
                        (true, false) => Sort::Increasing,
                        (true, true) => Sort::Decreasing,
                        (false, _) => Sort::None,
                    },
                )
                .unwrap();
        }
        config.finish()
    }
}

impl SettingsWidget<'_> {
    // Based on https://github.com/emilk/egui/blob/db63b6d1765552b345491ae4ee5894de8440cd44/crates/egui/src/widgets/slider_core.rs#L288-L296
    fn desired_size(&self, ui: &Ui) -> Vec2 {
        let thickness = ui
            .text_style_height(&TextStyle::Body)
            .at_least(ui.spacing().interact_size.y);
        vec2(ui.spacing().slider_width, thickness)
    }
}

// Based on:
// - https://github.com/emilk/egui/blob/db63b6d1765552b345491ae4ee5894de8440cd44/crates/egui/src/widgets/slider_core.rs#L382-L394
// - https://github.com/emilk/egui/blob/db63b6d1765552b345491ae4ee5894de8440cd44/crates/egui/src/widgets/slider_core.rs#L95-L98
fn paint_rail(ui: &Ui, root_rect: &Rect) -> Rect {
    let rail_radius = (ui.spacing().slider_rail_height / 2.).at_least(0.);
    let rail_rect = Rect::from_min_max(
        pos2(root_rect.left(), root_rect.center().y - rail_radius),
        pos2(root_rect.right(), root_rect.center().y + rail_radius),
    );
    let widget_visuals = &ui.visuals().widgets;

    ui.painter().rect_filled(
        rail_rect,
        widget_visuals.inactive.corner_radius,
        widget_visuals.inactive.bg_fill,
    );

    rail_rect
}

// Based on https://github.com/emilk/egui/blob/db63b6d1765552b345491ae4ee5894de8440cd44/crates/egui/src/widgets/slider_core.rs#L423-L446
fn paint_handle(
    ui: &Ui,
    handle_radius: f32,
    center: Pos2,
    visuals: &style::WidgetVisuals,
    active: bool,
) {
    ui.painter().add(epaint::CircleShape {
        center,
        radius: handle_radius + visuals.expansion,
        fill: if active {
            visuals.bg_fill
        } else {
            visuals.weak_bg_fill
        },
        stroke: if active {
            visuals.fg_stroke
        } else {
            visuals.bg_stroke
        },
    });
}

fn value_at_pointer(pointer_position_1d: f32, position_range: Rangef) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "value is clamped to 0..=255 and must be converted to a u8"
    )]
    {
        remap_clamp(
            pointer_position_1d,
            position_range,
            0.0..=f32::from(u8::MAX),
        )
        .round() as u8
    }
}

pub fn normalised_to_u8(value: f32) -> u8 {
    debug_assert!((0.0..=1.0).contains(&value));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "value should be in 0..=1 and must be converted to a u8"
    )]
    {
        (value * f32::from(u8::MAX)).round() as u8
    }
}

pub fn u8_to_normalised(value: u8) -> f32 {
    f32::from(value) / f32::from(u8::MAX)
}

impl SettingsWidget<'_> {
    fn slider_ui(&mut self, ui: &mut Ui) -> Response {
        let mut response = ui.allocate_response(self.desired_size(ui), Sense::click_and_drag());

        let handle_radius = response.rect.height() / 2.5;
        let position_range = response.rect.x_range().shrink(handle_radius);
        let scaled_handle_radius = handle_radius / position_range.span();

        if let Some(select_position) = response.interact_pointer_pos().and_then(|pos| {
            if response.clicked() {
                Some(pos)
            } else if response.drag_started() {
                Some(pos - response.drag_delta())
            } else {
                None
            }
        }) {
            let selected_value = value_at_pointer(select_position.x, position_range);
            *self.selected = self
                .values
                .iter()
                .enumerate()
                .fold(None, |acc, (i, (value, _, _))| {
                    let distance = selected_value.abs_diff(*value);
                    if distance > normalised_to_u8(scaled_handle_radius) {
                        acc
                    } else if let Some((_, acc_distance)) = acc
                        && acc_distance < distance
                    {
                        acc
                    } else {
                        Some((i, distance))
                    }
                })
                .map(|(i, _)| i);
        }

        if response.dragged_by(PointerButton::Primary)
            && let Some(i) = *self.selected
            && let Some(pointer_position_2d) = response.interact_pointer_pos()
        {
            let new_value = value_at_pointer(pointer_position_2d.x, position_range);
            let previous_value = std::mem::replace(&mut self.values[i].0, new_value);
            if previous_value != new_value {
                response.mark_changed();
            }
        }

        if ui.is_rect_visible(response.rect) {
            let rail_rect = paint_rail(ui, &response.rect);

            let visuals = ui.style().interact(&response);

            for (i, (value, _, _)) in self.values.iter().enumerate() {
                let center = pos2(
                    lerp(position_range, u8_to_normalised(*value)),
                    rail_rect.center().y,
                );
                paint_handle(
                    ui,
                    handle_radius,
                    center,
                    visuals,
                    Some(i) == *self.selected,
                );
            }
        }

        response
    }

    fn value_ui(&mut self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let old_formatter = style.number_formatter.clone();

        ui.add_enabled(
            self.selected.is_some(),
            DragValue::new(
                self.selected
                    .map_or(&mut self.dummy.0, |i| &mut self.values[i].0),
            )
            .range(0..=u8::MAX)
            .custom_formatter(move |value, decimals| {
                old_formatter.format(value / f64::from(u8::MAX), decimals)
            })
            .custom_parser(|string| {
                string
                    .parse::<f64>()
                    .ok()
                    .map(|value| value * f64::from(u8::MAX))
            })
            .speed(0.005 * f64::from(u8::MAX))
            .fixed_decimals(2)
            .clip_text(true),
        )
    }

    fn add_delete_ui(&mut self, ui: &mut Ui) -> Response {
        let inner_response = ui.vertical(|ui| {
            let mut add_response = ui.add_enabled(
                self.values.len() < ConfigBuffer::CAPACITY,
                Button::new("+").small(),
            );
            if add_response.clicked() {
                let (value, sort, reversed) = if let Some(i) = *self.selected {
                    let (current, sort, reversed) = self.values[i];
                    let next = self
                        .values
                        .iter()
                        .map(|(value, _, _)| value)
                        .copied()
                        .filter(|value| *value > current)
                        .min()
                        .unwrap_or(u8::MAX);

                    (current.midpoint(next), sort, reversed)
                } else {
                    let (last, sort, reversed) =
                        self.values.last().copied().unwrap_or((0, true, false));

                    (last.midpoint(u8::MAX), sort, reversed)
                };

                let new_index = self.values.len();
                self.values.push((value, sort, reversed));
                *self.selected = Some(new_index);

                add_response.mark_changed();
            }

            let mut delete_response =
                ui.add_enabled(self.selected.is_some(), Button::new("−").small());
            if delete_response.clicked() {
                let (current, _, _) = self.values.swap_remove(self.selected.unwrap());
                *self.selected = self
                    .values
                    .iter()
                    .map(|(value, _, _)| value)
                    .copied()
                    .enumerate()
                    .filter(|(_, value)| *value <= current)
                    .max_by_key(|(_, value)| *value)
                    .map(|(i, _)| i);

                delete_response.mark_changed();
            }

            add_response | delete_response
        });

        inner_response.inner | inner_response.response
    }

    fn switch_ui(&mut self, ui: &mut Ui) -> Response {
        let inner_response = ui.horizontal(|ui| {
            let enabled = self.selected.is_some();
            let value_sort = self
                .selected
                .map_or(&mut self.dummy, |i| &mut self.values[i]);
            ui.add_enabled(enabled, Checkbox::new(&mut value_sort.1, "Sorted"))
                | ui.add_enabled(enabled, Checkbox::new(&mut value_sort.2, "Reversed"))
        });

        inner_response.inner | inner_response.response
    }
}

impl Widget for SettingsWidget<'_> {
    // Based on:
    // - https://github.com/emilk/egui/blob/db63b6d1765552b345491ae4ee5894de8440cd44/crates/egui/src/widgets/slider.rs#L587-L657
    // - https://github.com/emilk/egui/blob/db63b6d1765552b345491ae4ee5894de8440cd44/crates/egui/src/widgets/slider.rs#L529-L573
    fn ui(mut self, ui: &mut Ui) -> Response {
        let inner_response = ui.vertical(|ui| {
            let inner_response =
                ui.horizontal(|ui| self.slider_ui(ui) | self.value_ui(ui) | self.add_delete_ui(ui));
            inner_response.inner | inner_response.response | self.switch_ui(ui)
        });

        inner_response.inner | inner_response.response
    }
}
