use chrono::Local;
use eframe::egui::{
    self, Button, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle,
};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fetch;
use crate::model::{AnnealingConfig, SeatingResult, Student, Target};
use crate::solver::find_best_seating_with_blocked;

pub struct SekigaeApp {
    rows: usize,
    cols: usize,
    empty_seats: Vec<bool>,
    students: Vec<StudentForm>,
    selected_student: Option<usize>,
    target_presets: Vec<TargetPreset>,
    new_preset_name: String,
    ui_font_size: f32,
    show_debug_status: bool,
    use_custom_date: bool,
    custom_date: String,
    students_json_path: String,
    seats_json_path: String,
    typ_path: String,
    pdf_output_path: String,
    png_output_path: String,
    svg_output_path: String,
    export_pdf: bool,
    export_png: bool,
    export_svg: bool,
    png_ppi: u16,
    config: AnnealingConfig,
    result: Option<SeatingResult>,
    last_error: Option<String>,
    last_info: Option<String>,
    preferences_url: String,
    seat_preferences: std::collections::HashMap<String, fetch::SeatRange>,
}

#[derive(Clone, Debug)]
struct StudentForm {
    id: Option<u16>,
    last_name: String,
    first_name: String,
    last_kana: String,
    first_kana: String,
    targets: Vec<usize>,
    close_to: Vec<u16>,
    avoid: Vec<u16>,
    seat_pref: Option<String>,
}

#[derive(Clone, Debug)]
struct TargetPreset {
    name: String,
    targets: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StudentProfile {
    last_name: String,
    first_name: String,
    last_kana: String,
    first_kana: String,
    targets: Vec<usize>,
}

#[derive(Deserialize)]
struct Config {
    preferences_url: String,
    #[serde(default)]
    seat_preferences: std::collections::HashMap<String, fetch::SeatRange>,
}


#[derive(Serialize)]
struct SeatsLayout {
    rows: usize,
    cols: usize,
}

#[derive(Serialize)]
struct SeatsJsonDocument {
    date: String,
    layout: SeatsLayout,
    seats: Vec<Vec<Option<u16>>>,
    students: BTreeMap<u16, StudentProfile>,
}

impl SekigaeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_japanese_fonts(&cc.egui_ctx);

        let rows = 4;
        let cols = 5;

