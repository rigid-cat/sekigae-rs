use super::*;

pub(super) fn install_japanese_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "noto_sans_jp".to_owned(),
        FontData::from_static(include_bytes!("../fonts/UDEVGothic35NFLG-Regular.ttf")).into(),
    );

    if let Some(proportional) = fonts.families.get_mut(&FontFamily::Proportional) {
        proportional.insert(0, "noto_sans_jp".to_owned());
    }

    if let Some(monospace) = fonts.families.get_mut(&FontFamily::Monospace) {
        monospace.push("noto_sans_jp".to_owned());
    }

    ctx.set_fonts(fonts);
}

impl eframe::App for SekigaeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_text_style(ctx);
        self.poll_solver_result(ctx);

        let mut should_fullscreen =
            self.current_stage == UiStage::SolveExport && self.result_fullscreen;
        if should_fullscreen && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.exit_result_fullscreen();
            should_fullscreen = false;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(should_fullscreen));

        // 結果表示中はアニメーションが進行中のため、毎フレーム再描画
        if self.result.is_some() {
            ctx.request_repaint();
        }

        if should_fullscreen {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(8.0);
                self.render_fullscreen_result_view(ctx, ui);
            });
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("main-page-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    let mut reset_all = false;
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.heading("sekigae-rs");
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                            reset_all = ui
                                .add_enabled(!self.is_solving, Button::new("すべてリセット"))
                                .clicked();

                            ui.add_space(10.0);
                            ui.group(|ui| {
                                self.render_message_area(ui);
                            });
                        });
                    });

                    if reset_all {
                        self.reset_all(ctx);
                    }

                    ui.separator();
                    self.render_stage_navigation(ui);

                    ui.separator();

                    ui.add_space(32.0);

                    match self.current_stage {
                        UiStage::Setup => self.render_setup_stage(ui),
                        UiStage::Students => self.render_students_stage(ui),
                        UiStage::Targets => self.render_targets_stage(ui),
                        UiStage::SolveExport => self.render_solve_export_stage(ctx, ui),
                    }
                });
        });
    }
}
