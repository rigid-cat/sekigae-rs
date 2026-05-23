use super::*;

impl SekigaeApp {
    pub(super) fn path_from_input(input: &str, default_value: &str) -> PathBuf {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            PathBuf::from(default_value)
        } else {
            PathBuf::from(trimmed)
        }
    }

    pub(super) fn pick_input_path(target: &mut String, filter_name: &str, extensions: &[&str]) {
        if let Some(path) = FileDialog::new()
            .add_filter(filter_name, extensions)
            .pick_file()
        {
            *target = path.to_string_lossy().to_string();
        }
    }

    pub(super) fn pick_output_path(
        target: &mut String,
        filter_name: &str,
        extensions: &[&str],
        default_file_name: &str,
    ) {
        if let Some(path) = FileDialog::new()
            .add_filter(filter_name, extensions)
            .set_file_name(default_file_name)
            .save_file()
        {
            *target = path.to_string_lossy().to_string();
        }
    }

    pub(super) fn path_row(
        ui: &mut egui::Ui,
        label: &str,
        button_label: &str,
        path: &mut String,
    ) -> bool {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.text_edit_singleline(path);
            ui.button(button_label).clicked()
        })
        .inner
    }

    pub(super) fn ensure_parent_dir(path: &Path, label: &str) -> Result<(), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "{} 出力先ディレクトリの作成に失敗しました: {} ({})",
                    label,
                    parent.display(),
                    err
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn load_students_from_json(&mut self) {
        self.clear_messages();

        let path = Self::path_from_input(&self.students_json_path, DEFAULT_STUDENTS_JSON_PATH);
        log::info!("loading students json: {}", path.display());
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                self.set_error(format!(
                    "students.json の読み込みに失敗しました: {} ({})",
                    path.display(),
                    err
                ));
                return;
            }
        };

        let (parsed_students, loaded_presets, loaded_tags) =
            if let Ok(document) = serde_json::from_str::<StudentsJsonDocument>(&text) {
                (document.students, document.target_presets, document.tags)
            } else {
                let raw: BTreeMap<String, StudentProfile> = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        self.set_error(format!("json の形式が不正です: {}", err));
                        return;
                    }
                };

                let mut converted = BTreeMap::new();
                for (id_text, profile) in raw {
                    let id = match id_text.parse::<u16>() {
                        Ok(id) if id > 0 => id,
                        _ => {
                            self.set_error(format!(
                            "students.json のキー '{}' は 1..65535 の数値文字列にしてください。",
                            id_text
                        ));
                            return;
                        }
                    };
                    converted.insert(id, profile);
                }
                (converted, Vec::new(), BTreeMap::new())
            };

        let mut tag_migrations = BTreeMap::new();
        let mut used_tag_symbols = HashSet::new();
        self.tag_forms = loaded_tags
            .into_iter()
            .filter_map(|(key, definition)| {
                let symbol = Self::tag_symbol_from_definition(&key, &definition);
                if symbol.is_empty() || !used_tag_symbols.insert(symbol.clone()) {
                    return None;
                }
                if key != symbol {
                    tag_migrations.insert(key, symbol.clone());
                }
                Some(TagDefinition {
                    symbol,
                    label: definition.label,
                })
            })
            .collect();

        self.students = parsed_students
            .into_iter()
            .map(|(id, profile)| StudentForm {
                id: Some(id),
                last_name: profile.last_name,
                first_name: profile.first_name,
                last_kana: profile.last_kana,
                first_kana: profile.first_kana,
                tags: profile
                    .tags
                    .into_iter()
                    .map(|tag| tag_migrations.get(&tag).cloned().unwrap_or(tag))
                    .collect(),
                targets: profile.targets,
                forced_targets: profile.forced_targets,
                close_to: profile.close_to,
                forced_close_to: profile.forced_close_to,
                avoid: profile.avoid,
                forced_avoid: profile.forced_avoid,
            })
            .collect();

        let valid_tags = self.build_tag_symbol_set();
        for student in &mut self.students {
            student.tags = Self::sanitize_student_tags(&student.tags, &valid_tags);
        }

        let seat_count = self.seat_count();
        let empty_seats = self.empty_seats.clone();
        for student in &mut self.students {
            Self::normalize_student_targets(student, seat_count, &empty_seats);
        }

        for preset in loaded_presets {
            self.upsert_target_preset(preset);
        }

        self.selected_student = (!self.students.is_empty()).then_some(0);

        self.mark_dirty();
        log::info!(
            "loaded students json: students={}, tags={}, presets={}",
            self.students.len(),
            self.tag_forms.len(),
            self.target_presets.len()
        );
        self.set_info(format!(
            "{} 人の学生情報を読み込みました。",
            self.students.len()
        ));
    }

    #[cfg(feature = "google-fetch")]
    pub(super) fn import_preferences_from_forms(&mut self) {
        self.clear_messages();
        log::info!("importing preferences from Google form");

        if self.students.is_empty() {
            self.set_error("学生がいません。先に学生を追加してください。");
            return;
        }

        let config = match load_preferences_config("config.json") {
            Ok(config) => config,
            Err(err) => {
                self.set_error(err);
                return;
            }
        };

        let fetched = match fetch_student_preferences(&config.preferences_url) {
            Ok(value) => value,
            Err(err) => {
                self.set_error(format!("フォームの取得に失敗しました: {}", err));
                return;
            }
        };

        let assigned_ids = self.assign_student_ids();
        let valid_ids = assigned_ids.iter().copied().collect::<HashSet<_>>();
        let seat_count = self.seat_count();
        let empty_seats = self.empty_seats.clone();
        let rows = self.rows;
        let cols = self.cols;
        let mut updated = 0usize;

        for (idx, student) in self.students.iter_mut().enumerate() {
            let Some(&id) = assigned_ids.get(idx) else {
                continue;
            };

            let Some(pref) = fetched.get(&id) else {
                continue;
            };

            student.targets =
                parse_targets(&pref.seat_targets_raw, rows, cols, &config.seat_preferences);
            student.forced_targets = parse_targets(
                &pref.forced_seat_targets_raw,
                rows,
                cols,
                &config.seat_preferences,
            );
            student.close_to = pref.close_to.clone();
            student.avoid = pref.avoid.clone();
            Self::normalize_student_targets(student, seat_count, &empty_seats);
            Self::normalize_student_relations(student, id, &valid_ids);
            updated += 1;
        }

        if updated == 0 {
            self.set_error(
                "取得したフォーム回答に、現在の学生IDと一致するデータがありませんでした。",
            );
            return;
        }

        self.mark_dirty();
        self.set_info(format!("フォームの希望を {} 人分反映しました。", updated));
    }

    pub(super) fn write_json_value<T: Serialize>(
        path: &Path,
        value: &T,
        label: &str,
    ) -> Result<(), String> {
        Self::ensure_parent_dir(path, label)?;

        let json = serde_json::to_string_pretty(value)
            .map_err(|err| format!("{} のJSON生成に失敗しました: {}", label, err))?;

        fs::write(path, json).map_err(|err| {
            format!(
                "{} の書き込みに失敗しました: {} ({})",
                label,
                path.display(),
                err
            )
        })?;
        log::info!("wrote {}: {}", label, path.display());
        Ok(())
    }

    pub(super) fn build_students_json_document(&self) -> StudentsJsonDocument {
        let students = self.build_students_map();
        let tags = self.build_tags_map();
        let target_presets = self
            .target_presets
            .iter()
            .cloned()
            .map(|mut preset| {
                preset.targets = self.sanitize_preset_targets(&preset.targets);
                preset
            })
            .collect::<Vec<_>>();

        StudentsJsonDocument {
            students,
            tags,
            target_presets,
        }
    }

    pub(super) fn export_students_json(&mut self) {
        self.clear_messages();

        let students_document = self.build_students_json_document();
        if students_document.students.is_empty() {
            self.set_error("書き出す学生がいません。学生を追加してください。");
            return;
        }

        let path = Self::path_from_input(&self.students_json_path, DEFAULT_STUDENTS_JSON_PATH);
        match Self::write_json_value(&path, &students_document, "students.json") {
            Ok(()) => self.set_info(format!("students.json を出力しました: {}", path.display())),
            Err(err) => self.set_error(err),
        }
    }
}
