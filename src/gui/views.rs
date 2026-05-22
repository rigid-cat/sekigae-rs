use eframe::egui::CollapsingHeader;

use super::*;

impl SekigaeApp {
    pub(super) fn render_stage_navigation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (idx, stage) in UiStage::ALL.iter().enumerate() {
                let selected = *stage == self.current_stage;
                let label = format!("{}. {}", idx + 1, stage.title());
                if ui.selectable_label(selected, label).clicked() {
                    self.current_stage = *stage;
                }
            }
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let stage_index = self.current_stage_index();

            if ui
                .add_enabled(stage_index > 0, Button::new("← 前のステージ"))
                .clicked()
            {
                self.go_prev_stage();
            }

            if ui
                .add_enabled(
                    stage_index + 1 < UiStage::ALL.len(),
                    Button::new("次のステージ →"),
                )
                .clicked()
            {
                self.go_next_stage();
            }

            ui.separator();
            ui.label(self.current_stage.description());
        });
    }

    pub(super) fn render_message_area(&self, ui: &mut egui::Ui) {
        if self.is_solving {
            ui.colored_label(Color32::from_rgb(200, 120, 20), "席替え中...");
        }
        if let Some(info) = &self.last_info {
            ui.colored_label(Color32::from_rgb(40, 140, 60), info);
        }
        if let Some(err) = &self.last_error {
            ui.colored_label(Color32::from_rgb(220, 40, 40), err);
        }
    }

    pub(super) fn render_setup_stage(&mut self, ui: &mut egui::Ui) {
        let mut new_rows = self.rows;
        let mut new_cols = self.cols;
        let seat_cell_size = self.seat_cell_size();

        let mut insert_row_at: Option<usize> = None;
        let mut delete_row_at: Option<usize> = None;
        let mut insert_col_at: Option<usize> = None;
        let mut delete_col_at: Option<usize> = None;
        let mut clicked_seats = Vec::new();

        ui.add_space(8.0);

        ui.group(|ui| {
            ui.set_min_width(260.0);
            ui.label(RichText::new("基本設定").strong());
            ui.add_space(6.0);

            egui::Grid::new("setup-basic-grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("希望無視率");
                    ui.add(
                        eframe::egui::Slider::new(&mut self.config.randomness, 0.0..=1.0)
                            .show_value(true),
                    );

                    ui.end_row();
                });
            CollapsingHeader::new("詳細設定").show(ui, |ui| {
                egui::Grid::new("setup-basic-grid")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("seed(0でランダム)");
                        ui.add(
                            eframe::egui::DragValue::new(&mut self.config.seed).range(0..=u64::MAX),
                        );
                        ui.end_row();

                        ui.label("最適化施行回数");
                        ui.add(
                            eframe::egui::DragValue::new(&mut self.config.budget)
                                .range(0..=2_000_000),
                        );
                        ui.end_row();
                    });
            });

            ui.add_space(6.0);
        });

        ui.group(|ui| {
            ui.set_min_width(320.0);
            ui.label(RichText::new("座席形状設定").strong());
            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                ui.label("行数");
                ui.add(eframe::egui::DragValue::new(&mut new_rows).range(1..=usize::MAX));
                ui.label("列数");
                ui.add(eframe::egui::DragValue::new(&mut new_cols).range(1..=usize::MAX));
            });

            ui.label(format!(
                "総席数: {} / 空席: {} / 利用可能席: {} / 学生数: {}",
                self.seat_count(),
                self.empty_seats
                    .iter()
                    .filter(|is_empty| **is_empty)
                    .count(),
                self.available_seat_count(),
                self.students.len()
            ));
            ui.separator();
            ui.add_space(6.0);
            ui.label("席をクリックすると空席/使用席を切り替えます");

            if new_rows != self.rows || new_cols != self.cols {
                self.resize_grid(new_rows, new_cols);
            }

            ui.add_space(8.0);
            egui::ScrollArea::both()
                .id_salt("setup-seat-shape-scroll")
                .auto_shrink([false, false])
                .max_height(460.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let _ = Self::bubble_pair_cell(ui, seat_cell_size, false, false);
                        for c in 0..self.cols {
                            let (plus, minus) =
                                Self::bubble_pair_cell(ui, seat_cell_size, true, self.cols > 1);
                            if plus {
                                insert_col_at = Some(c);
                            }
                            if minus {
                                delete_col_at = Some(c);
                            }
                        }
                        let (plus_tail, _) =
                            Self::bubble_pair_cell(ui, seat_cell_size, true, false);
                        if plus_tail {
                            insert_col_at = Some(self.cols);
                        }
                    });

                    ui.add_space(4.0);
                    for r in 0..self.rows {
                        ui.horizontal(|ui| {
                            let (plus, minus) =
                                Self::bubble_pair_cell(ui, seat_cell_size, true, self.rows > 1);
                            if plus {
                                insert_row_at = Some(r);
                            }
                            if minus {
                                delete_row_at = Some(r);
                            }

                            for c in 0..self.cols {
                                let idx = r * self.cols + c;
                                let is_empty = self.empty_seats[idx];
                                let label = self.coord_label(idx);
                                let button = Button::new(RichText::new(label).color(if is_empty {
                                    Color32::WHITE
                                } else {
                                    Color32::BLACK
                                }))
                                .fill(if is_empty {
                                    Color32::from_rgb(180, 40, 40)
                                } else {
                                    Color32::from_rgb(220, 220, 220)
                                });

                                if ui.add_sized(seat_cell_size, button).clicked() {
                                    clicked_seats.push(idx);
                                }
                            }
                        });
                    }

                    ui.horizontal(|ui| {
                        let (plus_tail, _) =
                            Self::bubble_pair_cell(ui, seat_cell_size, true, false);
                        if plus_tail {
                            insert_row_at = Some(self.rows);
                        }
                    });
                });
        });

        let mut structure_changed = false;
        if let Some(r) = insert_row_at {
            self.insert_row_at(r);
            structure_changed = true;
        }
        if let Some(r) = delete_row_at {
            self.delete_row_at(r);
            structure_changed = true;
        }
        if let Some(c) = insert_col_at {
            self.insert_col_at(c);
            structure_changed = true;
        }
        if let Some(c) = delete_col_at {
            self.delete_col_at(c);
            structure_changed = true;
        }

        if !structure_changed {
            let mut seat_changed = false;
            for idx in clicked_seats {
                if idx < self.empty_seats.len() {
                    let next = !self.empty_seats[idx];
                    seat_changed |= self.set_empty_seat_state(idx, next);
                }
            }

            if seat_changed {
                self.clear_result_if_needed();
                self.clear_messages();
            }
        }
    }

    pub(super) fn render_students_stage(&mut self, ui: &mut egui::Ui) {
        self.ensure_valid_selected_student();

        let mut pick_students_json = false;
        let mut load_students_json = false;
        let mut export_students_json = false;
        let mut add_student = false;
        let mut remove_selected = false;
        let mut student_changed = false;
        let mut add_tag = false;
        let mut remove_tag_idx: Option<usize> = None;
        let mut renamed_tag_symbols = Vec::new();
        let mut toggle_tag_symbols = Vec::new();
        let mut clear_tags_for_selected = false;
        let mut tag_definition_changed = false;

        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(RichText::new("学生一覧").strong());

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.students_json_path);
                    if ui.button("参照").clicked() {
                        pick_students_json = true;
                    }
                    if ui.button("読み込む").clicked() {
                        load_students_json = true;
                    }
                    if ui.button("保存").clicked() {
                        export_students_json = true;
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("学生を追加").clicked() {
                        add_student = true;
                    }

                    if ui
                        .add_enabled(self.selected_student.is_some(), Button::new("選択中を削除"))
                        .clicked()
                    {
                        remove_selected = true;
                    }
                });

                ui.separator();
                ui.add_space(6.0);
                ui.label("選択して右側で編集してください");
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("students-list-scroll")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        for (i, student) in self.students.iter().enumerate() {
                            let selected = self.selected_student == Some(i);
                            let display_name = Self::student_display_name(student, i);
                            let id_label = student
                                .id
                                .map(|id| format!("#{}", id))
                                .unwrap_or_else(|| "#未設定".to_string());
                            let label = format!("{}. {} {}", i + 1, id_label, display_name);

                            if ui.selectable_label(selected, label).clicked() {
                                self.selected_student = Some(i);
                            }
                        }
                    });
            });

            columns[1].group(|ui| {
                ui.label(RichText::new("選択中の学生を編集").strong());
                ui.add_space(4.0);

                if let Some(student_idx) = self.selected_student {
                    if student_idx < self.students.len() {
                        ui.horizontal(|ui| {
                            ui.label("番号");
                            let mut id_str = self.students[student_idx]
                                .id
                                .map_or(String::new(), |id| id.to_string());
                            if ui.text_edit_singleline(&mut id_str).changed() {
                                if id_str.trim().is_empty() {
                                    self.students[student_idx].id = None;
                                } else if let Ok(id) = id_str.trim().parse::<u16>() {
                                    self.students[student_idx].id = Some(id);
                                }
                                student_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("姓");
                            if ui
                                .text_edit_singleline(&mut self.students[student_idx].last_name)
                                .changed()
                            {
                                student_changed = true;
                            }
                            ui.label("名");
                            if ui
                                .text_edit_singleline(&mut self.students[student_idx].first_name)
                                .changed()
                            {
                                student_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("セイ");
                            if ui
                                .text_edit_singleline(&mut self.students[student_idx].last_kana)
                                .changed()
                            {
                                student_changed = true;
                            }
                            ui.label("メイ");
                            if ui
                                .text_edit_singleline(&mut self.students[student_idx].first_kana)
                                .changed()
                            {
                                student_changed = true;
                            }
                        });

                        ui.add_space(6.0);
                        if ui.button("この学生の希望席設定へ移動").clicked() {
                            self.current_stage = UiStage::Targets;
                        }

                        ui.add_space(8.0);

                        if self.tag_forms.is_empty() {
                            ui.label("タグが存在しません。");
                        } else {
                            ui.label("タグの割り当て");
                            for tag in &self.tag_forms {
                                let symbol = Self::normalize_tag_symbol(&tag.symbol);
                                if symbol.is_empty() {
                                    continue;
                                }

                                let mut selected = self.students[student_idx]
                                    .tags
                                    .iter()
                                    .any(|existing| existing == &symbol);

                                let text = if tag.label.trim().is_empty() {
                                    symbol.clone()
                                } else {
                                    format!("{} {}", symbol, tag.label.trim())
                                };

                                if ui.checkbox(&mut selected, text).clicked() {
                                    toggle_tag_symbols.push(symbol);
                                }
                            }

                            ui.add_space(4.0);

                            if ui.button("この学生のタグを全解除").clicked() {
                                clear_tags_for_selected = true;
                            }
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.label(RichText::new("タグ登録と割り当て").strong());

                        if ui.button("新しいタグを追加").clicked() {
                            add_tag = true;
                        }

                        ui.add_space(6.0);
                        if self.tag_forms.is_empty() {
                            ui.label("登録済みタグはありません。");
                        } else {
                            let mut used_symbols = HashSet::new();
                            let mut has_invalid_tag = false;
                            for (tag_idx, tag) in self.tag_forms.iter_mut().enumerate() {
                                let normalized_symbol = Self::normalize_tag_symbol(&tag.symbol);
                                if tag.symbol != normalized_symbol {
                                    tag.symbol = normalized_symbol;
                                    tag_definition_changed = true;
                                }
                                let old_symbol = tag.symbol.clone();

                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("記号:");
                                        let symbol_changed = ui
                                            .add(
                                                egui::TextEdit::singleline(&mut tag.symbol)
                                                    .desired_width(64.0),
                                            )
                                            .changed();
                                        if symbol_changed {
                                            tag.symbol = Self::normalize_tag_symbol(&tag.symbol);
                                            if tag.symbol != old_symbol {
                                                renamed_tag_symbols
                                                    .push((old_symbol.clone(), tag.symbol.clone()));
                                            }
                                        }
                                        if ui.button("削除").clicked() {
                                            remove_tag_idx = Some(tag_idx);
                                        }
                                        tag_definition_changed |= symbol_changed;
                                    });

                                    ui.horizontal(|ui| {
                                        ui.add_space(24.0);
                                        ui.label("説明");
                                        let label_changed = ui
                                            .add(
                                                egui::TextEdit::singleline(&mut tag.label)
                                                    .desired_width(180.0),
                                            )
                                            .changed();
                                        tag_definition_changed |= label_changed;
                                    });
                                });
                                has_invalid_tag |= tag.symbol.is_empty()
                                    || !used_symbols.insert(tag.symbol.clone());
                                ui.add_space(4.0);
                            }

                            if has_invalid_tag {
                                ui.colored_label(
                                    Color32::from_rgb(220, 120, 20),
                                    "記号が空、または重複しているタグは出力に含まれません。",
                                );
                            }
                        }
                    }
                } else {
                    ui.label("学生を追加して選択してください。");
                }
            });
        });

        if add_student {
            self.students.push(StudentForm::default());
            self.selected_student = Some(self.students.len() - 1);
            self.clear_result_if_needed();
            self.clear_messages();
        }

        if pick_students_json {
            Self::pick_input_path(&mut self.students_json_path, "JSON", &["json"]);
        }

        if load_students_json {
            self.load_students_from_json();
        }

        if export_students_json {
            self.export_students_json();
        }

        if add_tag {
            let used_symbols = self
                .tag_forms
                .iter()
                .map(|tag| Self::normalize_tag_symbol(&tag.symbol))
                .collect::<HashSet<_>>();
            let mut tag = Self::default_tag_form();
            tag.symbol = Self::next_unused_tag_symbol(&used_symbols);
            self.tag_forms.push(tag);
            self.clear_result_if_needed();
            self.clear_messages();
        }

        if tag_definition_changed {
            self.clear_result_if_needed();
            self.clear_messages();
        }

        for (old_symbol, new_symbol) in renamed_tag_symbols {
            self.rename_tag_in_students(&old_symbol, &new_symbol);
        }

        if let Some(tag_idx) = remove_tag_idx
            && tag_idx < self.tag_forms.len()
        {
            let removed_symbol = Self::normalize_tag_symbol(&self.tag_forms[tag_idx].symbol);
            self.tag_forms.remove(tag_idx);
            if !removed_symbol.is_empty() {
                self.remove_tag_from_students(&removed_symbol);
            }
            self.clear_result_if_needed();
            self.clear_messages();
        }

        if let Some(student_idx) = self.selected_student
            && student_idx < self.students.len()
        {
            if clear_tags_for_selected {
                self.students[student_idx].tags.clear();
                self.clear_result_if_needed();
                self.clear_messages();
            }

            if !toggle_tag_symbols.is_empty() {
                let valid_tags = self.build_tag_symbol_set();
                let mut tags = self.students[student_idx].tags.clone();

                for symbol in toggle_tag_symbols {
                    let selected = tags.contains(&symbol);
                    let mut student = self.students[student_idx].clone();
                    student.tags = tags.clone();
                    Self::assign_tag_to_student(&mut student, &symbol, !selected);
                    tags = student.tags;
                }

                tags = Self::sanitize_student_tags(&tags, &valid_tags);
                if tags != self.students[student_idx].tags {
                    self.students[student_idx].tags = tags;
                    self.clear_result_if_needed();
                    self.clear_messages();
                }
            }
        }

        if remove_selected
            && let Some(idx) = self.selected_student
            && idx < self.students.len()
        {
            self.students.remove(idx);
            self.selected_student = if self.students.is_empty() {
                None
            } else if idx >= self.students.len() {
                Some(self.students.len() - 1)
            } else {
                Some(idx)
            };
            self.clear_result_if_needed();
            self.clear_messages();
        }

        if student_changed {
            self.clear_result_if_needed();
            self.clear_messages();
        }

        if self.students.len() > self.available_seat_count() {
            ui.add_space(8.0);
            ui.colored_label(
                Color32::from_rgb(220, 40, 40),
                format!(
                    "学生数({}) が利用可能席数({})を超えています。",
                    self.students.len(),
                    self.available_seat_count()
                ),
            );
        }
    }

    pub(super) fn render_relation_editor(
        &self,
        ui: &mut egui::Ui,
        relation: RelationKind,
        student_idx: usize,
        assigned_ids: &[u16],
    ) -> (bool, Vec<u16>) {
        let mut clear = false;
        let mut toggled_ids = Vec::new();

        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new(relation.title()).strong());

        if ui.button(relation.clear_button_label()).clicked() {
            clear = true;
        }

        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt(relation.scroll_id())
            .max_height(150.0)
            .show(ui, |ui| {
                for (other_idx, other_student) in self.students.iter().enumerate() {
                    if other_idx == student_idx {
                        continue;
                    }

                    let Some(&other_id) = assigned_ids.get(other_idx) else {
                        continue;
                    };

                    let selected = Self::relation_ids(
                        &self.students[student_idx],
                        relation,
                        self.target_edit_mode,
                    )
                    .contains(&other_id);
                    let label = format!(
                        "#{} {}",
                        other_id,
                        Self::student_display_name(other_student, other_idx)
                    );
                    if ui.selectable_label(selected, label).clicked() {
                        toggled_ids.push(other_id);
                    }
                }
            });

        (clear, toggled_ids)
    }

    pub(super) fn clear_relation(
        &mut self,
        student_idx: usize,
        relation: RelationKind,
        mode: TargetEditMode,
    ) {
        if student_idx >= self.students.len() {
            return;
        }

        let ids = Self::relation_ids_mut(&mut self.students[student_idx], relation, mode);
        if !ids.is_empty() {
            ids.clear();
            self.clear_result_if_needed();
            self.clear_messages();
        }
    }

    pub(super) fn apply_relation_toggles(
        &mut self,
        student_idx: usize,
        relation: RelationKind,
        mode: TargetEditMode,
        toggled_ids: &[u16],
        assigned_ids: &[u16],
        valid_ids: &HashSet<u16>,
    ) {
        if student_idx >= self.students.len() {
            return;
        }

        let self_id = assigned_ids.get(student_idx).copied().unwrap_or(0);
        let ids = Self::relation_ids_mut(&mut self.students[student_idx], relation, mode);
        if Self::toggle_relation_ids(ids, toggled_ids, self_id, valid_ids) {
            self.clear_result_if_needed();
            self.clear_messages();
        }
    }

    pub(super) fn render_targets_stage(&mut self, ui: &mut egui::Ui) {
        self.ensure_valid_selected_student();
        let seat_cell_size = self.seat_cell_size();
        let assigned_ids = self.assign_student_ids();
        let valid_ids = assigned_ids.iter().copied().collect::<HashSet<_>>();
        let mut id_to_name = BTreeMap::new();
        for (i, student) in self.students.iter().enumerate() {
            if let Some(&id) = assigned_ids.get(i) {
                id_to_name.insert(id, Self::student_display_name(student, i));
            }
        }

        if let Some(student_idx) = self.selected_student
            && student_idx < self.students.len()
        {
            let self_id = assigned_ids.get(student_idx).copied().unwrap_or(0);
            if Self::normalize_student_relations(
                &mut self.students[student_idx],
                self_id,
                &valid_ids,
            ) {
                self.clear_result_if_needed();
            }
        }

        let mut clear_current_targets_for_selected = false;
        let mut clear_close_to_for_selected = false;
        let mut clear_avoid_for_selected = false;
        let mut toggle_close_to_ids = Vec::new();
        let mut toggle_avoid_ids = Vec::new();
        let mut register_preset = false;
        let mut apply_preset_idx: Option<usize> = None;
        let mut remove_preset_idx: Option<usize> = None;
        #[cfg(feature = "google-fetch")]
        let mut import_preferences = false;

        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("編集モード").strong());
                ui.horizontal(|ui| {
                    #[cfg(feature = "google-fetch")]
                    if ui.button("フォームから希望を反映").clicked() {
                        import_preferences = true;
                    }
                });

                ui.selectable_value(
                    &mut self.target_edit_mode,
                    TargetEditMode::Soft,
                    TargetEditMode::Soft.title(),
                );
                ui.selectable_value(
                    &mut self.target_edit_mode,
                    TargetEditMode::Forced,
                    TargetEditMode::Forced.title(),
                );
                ui.separator();

                if let Some(student_idx) = self.selected_student
                    && student_idx < self.students.len()
                {
                    let display_name =
                        Self::student_display_name(&self.students[student_idx], student_idx);
                    ui.label(format!(
                        "編集中: {} / {}",
                        display_name,
                        self.target_edit_mode.title()
                    ));
                } else {
                    ui.label("学生を選択してください。");
                }
            });
        });
        ui.add_space(8.0);

        let relation_action_mode = self.target_edit_mode;

        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(RichText::new("編集対象の学生").strong());
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .id_salt("edit-student-scroll")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for (i, student) in self.students.iter().enumerate() {
                            let selected = self.selected_student == Some(i);
                            let label =
                                format!("{}. {}", i + 1, Self::student_display_name(student, i));
                            if ui.selectable_label(selected, label).clicked() {
                                self.selected_student = Some(i);
                            }
                        }
                    });

                ui.separator();
                ui.add_space(6.0);
                ui.label(format!(
                    "{}を選択してください",
                    self.target_edit_mode.title()
                ));
                ui.add_space(8.0);

                if let Some(student_idx) = self.selected_student
                    && student_idx < self.students.len()
                {
                    egui::ScrollArea::both()
                        .id_salt("target-seat-map-scroll")
                        .auto_shrink([false, false])
                        .max_height(400.0)
                        .show(ui, |ui| {
                            egui::Grid::new("target-seat-grid")
                                .num_columns(self.cols)
                                .spacing([6.0, 6.0])
                                .show(ui, |ui| {
                                    for r in 0..self.rows {
                                        for c in 0..self.cols {
                                            let idx = r * self.cols + c;
                                            let label = self.coord_label(idx);

                                            if self.empty_seats[idx] {
                                                ui.add_enabled_ui(false, |ui| {
                                                    ui.add_sized(
                                                        seat_cell_size,
                                                        Button::new(format!("{}(空)", label)),
                                                    );
                                                });
                                                continue;
                                            }

                                            let selected = match self.target_edit_mode {
                                                TargetEditMode::Soft => self.students[student_idx]
                                                    .targets
                                                    .contains(&idx),
                                                TargetEditMode::Forced => self.students
                                                    [student_idx]
                                                    .forced_targets
                                                    .contains(&idx),
                                            };

                                            let button = Button::new(RichText::new(label).color(
                                                if selected {
                                                    Color32::WHITE
                                                } else {
                                                    Color32::BLACK
                                                },
                                            ))
                                            .fill(if selected {
                                                Color32::from_rgb(50, 130, 80)
                                            } else {
                                                Color32::from_rgb(220, 220, 220)
                                            });

                                            if ui.add_sized(seat_cell_size, button).clicked() {
                                                self.toggle_target(
                                                    student_idx,
                                                    idx,
                                                    matches!(
                                                        self.target_edit_mode,
                                                        TargetEditMode::Forced
                                                    ),
                                                );
                                            }
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                } else if self.selected_student.is_some() {
                    ui.label("対象学生が見つかりません。学生入力ステージを確認してください。");
                } else {
                    ui.label("まず学生を追加して選択してください。");
                }

                if let Some(student_idx) = self.selected_student
                    && student_idx < self.students.len()
                {
                    if ui.button("クリア").clicked() {
                        clear_current_targets_for_selected = true;
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("プリセット名");
                        ui.text_edit_singleline(&mut self.new_preset_name);
                    });
                    if ui
                        .button(format!("現在の{}を登録", self.target_edit_mode.title()))
                        .clicked()
                    {
                        register_preset = true;
                    }

                    ui.add_space(6.0);
                    if self.target_presets.is_empty() {
                        ui.label("登録済みプリセットはありません。");
                    } else {
                        for (preset_idx, preset) in self.target_presets.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "{}: {}",
                                    preset.name,
                                    self.preset_summary(preset)
                                ));

                                if ui.button("適用").clicked() {
                                    apply_preset_idx = Some(preset_idx);
                                }
                                if ui.button("削除").clicked() {
                                    remove_preset_idx = Some(preset_idx);
                                }
                            });
                        }
                    }
                }
            });

            columns[1].group(|ui| {
                ui.label(
                    RichText::new(format!("隣席希望 - {}", self.target_edit_mode.title())).strong(),
                );
                ui.add_space(6.0);
                if let Some(student_idx) = self.selected_student
                    && student_idx < self.students.len()
                {
                    let (clear_close_to, close_to_toggles) = self.render_relation_editor(
                        ui,
                        RelationKind::CloseTo,
                        student_idx,
                        &assigned_ids,
                    );
                    clear_close_to_for_selected = clear_close_to;
                    toggle_close_to_ids = close_to_toggles;

                    let (clear_avoid, avoid_toggles) = self.render_relation_editor(
                        ui,
                        RelationKind::Avoid,
                        student_idx,
                        &assigned_ids,
                    );
                    clear_avoid_for_selected = clear_avoid;
                    toggle_avoid_ids = avoid_toggles;
                }
            });
        });

        #[cfg(feature = "google-fetch")]
        if import_preferences {
            self.import_preferences_from_forms();
        }

        if clear_current_targets_for_selected && let Some(student_idx) = self.selected_student {
            match relation_action_mode {
                TargetEditMode::Soft => self.clear_targets(student_idx),
                TargetEditMode::Forced => self.clear_forced_targets(student_idx),
            }
        }

        if let Some(student_idx) = self.selected_student {
            if clear_close_to_for_selected {
                self.clear_relation(student_idx, RelationKind::CloseTo, relation_action_mode);
            }
            if clear_avoid_for_selected {
                self.clear_relation(student_idx, RelationKind::Avoid, relation_action_mode);
            }
            self.apply_relation_toggles(
                student_idx,
                RelationKind::CloseTo,
                relation_action_mode,
                &toggle_close_to_ids,
                &assigned_ids,
                &valid_ids,
            );
            self.apply_relation_toggles(
                student_idx,
                RelationKind::Avoid,
                relation_action_mode,
                &toggle_avoid_ids,
                &assigned_ids,
                &valid_ids,
            );
        }

        if register_preset && let Some(student_idx) = self.selected_student {
            self.register_current_as_preset(student_idx);
        }

        if let Some(preset_idx) = apply_preset_idx
            && let Some(student_idx) = self.selected_student
        {
            self.apply_preset_to_student(student_idx, preset_idx);
        }

        if let Some(preset_idx) = remove_preset_idx
            && preset_idx < self.target_presets.len()
        {
            let removed = self.target_presets.remove(preset_idx);
            self.set_info(format!("プリセット '{}' を削除しました。", removed.name));
        }
    }

    pub(super) fn result_cell_text(
        &self,
        result: &SeatingResult,
        seat_idx: usize,
        built_students: &[Student],
        tag_defs: &BTreeMap<String, TagDefinition>,
        reveal_all: bool,
    ) -> String {
        let Some(student_idx) = result.layout.get(seat_idx).and_then(|x| *x) else {
            return "-".to_string();
        };
        let Some(student) = built_students.get(student_idx) else {
            return "-".to_string();
        };

        if !reveal_all
            && self.result_display_mode == ResultDisplayMode::Random
            && !self.animation_displayed_indices.contains(&student_idx)
        {
            return "?".to_string();
        }

        let tags = Self::student_tag_symbols_with_defs(&student.tags, tag_defs);
        if tags.is_empty() {
            format!("{}\n({})", student.name, student.number)
        } else {
            format!("{}\n({})\n{}", student.name, student.number, tags)
        }
    }

    pub(super) fn render_result_grid(
        &mut self,
        ui: &mut egui::Ui,
        result: &SeatingResult,
        full_screen: bool,
    ) {
        let seat_cell_size = if full_screen {
            [176.0, 96.0]
        } else {
            self.result_cell_size()
        };
        let built_students = self.build_students();
        let tag_defs = self.build_tags_map();

        ui.label(RichText::new(format!("sekigae3 cost: {:.3}", result.cost)).strong());

        if self.result_display_mode == ResultDisplayMode::Random {
            // 1秒ごとにランダムに学生を追加表示
            if self.animation_last_update.elapsed() >= Duration::from_secs(1) {
                let all_student_indices: Vec<usize> = (0..built_students.len()).collect();
                let remaining: Vec<usize> = all_student_indices
                    .iter()
                    .filter(|idx| !self.animation_displayed_indices.contains(idx))
                    .copied()
                    .collect();

                if !remaining.is_empty() {
                    // より良い乱数生成：複数の値を組み合わせる
                    let now = Instant::now();
                    let seed = (now.elapsed().subsec_nanos() as u64)
                        .wrapping_mul(2654435761)
                        .wrapping_add(self.animation_displayed_indices.len() as u64);
                    let random_idx = remaining[(seed as usize) % remaining.len()];
                    self.animation_displayed_indices.push(random_idx);
                    self.animation_last_update = Instant::now();
                }
            }
        }

        ui.add_space(6.0);
        let scroll = egui::ScrollArea::both()
            .id_salt("result-seat-map-scroll")
            .auto_shrink([false, false]);
        if full_screen {
            // 全画面時は中央寄せせず、左上基準で大きく表示する
            scroll.show(ui, |ui| {
                let avail_w = ui.available_width().max(900.0);
                let avail_h = ui.available_height().max(400.0);
                let spacing = 8.0;
                let mut cw = (avail_w - (self.cols as f32 - 1.0) * spacing) / self.cols as f32;
                cw = cw.clamp(90.0, 240.0);
                let ch = (avail_h / self.rows.max(1) as f32).clamp(58.0, 170.0);
                let cell = [cw, ch.min(cw * 0.44)];

                egui::Grid::new("result-grid")
                    .num_columns(self.cols)
                    .spacing([8.0, 8.0])
                    .show(ui, |ui| {
                        for r in 0..self.rows {
                            for c in 0..self.cols {
                                let idx = r * self.cols + c;

                                if self.empty_seats[idx] {
                                    ui.add_sized(
                                        cell,
                                        Button::new(
                                            RichText::new("空席").color(Color32::WHITE).size(18.0),
                                        )
                                        .fill(Color32::from_rgb(120, 120, 120)),
                                    );
                                    continue;
                                }

                                let text = self.result_cell_text(
                                    result,
                                    idx,
                                    &built_students,
                                    &tag_defs,
                                    true,
                                );

                                ui.add_sized(cell, Button::new(RichText::new(text).size(18.0)));
                            }
                            ui.end_row();
                        }
                    });
            });
        } else {
            scroll.max_height(500.0).show(ui, |ui| {
                ui.set_min_width(500.0);
                egui::Grid::new("result-grid")
                    .num_columns(self.cols)
                    .spacing([6.0, 6.0])
                    .show(ui, |ui| {
                        for r in 0..self.rows {
                            for c in 0..self.cols {
                                let idx = r * self.cols + c;

                                if self.empty_seats[idx] {
                                    ui.add_sized(
                                        seat_cell_size,
                                        Button::new(RichText::new("空席").color(Color32::WHITE))
                                            .fill(Color32::from_rgb(120, 120, 120)),
                                    );
                                    continue;
                                }

                                let text = self.result_cell_text(
                                    result,
                                    idx,
                                    &built_students,
                                    &tag_defs,
                                    false,
                                );

                                ui.add_sized(
                                    seat_cell_size,
                                    Button::new(RichText::new(text).size(13.0)),
                                );
                            }
                            ui.end_row();
                        }
                    });
            });
        }
    }

    pub(super) fn render_solve_export_stage(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let mut run_solver = false;
        let full_screen = self.result_fullscreen;

        if full_screen {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("席替え実行と結果").strong());
                    ui.separator();
                    self.render_result_display_mode_selector(ui);
                });

                ui.add_space(6.0);
                if ui
                    .add_enabled(
                        !self.is_solving,
                        Button::new("席替えを実行").min_size(egui::vec2(240.0, 40.0)),
                    )
                    .clicked()
                {
                    run_solver = true;
                }

                if self.is_solving {
                    ui.add_space(4.0);
                    ui.colored_label(Color32::from_rgb(200, 120, 20), "席替え中...");
                }

                ui.add_space(8.0);
                if self.result.is_some() {
                    let result = self.result.as_ref().unwrap().clone();
                    self.render_result_grid(ui, &result, true);
                } else {
                    ui.label("まだ結果がありません。左上のボタンで席替えを実行してください。");
                }
            });
        } else {
            ui.columns(2, |columns| {
                columns[0].group(|ui| {
                    ui.label(RichText::new("席替え実行と結果").strong());
                    ui.add_space(6.0);

                    if ui
                        .add_enabled(
                            !self.is_solving,
                            Button::new("席替えを実行").min_size(egui::vec2(240.0, 40.0)),
                        )
                        .clicked()
                    {
                        run_solver = true;
                    }

                    if self.is_solving {
                        ui.add_space(4.0);
                        ui.colored_label(Color32::from_rgb(200, 120, 20), "席替え中...");
                    }

                    ui.add_space(8.0);
                    self.render_result_display_mode_selector(ui);
                    ui.add_space(4.0);

                    if self.result.is_some() {
                        let result = self.result.as_ref().unwrap().clone();
                        self.render_result_grid(ui, &result, false);
                    } else {
                        ui.label("まだ結果がありません。左上のボタンで席替えを実行してください。");
                    }
                });

                columns[1].group(|ui| {
                    self.render_export_panel(ui);
                });
            });
        }

        if run_solver {
            self.run_solver(ctx);
        }
    }
}
