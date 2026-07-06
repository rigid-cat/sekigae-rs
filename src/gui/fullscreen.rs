use super::*;

impl SekigaeApp {
    pub(super) fn exit_result_fullscreen(&mut self) {
        self.result_fullscreen = false;
        self.animation_displayed_indices.clear();
        self.animation_last_update = Instant::now();
    }

    pub(super) fn render_fullscreen_result_toolbar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut run_solver = false;

        ui.horizontal_wrapped(|ui| {
            run_solver |= self.render_solver_button(ui);
            ui.separator();
            self.render_result_display_mode_selector_compact(ui);
            ui.separator();

            if ui
                .add(
                    Button::new(RichText::new("戻る").strong().color(Color32::WHITE))
                        .fill(Color32::from_rgb(180, 50, 50))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(120, 30, 30)))
                        .min_size(egui::vec2(100.0, 34.0)),
                )
                .clicked()
            {
                self.exit_result_fullscreen();
            }
        });

        run_solver
    }

    pub(super) fn render_fullscreen_result_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let run_solver = self.render_fullscreen_result_toolbar(ui);
        ui.add_space(6.0);
        self.render_current_result(ui, true);

        if run_solver {
            self.run_solver(ctx);
        }
    }
}