        let (preferences_url, seat_preferences) = match fs::read_to_string("config.json") {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(config) => (config.preferences_url, config.seat_preferences),
                Err(_) => (String::new(), std::collections::HashMap::new()),
            },
            Err(_) => (String::new(), std::collections::HashMap::new()),
        };

        Self {
            rows,
            cols,
            empty_seats: vec![false; rows * cols],
            students: vec![StudentForm {
                id: None,
                last_name: String::new(),
                first_name: String::new(),
                last_kana: String::new(),
                first_kana: String::new(),
                targets: Vec::new(),
                close_to: Vec::new(),
                avoid: Vec::new(),
                seat_pref: None,
            }],
            selected_student: Some(0),
            target_presets: Vec::new(),
            new_preset_name: String::new(),
            ui_font_size: 18.0,
            show_debug_status: false,
            use_custom_date: false,
            custom_date: Local::now().format("%Y/%m/%d").to_string(),
            students_json_path: "./students.json".to_string(),
            seats_json_path: "./seats.json".to_string(),
            typ_path: "./seats.typ".to_string(),
            pdf_output_path: "./seats.pdf".to_string(),
            png_output_path: "./seats.png".to_string(),
            svg_output_path: "./seats.svg".to_string(),
            export_pdf: true,
            export_png: false,
            export_svg: false,
            png_ppi: 144,
            config: AnnealingConfig {
                iterations: 120_000,
                start_temp: 10.0,
                end_temp: 0.02,
                randomness: 0.5,
            },
            result: None,
            last_error: None,
            last_info: None,
            preferences_url,
            seat_preferences,
        }
    }

    fn seat_count(&self) -> usize {
        self.rows * self.cols
    }

    fn coord_label(&self, seat_idx: usize) -> String {
        let r = seat_idx / self.cols + 1;
        let c = seat_idx % self.cols + 1;
        format!("{}-{}", r, c)
    }

    fn available_seat_count(&self) -> usize {
        self.empty_seats.iter().filter(|is_empty| !**is_empty).count()
    }

    fn clear_result_if_needed(&mut self) {
        self.result = None;
    }

    fn clear_messages(&mut self) {
        self.last_error = None;
        self.last_info = None;
    }

    fn set_error(&mut self, msg: impl Into<String>) {
        self.last_error = Some(msg.into());
        self.last_info = None;
    }

    fn set_info(&mut self, msg: impl Into<String>) {
        self.last_info = Some(msg.into());
        self.last_error = None;
    }

    fn seat_cell_size(&self) -> [f32; 2] {
        if self.show_debug_status {
            [120.0, 52.0]
        } else {
            [120.0, 40.0]
        }
    }

    fn apply_text_style(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        let base = self.ui_font_size;

        style.text_styles.insert(
            TextStyle::Small,
            FontId::new((base - 2.0).max(8.0), FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Body,
            FontId::new(base, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(base, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(base, FontFamily::Monospace),
        );
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(base + 4.0, FontFamily::Proportional),
        );

        ctx.set_style(style);
    }

    fn remap_indices(
        old_indices: &[usize],
        old_cols: usize,
        new_rows: usize,
        new_cols: usize,
    ) -> Vec<usize> {
        let mut mapped = Vec::new();
        for old_idx in old_indices {
            let r = old_idx / old_cols;
            let c = old_idx % old_cols;
            if r < new_rows && c < new_cols {
                mapped.push(r * new_cols + c);
            }
        }
        mapped.sort_unstable();
        mapped.dedup();
        mapped
    }

    fn sanitize_targets_for_grid(
        seat_count: usize,
        empty_seats: &[bool],
        targets: &[usize],
    ) -> Vec<usize> {
        let mut out = targets
            .iter()
            .copied()
            .filter(|seat_idx| *seat_idx < seat_count && !empty_seats[*seat_idx])
            .collect::<Vec<_>>();
        out.sort_unstable();
        out.dedup();
        out
    }

    fn resize_grid(&mut self, new_rows: usize, new_cols: usize) {
        let old_rows = self.rows;
        let old_cols = self.cols;
        let old_empty = self.empty_seats.clone();

        self.rows = new_rows;
        self.cols = new_cols;
        self.empty_seats = vec![false; self.seat_count()];

        for old_idx in 0..(old_rows * old_cols) {
            if !old_empty[old_idx] {
                continue;
            }

            let r = old_idx / old_cols;
            let c = old_idx % old_cols;
            if r < new_rows && c < new_cols {
                let new_idx = r * new_cols + c;
                self.empty_seats[new_idx] = true;
            }
        }

        for student in &mut self.students {
            student.targets = Self::remap_indices(&student.targets, old_cols, new_rows, new_cols)
                .into_iter()
                .filter(|seat_idx| !self.empty_seats[*seat_idx])
                .collect();
        }

        for preset in &mut self.target_presets {
            preset.targets = Self::remap_indices(&preset.targets, old_cols, new_rows, new_cols)
                .into_iter()
                .filter(|seat_idx| !self.empty_seats[*seat_idx])
                .collect();
        }

        if let Some(idx) = self.selected_student {
            if idx >= self.students.len() {
                self.selected_student = self.students.len().checked_sub(1);
            }
        }

        self.clear_result_if_needed();
        self.clear_messages();
    }

    fn toggle_empty_seat(&mut self, seat_idx: usize) {
        if seat_idx >= self.empty_seats.len() {
            return;
        }

        self.empty_seats[seat_idx] = !self.empty_seats[seat_idx];

        if self.empty_seats[seat_idx] {
            for student in &mut self.students {
                student.targets.retain(|idx| *idx != seat_idx);
            }

            for preset in &mut self.target_presets {
                preset.targets.retain(|idx| *idx != seat_idx);
            }
        }

        self.clear_result_if_needed();
        self.clear_messages();
    }

    fn toggle_target(&mut self, student_idx: usize, seat_idx: usize) {
        if student_idx >= self.students.len()
            || seat_idx >= self.seat_count()
            || self.empty_seats[seat_idx]
        {
            return;
        }

        if let Some(pos) = self.students[student_idx]
            .targets
            .iter()
            .position(|idx| *idx == seat_idx)
        {
            self.students[student_idx].targets.remove(pos);
        } else {
            self.students[student_idx].targets.push(seat_idx);
            self.students[student_idx].targets.sort_unstable();
            self.students[student_idx].targets.dedup();
        }

        self.clear_result_if_needed();
        self.clear_messages();
    }

    fn clear_targets(&mut self, student_idx: usize) {
        if student_idx >= self.students.len() {
            return;
        }
        self.students[student_idx].targets.clear();
        self.clear_result_if_needed();
        self.clear_messages();
    }

    fn next_unused_id(used: &HashSet<u16>, mut start: u16) -> u16 {
        if start == 0 {
            start = 1;
        }

        for id in start..=u16::MAX {
            if !used.contains(&id) {
                return id;
            }
        }
        for id in 1..start {
            if !used.contains(&id) {
                return id;
            }
        }
        start
    }

    fn assign_student_ids(&self) -> Vec<u16> {
        let mut assigned = Vec::with_capacity(self.students.len());
        let mut used = HashSet::new();

        for (index, student) in self.students.iter().enumerate() {
            let fallback = u16::try_from(index + 1).unwrap_or(u16::MAX);
            let preferred = student.id.unwrap_or(fallback);
            let id = Self::next_unused_id(&used, preferred);
            used.insert(id);
            assigned.push(id);
        }

        assigned
    }

    fn student_display_name(student: &StudentForm, idx: usize) -> String {
        let name = format!("{}{}", student.last_name.trim(), student.first_name.trim());
        if name.is_empty() {
            format!("生徒{}", idx + 1)
        } else {
            name
        }
    }

    fn profile_from_form(form: &StudentForm, idx: usize) -> StudentProfile {
        let mut last_name = form.last_name.trim().to_string();
        let first_name = form.first_name.trim().to_string();

        if last_name.is_empty() && first_name.is_empty() {
            last_name = format!("生徒{}", idx + 1);
        }

        StudentProfile {
            last_name,
            first_name,
            last_kana: form.last_kana.trim().to_string(),
            first_kana: form.first_kana.trim().to_string(),
            targets: form.targets.clone(),
        }
    }

    fn build_students(&self) -> Vec<Student> {
        let assigned_ids = self.assign_student_ids();

        self.students
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let mut targets = entry
                    .targets
                    .iter()
                    .filter(|seat_idx| **seat_idx < self.seat_count() && !self.empty_seats[**seat_idx])
                    .map(|seat_idx| Target::new(seat_idx % self.cols, seat_idx / self.cols))
                    .collect::<Vec<_>>();

                targets.sort_by_key(|t| (t.r, t.c));
                targets.dedup();

                let name = Self::student_display_name(entry, i);
                let number = assigned_ids.get(i).copied().unwrap_or(u16::MAX);
                Student::new(
                    &name,
                    number,
                    targets,
                    entry.close_to.clone(),
                    entry.avoid.clone(),
                    entry.seat_pref.clone(),
                )
            })
            .collect()
    }

    fn build_students_map(&self) -> BTreeMap<u16, StudentProfile> {
        let assigned_ids = self.assign_student_ids();
        let mut students = BTreeMap::new();

        for (idx, form) in self.students.iter().enumerate() {
            if idx >= assigned_ids.len() {
                continue;
            }
            let id = assigned_ids[idx];
            students.insert(id, Self::profile_from_form(form, idx));
        }

        students
    }

    fn output_date(&self) -> Result<String, String> {
        if self.use_custom_date {
            let value = self.custom_date.trim();
            if value.is_empty() {
                return Err("カスタム日付が空です。日付を入力してください。".to_string());
            }
            Ok(value.to_string())
        } else {
            Ok(Local::now().format("%Y/%m/%d").to_string())
        }
    }

    fn empty_seat_indices(&self) -> Vec<usize> {
        self.empty_seats
            .iter()
            .enumerate()
            .filter_map(|(idx, is_empty)| if *is_empty { Some(idx) } else { None })
            .collect()
    }

    fn run_solver(&mut self) {
        self.clear_messages();

        let students = self.build_students();
        if students.is_empty() {
            self.result = None;
            self.set_error("生徒を1人以上追加してください。");
            return;
        }

        let available = self.available_seat_count();
        if students.len() > available {
            self.result = None;
            self.set_error(format!(
                "生徒数({})が利用可能席数({})を超えています。",
                students.len(),
                available
            ));
            return;
        }

        let empty = self.empty_seat_indices();
        match find_best_seating_with_blocked(&students, self.rows, self.cols, &empty, self.config) {
            Ok(result) => {
                self.result = Some(result);
                self.set_info("席替えを実行しました。");
            }
            Err(err) => {
                self.result = None;
                self.set_error(err.to_string());
            }
        }
    }

    fn targets_to_summary(&self, targets: &[usize]) -> String {
        if targets.is_empty() {
            return "希望席なし(どこでも可)".to_string();
        }

        targets
            .iter()
            .map(|idx| self.coord_label(*idx))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn target_summary(&self, student_idx: usize) -> String {
        if student_idx >= self.students.len() {
            return String::new();
        }
        self.targets_to_summary(&self.students[student_idx].targets)
    }

    fn register_current_as_preset(&mut self, student_idx: usize) {
        if student_idx >= self.students.len() {
            return;
        }

        let name = self.new_preset_name.trim().to_string();
        if name.is_empty() {
            self.set_error("プリセット名を入力してください。");
            return;
        }

        let targets = Self::sanitize_targets_for_grid(
            self.seat_count(),
            &self.empty_seats,
            &self.students[student_idx].targets,
        );


        if let Some(existing_idx) = self.target_presets.iter().position(|preset| preset.name == name)
        {
            self.target_presets[existing_idx].targets = targets;
            self.set_info(format!("プリセット '{}' を更新しました。", name));
        } else {
            self.target_presets.push(TargetPreset {
                name: name.clone(),
                targets,
            });
            self.set_info(format!("プリセット '{}' を追加しました。", name));
        }
    }

    fn apply_preset_to_student(&mut self, student_idx: usize, preset_idx: usize) {
        if student_idx >= self.students.len() || preset_idx >= self.target_presets.len() {
            return;
        }

        let preset_name = self.target_presets[preset_idx].name.clone();
        let targets = Self::sanitize_targets_for_grid(
            self.seat_count(),
            &self.empty_seats,
            &self.target_presets[preset_idx].targets,
        );

        self.students[student_idx].targets = targets;
        self.clear_result_if_needed();
        self.set_info(format!("プリセット '{}' を適用しました。", preset_name));
    }

    fn path_from_input(input: &str, default_value: &str) -> PathBuf {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            PathBuf::from(default_value)
        } else {
            PathBuf::from(trimmed)
        }
    }

    fn absolute_path(path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }

        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => path.to_path_buf(),
        }
    }

    fn pick_students_json_path(&mut self) {
        if let Some(path) = FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
            self.students_json_path = path.to_string_lossy().to_string();
        }
    }

    fn pick_typ_path(&mut self) {
        if let Some(path) = FileDialog::new().add_filter("Typst", &["typ"]).pick_file() {
            self.typ_path = path.to_string_lossy().to_string();
        }
    }

    fn pick_pdf_output_path(&mut self) {
        if let Some(path) = FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("seats.pdf")
            .save_file()
        {
            self.pdf_output_path = path.to_string_lossy().to_string();
        }
    }

    fn pick_png_output_path(&mut self) {
        if let Some(path) = FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("seats.png")
            .save_file()
        {
            self.png_output_path = path.to_string_lossy().to_string();
        }
    }

    fn pick_svg_output_path(&mut self) {
        if let Some(path) = FileDialog::new()
            .add_filter("SVG", &["svg"])
            .set_file_name("seats.svg")
            .save_file()
        {
            self.svg_output_path = path.to_string_lossy().to_string();
        }
    }

    fn load_students_from_json(&mut self) {
        self.clear_messages();

        let path = Self::path_from_input(&self.students_json_path, "./students.json");
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

        let raw: BTreeMap<String, StudentProfile> = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(err) => {
                self.set_error(format!("json の形式が不正です: {}", err));
                return;
            }
        };

        let mut parsed = Vec::new();
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
            parsed.push((id, profile));
        }
        parsed.sort_by_key(|(id, _)| *id);

        self.students = parsed
            .into_iter()
            .map(|(id, profile)| StudentForm {
                id: Some(id),
                last_name: profile.last_name,
                first_name: profile.first_name,
                last_kana: profile.last_kana,
                first_kana: profile.first_kana,
                targets: profile.targets,
                close_to: Vec::new(),
                avoid: Vec::new(),
                seat_pref: None,
            })
            .collect();

        self.selected_student = if self.students.is_empty() {
            None
        } else {
            Some(0)
        };

        self.clear_result_if_needed();
        self.set_info(format!(
            "{} 人の生徒情報を読み込みました。",
            self.students.len()
        ));
    }

    fn write_json_value<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "{} 出力先ディレクトリの作成に失敗しました: {} ({})",
                        label,
                        parent.display(),
                        err
                    )
                })?;
            }
        }

        let json = serde_json::to_string_pretty(value)
            .map_err(|err| format!("{} のJSON生成に失敗しました: {}", label, err))?;

        fs::write(path, json)
            .map_err(|err| format!("{} の書き込みに失敗しました: {} ({})", label, path.display(), err))
    }

    fn export_students_json(&mut self) {
        self.clear_messages();

        let students_map = self.build_students_map();
        if students_map.is_empty() {
            self.set_error("書き出す生徒がいません。生徒を追加してください。");
            return;
        }

        let path = Self::path_from_input(&self.students_json_path, "./students.json");
        match Self::write_json_value(&path, &students_map, "students.json") {
            Ok(()) => self.set_info(format!("students.json を出力しました: {}", path.display())),
            Err(err) => self.set_error(err),
        }
    }

    fn build_seats_json_document(&self) -> Result<SeatsJsonDocument, String> {
        let result = self
            .result
            .as_ref()
            .ok_or_else(|| "先に「席替えを実行」を押してください。".to_string())?;

        let assigned_ids = self.assign_student_ids();
        let mut seats = vec![vec![None; self.cols]; self.rows];

        for r in 0..self.rows {
            for c in 0..self.cols {
                let seat_idx = r * self.cols + c;

                if self.empty_seats[seat_idx] {
                    seats[r][c] = None;
                    continue;
                }

                if let Some(student_idx) = result.layout.get(seat_idx).and_then(|x| *x) {
                    if student_idx < assigned_ids.len() {
                        seats[r][c] = Some(assigned_ids[student_idx]);
                    }
                }
            }
        }

        let date = self.output_date()?;

        Ok(SeatsJsonDocument {
            date,
            layout: SeatsLayout {
                rows: self.rows,
                cols: self.cols,
            },
            seats,
            students: self.build_students_map(),
        })
    }

    fn export_seats_json(&mut self) {
        self.clear_messages();

        let document = match self.build_seats_json_document() {
            Ok(document) => document,
            Err(err) => {
                self.set_error(err);
                return;
            }
        };

        let path = Self::path_from_input(&self.seats_json_path, "./seats.json");
        match Self::write_json_value(&path, &document, "seats.json") {
            Ok(()) => self.set_info(format!("seats.json を出力しました: {}", path.display())),
            Err(err) => self.set_error(err),
        }
    }

    fn ensure_typst_input_json(&mut self, document: &SeatsJsonDocument) -> Result<PathBuf, String> {
        let seats_path = Self::path_from_input(&self.seats_json_path, "./seats.json");
        Self::write_json_value(&seats_path, document, "seats.json")?;

        let typ_path = Self::path_from_input(&self.typ_path, "./seats.typ");
        if !typ_path.exists() {
            return Err(format!(
                "Typst ファイルが見つかりません: {}",
                typ_path.display()
            ));
        }

        // seats.typ 側が json("seats.json") を参照する前提なので、同階層にも配置する。
        let typ_dir = typ_path.parent().unwrap_or_else(|| Path::new("."));
        let typ_local_json = typ_dir.join("seats.json");
        if Self::absolute_path(&typ_local_json) != Self::absolute_path(&seats_path) {
            Self::write_json_value(&typ_local_json, document, "seats.json")?;
        }

        Ok(typ_path)
    }

    fn compile_typst(
        typ_path: &Path,
        output_path: &Path,
        format: &str,
        png_ppi: Option<u16>,
    ) -> Result<(), String> {
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "出力先ディレクトリの作成に失敗しました: {} ({})",
                        parent.display(),
                        err
                    )
                })?;
            }
        }

        let mut cmd = Command::new("typst");
        cmd.arg("compile")
            .arg("--format")
            .arg(format)
            .arg(typ_path)
            .arg(output_path);

        if let Some(ppi) = png_ppi {
            cmd.arg("--ppi").arg(ppi.to_string());
        }

        let output = cmd
            .output()
            .map_err(|err| format!("typst コマンドの実行に失敗しました: {}", err))?;

        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "不明なエラー".to_string()
        };

        Err(format!(
            "{} 生成に失敗しました ({}): {}",
            format.to_uppercase(),
            output_path.display(),
            detail
        ))
    }

    fn generate_typst_outputs(&mut self) {
        self.clear_messages();

        if !self.export_pdf && !self.export_png && !self.export_svg {
            self.set_error("出力形式を1つ以上選択してください。");
            return;
        }

        let document = match self.build_seats_json_document() {
            Ok(document) => document,
            Err(err) => {
                self.set_error(err);
                return;
            }
        };

        let typ_path = match self.ensure_typst_input_json(&document) {
            Ok(path) => path,
            Err(err) => {
                self.set_error(err);
                return;
            }
        };

        let mut success = Vec::new();
        let mut failures = Vec::new();

        if self.export_pdf {
            let path = Self::path_from_input(&self.pdf_output_path, "./seats.pdf");
            match Self::compile_typst(&typ_path, &path, "pdf", None) {
                Ok(()) => success.push(format!("PDF: {}", path.display())),
                Err(err) => failures.push(err),
            }
        }

        if self.export_png {
            let path = Self::path_from_input(&self.png_output_path, "./seats.png");
            match Self::compile_typst(&typ_path, &path, "png", Some(self.png_ppi)) {
                Ok(()) => success.push(format!("PNG: {} ({} ppi)", path.display(), self.png_ppi)),
                Err(err) => failures.push(err),
            }
        }

        if self.export_svg {
            let path = Self::path_from_input(&self.svg_output_path, "./seats.svg");
            match Self::compile_typst(&typ_path, &path, "svg", None) {
                Ok(()) => success.push(format!("SVG: {}", path.display())),
                Err(err) => failures.push(err),
            }
        }

        if failures.is_empty() {
            self.set_info(format!(
                "Typst出力が完了しました。{}",
                success.join(" / ")
            ));
            return;
        }

        if success.is_empty() {
            self.set_error(failures.join("\n"));
            return;
        }

        self.set_error(format!(
            "一部出力は成功しました。成功: {}\n失敗: {}",
            success.join(" / "),
            failures.join("\n")
        ));
    }

    fn fetch_preferences(&mut self) {
        self.clear_messages();

        if self.preferences_url.trim().is_empty() {
            self.set_error("Google Sheets API URLを入力してください。");
            return;
        }

        match fetch::fetch_student_preferences(&self.preferences_url) {
            Ok(preferences) => {
                // Merge preferences into existing students
                for student in &mut self.students {
                    if let Some(id) = student.id {
                        if let Some((close_to, avoid, targets_str)) = preferences.get(&id) {
                            student.close_to = close_to.clone();
                            student.avoid = avoid.clone();
                            student.targets = fetch::parse_targets(targets_str, self.rows, self.cols, &self.seat_preferences);
                        }
                    }
                }
                self.set_info(format!("{} 人の生徒の希望をフェッチしました。", preferences.len()));
            }
            Err(err) => {
                self.set_error(format!("フェッチに失敗しました: {}", err));
            }
        }
    }
}

fn install_japanese_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "noto_sans_jp".to_owned(),
        FontData::from_static(include_bytes!("fonts/UDEVGothic35NFLG-Regular.ttf")).into(),
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

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let seat_cell_size = self.seat_cell_size();

                    ui.heading("sekigae-rs");
                    ui.label(
                        "1) 空席を決める 2) 生徒情報を入力 3) 希望席を設定 4) 席替え 5) JSON / Typst 出力",
                    );

                    ui.separator();
                    ui.heading("基本設定");
                    let mut new_rows = self.rows;
                    let mut new_cols = self.cols;

                    ui.horizontal(|ui| {
                        ui.label("行数");
                        ui.add(eframe::egui::DragValue::new(&mut new_rows).range(1..=20));
                        ui.label("列数");
                        ui.add(eframe::egui::DragValue::new(&mut new_cols).range(1..=20));
                        ui.separator();
                        ui.label("反復回数");
                        ui.add(
                            eframe::egui::DragValue::new(&mut self.config.iterations)
                                .range(100..=2_000_000),
                        );
                    });

                    ui.horizontal(|ui| {
                        ui.label("開始温度");
                        ui.add(
                            eframe::egui::DragValue::new(&mut self.config.start_temp)
                                .speed(0.05)
                                .range(0.01..=1000.0),
                        );
                        ui.label("終了温度");
                        ui.add(
                            eframe::egui::DragValue::new(&mut self.config.end_temp)
                                .speed(0.01)
                                .range(0.0001..=100.0),
                        );
                        ui.label("ランダム度");
                        ui.add(eframe::egui::Slider::new(&mut self.config.randomness, 0.0..=1.0).suffix(""));
                    });

                    ui.horizontal(|ui| {
                        ui.label("フォントサイズ");
                        ui.add(eframe::egui::Slider::new(&mut self.ui_font_size, 12.0..=36.0).suffix(" px"));
                        ui.separator();
                        ui.checkbox(&mut self.show_debug_status, "結果にOK/NGを表示");
                    });

                    if new_rows != self.rows || new_cols != self.cols {
                        self.resize_grid(new_rows, new_cols);
                    }

                    ui.label(format!(
                        "総席数: {} / 空席: {} / 利用可能席: {} / 生徒数: {}",
                        self.seat_count(),
                        self.empty_seats.iter().filter(|is_empty| **is_empty).count(),
                        self.available_seat_count(),
                        self.students.len()
                    ));

                    ui.separator();
                    ui.heading("JSON / Typst 連携");

                    let mut pick_students_json = false;
                    let mut load_students_json = false;
                    let mut export_students_json = false;
                    let mut export_seats_json = false;
                    let mut pick_typ_file = false;
                    let mut pick_pdf_file = false;
                    let mut pick_png_file = false;
                    let mut pick_svg_file = false;
                    let mut generate_typst = false;
                    let mut fetch_preferences = false;

                    ui.horizontal(|ui| {
                        ui.label("students.json");
                        ui.text_edit_singleline(&mut self.students_json_path);
                        if ui.button("参照").clicked() {
                            pick_students_json = true;
                        }
                        if ui.button("読み込む").clicked() {
                            load_students_json = true;
                        }
                        if ui.button("書き出す").clicked() {
                            export_students_json = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("seats.json 出力先");
                        ui.text_edit_singleline(&mut self.seats_json_path);
                        if ui.button("書き出す").clicked() {
                            export_seats_json = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("seats.jsonの日付");
                        ui.radio_value(&mut self.use_custom_date, false, "実行日");
                        ui.radio_value(&mut self.use_custom_date, true, "カスタム");
                        if self.use_custom_date {
                            ui.label("値");
                            ui.text_edit_singleline(&mut self.custom_date);
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("seats.typ");
                        ui.text_edit_singleline(&mut self.typ_path);
                        if ui.button("参照").clicked() {
                            pick_typ_file = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("出力形式");
                        ui.checkbox(&mut self.export_pdf, "PDF");
                        ui.checkbox(&mut self.export_png, "PNG");
                        ui.checkbox(&mut self.export_svg, "SVG");
                        if self.export_png {
                            ui.separator();
                            ui.label("PNG PPI");
                            ui.add(eframe::egui::DragValue::new(&mut self.png_ppi).range(72..=1200));
                        }
                    });

                    if self.export_pdf {
                        ui.horizontal(|ui| {
                            ui.label("PDF 出力先");
                            ui.text_edit_singleline(&mut self.pdf_output_path);
                            if ui.button("参照").clicked() {
                                pick_pdf_file = true;
                            }
                        });
                    }

                    if self.export_png {
                        ui.horizontal(|ui| {
                            ui.label("PNG 出力先");
                            ui.text_edit_singleline(&mut self.png_output_path);
                            if ui.button("参照").clicked() {
                                pick_png_file = true;
                            }
                        });
                    }

                    if self.export_svg {
                        ui.horizontal(|ui| {
                            ui.label("SVG 出力先");
                            ui.text_edit_singleline(&mut self.svg_output_path);
                            if ui.button("参照").clicked() {
                                pick_svg_file = true;
                            }
                        });
                    }

                    if ui.button("Typstで選択形式を生成").clicked() {
                        generate_typst = true;
                    }

                    ui.horizontal(|ui| {
                        ui.label("初期設定を取得");
                        if ui.button("実行").clicked() {
                            fetch_preferences = true;
                        }
                    });

                    if pick_students_json {
                        self.pick_students_json_path();
                    }
                    if load_students_json {
                        self.load_students_from_json();
                    }
                    if export_students_json {
                        self.export_students_json();
                    }
                    if export_seats_json {
                        self.export_seats_json();
                    }
                    if pick_typ_file {
                        self.pick_typ_path();
                    }
                    if pick_pdf_file {
                        self.pick_pdf_output_path();
                    }
                    if pick_png_file {
                        self.pick_png_output_path();
                    }
                    if pick_svg_file {
                        self.pick_svg_output_path();
                    }
                    if generate_typst {
                        self.generate_typst_outputs();
                    }
                    if fetch_preferences {
                        self.fetch_preferences();
                    }

                    ui.separator();
                    ui.heading("1. 空席位置の設定");
                    ui.label(
                        "赤いマスが空席固定。クリックで切り替え。空席にしたマスは希望席から自動で外れます。",
                    );

                    egui::Grid::new("empty-seat-grid")
                        .num_columns(self.cols)
                        .spacing([4.0, 4.0])
                        .show(ui, |ui| {
                            for r in 0..self.rows {
                                for c in 0..self.cols {
                                    let idx = r * self.cols + c;
                                    let is_empty = self.empty_seats[idx];
                                    let label = self.coord_label(idx);
                                    let button = Button::new(
                                        RichText::new(label).color(if is_empty {
                                            Color32::WHITE
                                        } else {
                                            Color32::BLACK
                                        }),
                                    )
                                    .fill(if is_empty {
                                        Color32::from_rgb(180, 40, 40)
                                    } else {
                                        Color32::from_rgb(210, 210, 210)
                                    });

                                    if ui.add_sized(seat_cell_size, button).clicked() {
                                        self.toggle_empty_seat(idx);
                                    }
                                }
                                ui.end_row();
                            }
                        });

                    ui.separator();
                    ui.heading("2. 生徒名の入力");

                    let mut remove_idx: Option<usize> = None;
                    let mut student_changed = false;

                    for i in 0..self.students.len() {
                        ui.horizontal(|ui| {
                            let label = if let Some(id) = self.students[i].id {
                                format!("{}: #{}", i + 1, id)
                            } else {
                                format!("{}:", i + 1)
                            };
                            ui.label(label);

                            ui.label("番号");
                            let mut id_str = self.students[i].id.map_or(String::new(), |id| id.to_string());
                            if ui.text_edit_singleline(&mut id_str).changed() {
                                if id_str.trim().is_empty() {
                                    self.students[i].id = None;
                                } else if let Ok(id) = id_str.trim().parse::<u16>() {
                                    self.students[i].id = Some(id);
                                }
                                student_changed = true;
                            }

                            ui.label("姓");
                            if ui.text_edit_singleline(&mut self.students[i].last_name).changed() {
                                student_changed = true;
                            }

                            ui.label("名");
                            if ui.text_edit_singleline(&mut self.students[i].first_name).changed() {
                                student_changed = true;
                            }

                            ui.label("セイ");
                            if ui.text_edit_singleline(&mut self.students[i].last_kana).changed() {
                                student_changed = true;
                            }

                            ui.label("メイ");
                            if ui.text_edit_singleline(&mut self.students[i].first_kana).changed() {
                                student_changed = true;
                            }

                            let selected = self.selected_student == Some(i);
                            if ui.selectable_label(selected, "希望席を編集").clicked() {
                                self.selected_student = Some(i);
                            }

                            if ui.button("削除").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    }

                    if student_changed {
                        self.clear_result_if_needed();
                        self.clear_messages();
                    }

                    if let Some(idx) = remove_idx {
                        self.students.remove(idx);

                        match self.selected_student {
                            Some(_) if self.students.is_empty() => self.selected_student = None,
                            Some(s) if s == idx => {
                                self.selected_student = self.students.len().checked_sub(1)
                            }
                            Some(s) if s > idx => self.selected_student = Some(s - 1),
                            _ => {}
                        }

                        self.clear_result_if_needed();
                        self.clear_messages();
                    }

                    if ui.button("生徒を追加").clicked() {
                        self.students.push(StudentForm {
                            id: None,
                            last_name: String::new(),
                            first_name: String::new(),
                            last_kana: String::new(),
                            first_kana: String::new(),
                            targets: Vec::new(),
                            close_to: Vec::new(),
                            avoid: Vec::new(),
                            seat_pref: None,
                        });
                        if self.selected_student.is_none() {
                            self.selected_student = Some(0);
                        }
                        self.clear_result_if_needed();
                        self.clear_messages();
                    }

                    ui.separator();
                    ui.heading("3. 生徒ごとの希望席設定");
                    if let Some(student_idx) = self.selected_student {
                        if student_idx < self.students.len() {
                            let display_name =
                                Self::student_display_name(&self.students[student_idx], student_idx);
                            ui.label(format!("編集中: {}", display_name));
                            ui.label(format!("現在の希望席: {}", self.target_summary(student_idx)));

                            if ui.button("この生徒の希望席をクリア").clicked() {
                                self.clear_targets(student_idx);
                            }

                            ui.horizontal(|ui| {
                                ui.label("プリセット名");
                                ui.text_edit_singleline(&mut self.new_preset_name);
                                if ui.button("現在の希望席設定を登録").clicked() {
                                    self.register_current_as_preset(student_idx);
                                }
                            });

                            if self.target_presets.is_empty() {
                                ui.label("登録済みプリセットはありません。");
                            } else {
                                let mut apply_preset_idx: Option<usize> = None;
                                let mut remove_preset_idx: Option<usize> = None;

                                for (preset_idx, preset) in self.target_presets.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.label(format!(
                                            "{}: {}",
                                            preset.name,
                                            self.targets_to_summary(&preset.targets)
                                        ));

                                        if ui.button("適用").clicked() {
                                            apply_preset_idx = Some(preset_idx);
                                        }
                                        if ui.button("削除").clicked() {
                                            remove_preset_idx = Some(preset_idx);
                                        }
                                    });
                                }

                                if let Some(preset_idx) = apply_preset_idx {
                                    self.apply_preset_to_student(student_idx, preset_idx);
                                }
                                if let Some(preset_idx) = remove_preset_idx {
                                    let removed = self.target_presets.remove(preset_idx);
                                    self.set_info(format!(
                                        "プリセット '{}' を削除しました。",
                                        removed.name
                                    ));
                                }
                            }

                            egui::Grid::new("target-seat-grid")
                                .num_columns(self.cols)
                                .spacing([4.0, 4.0])
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

                                            let selected = self.students[student_idx]
                                                .targets
                                                .iter()
                                                .any(|seat_idx| *seat_idx == idx);

                                            let button = Button::new(
                                                RichText::new(label).color(if selected {
                                                    Color32::WHITE
                                                } else {
                                                    Color32::BLACK
                                                }),
                                            )
                                            .fill(if selected {
                                                Color32::from_rgb(50, 130, 80)
                                            } else {
                                                Color32::from_rgb(210, 210, 210)
                                            });

                                            if ui.add_sized(seat_cell_size, button).clicked() {
                                                self.toggle_target(student_idx, idx);
                                            }
                                        }
                                        ui.end_row();
                                    }
                                });
                        }
                    } else {
                        ui.label("まず生徒を追加し、\"希望席を編集\"を選んでください。");
                    }

                    ui.separator();
                    if ui
                        .add_sized([220.0, 36.0], Button::new("席替えを実行"))
                        .clicked()
                    {
                        self.run_solver();
                    }

                    if let Some(info) = &self.last_info {
                        ui.colored_label(Color32::from_rgb(40, 140, 60), info);
                    }
                    if let Some(err) = &self.last_error {
                        ui.colored_label(Color32::from_rgb(220, 40, 40), err);
                    }

                    if let Some(result) = &self.result {
                        let built_students = self.build_students();

                        ui.separator();
                        ui.heading("4. 席替え結果");
                        ui.label(format!(
                            "満足人数: {}/{}  ボーナス: {}  希望ボーナス: {}, 希望ペナルティ: {}",
                            result.satisfied,
                            built_students.len(),
                            result.weighted_bonus,
                            result.preference_bonus,
                            result.preference_penalty
                        ));

                        egui::Grid::new("result-grid")
                            .num_columns(self.cols)
                            .spacing([4.0, 4.0])
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

                                        let text = match result.layout.get(idx).and_then(|x| *x) {
                                            Some(student_idx)
                                                if student_idx < built_students.len() =>
                                            {
                                                let student = &built_students[student_idx];
                                                let seat = Target::new(c, r);
                                                let mark = if student.is_satisfied_at(seat) {
                                                    "OK"
                                                } else {
                                                    "NG"
                                                };
                                                if self.show_debug_status {
                                                    format!(
                                                        "{}({})\n{}",
                                                        student.name, student.number, mark
                                                    )
                                                } else {
                                                    format!("{}({})", student.name, student.number)
                                                }
                                            }
                                            _ => "-".to_string(),
                                        };

                                        ui.add_sized(seat_cell_size, Button::new(text));
                                    }
                                    ui.end_row();
                                }
                            });
                    }
                });
        });
    }
}
