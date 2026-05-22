use chrono::Local;
use eframe::egui::{
    self, Button, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle,
};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use typst::layout::PagedDocument;
use typst_as_lib::TypstEngine;

#[cfg(feature = "google-fetch")]
use crate::fetch::{fetch_student_preferences, load_preferences_config, parse_targets};
use crate::model::{AnnealingConfig, SeatingResult, Student, Target};
use crate::solver::find_best_seating_with_blocked;

const APP_TITLE: &str = "sekigae-rs";
const DEFAULT_SEATS_TYP_TEMPLATE: &str = include_str!("../seats.typ");
const DEFAULT_ROWS: usize = 4;
const DEFAULT_COLS: usize = 5;
const DEFAULT_STUDENTS_JSON_PATH: &str = "./students.json";
const DEFAULT_SEATS_JSON_PATH: &str = "./seats.json";
const DEFAULT_TYP_PATH: &str = "./seats.typ";
const DEFAULT_PDF_OUTPUT_PATH: &str = "./seats.pdf";
const DEFAULT_PNG_OUTPUT_PATH: &str = "./seats.png";
const DEFAULT_SVG_OUTPUT_PATH: &str = "./seats.svg";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UiStage {
    Setup,
    Students,
    Targets,
    SolveExport,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum TargetEditMode {
    #[default]
    Soft,
    Forced,
}

impl TargetEditMode {
    fn title(self) -> &'static str {
        match self {
            TargetEditMode::Soft => "希望席",
            TargetEditMode::Forced => "確定希望",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RelationKind {
    CloseTo,
    Avoid,
}

impl RelationKind {
    fn title(self) -> &'static str {
        match self {
            RelationKind::CloseTo => "隣になりたい生徒 (sekigae3)",
            RelationKind::Avoid => "遠ざかりたい生徒 (sekigae3)",
        }
    }

    fn summary_label(self) -> &'static str {
        match self {
            RelationKind::CloseTo => "隣希望",
            RelationKind::Avoid => "遠ざかり希望",
        }
    }

    fn clear_button_label(self) -> &'static str {
        match self {
            RelationKind::CloseTo => "この生徒の隣希望をクリア",
            RelationKind::Avoid => "この生徒の遠ざかり希望をクリア",
        }
    }

    fn scroll_id(self) -> &'static str {
        match self {
            RelationKind::CloseTo => "targets-close-to-options-scroll",
            RelationKind::Avoid => "targets-avoid-options-scroll",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResultDisplayMode {
    All,
    Random,
}

impl ResultDisplayMode {
    fn title(self) -> &'static str {
        match self {
            ResultDisplayMode::All => "一括表示",
            ResultDisplayMode::Random => "ランダム表示",
        }
    }
}

impl UiStage {
    const ALL: [UiStage; 4] = [
        UiStage::Setup,
        UiStage::Students,
        UiStage::Targets,
        UiStage::SolveExport,
    ];

    fn title(self) -> &'static str {
        match self {
            UiStage::Setup => "基本設定・座席形状",
            UiStage::Students => "学生情報入力",
            UiStage::Targets => "希望席設定",
            UiStage::SolveExport => "実行・出力",
        }
    }

    fn description(self) -> &'static str {
        match self {
            UiStage::Setup => "座席サイズ/形状、探索パラメータ、表示設定を調整します。",
            UiStage::Students => "生徒情報を入力し、編集対象の生徒を決めます。",
            UiStage::Targets => "選択した生徒の希望席と隣希望を設定します。",
            UiStage::SolveExport => "席替え実行、結果確認、JSON/Typst 出力を行います。",
        }
    }
}

pub struct SekigaeApp {
    rows: usize,
    cols: usize,
    current_stage: UiStage,
    empty_seats: Vec<bool>,
    students: Vec<StudentForm>,
    selected_student: Option<usize>,
    target_presets: Vec<TargetPreset>,
    new_preset_name: String,
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
    target_edit_mode: TargetEditMode,
    result_display_mode: ResultDisplayMode,
    result_fullscreen: bool,
    result: Option<SeatingResult>,
    last_error: Option<String>,
    last_info: Option<String>,
    is_solving: bool,
    solver_rx: Option<Receiver<Result<SeatingResult, String>>>,
    // アニメーション表示用
    animation_displayed_indices: Vec<usize>,
    animation_last_update: std::time::Instant,
}

#[derive(Clone, Debug, Default)]
struct StudentForm {
    id: Option<u16>,
    last_name: String,
    first_name: String,
    last_kana: String,
    first_kana: String,
    targets: Vec<usize>,
    forced_targets: Vec<usize>,
    close_to: Vec<u16>,
    forced_close_to: Vec<u16>,
    avoid: Vec<u16>,
    forced_avoid: Vec<u16>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TargetPreset {
    name: String,
    #[serde(default)]
    mode: TargetEditMode,
    targets: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StudentsJsonDocument {
    students: BTreeMap<u16, StudentProfile>,
    #[serde(default)]
    target_presets: Vec<TargetPreset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StudentProfile {
    last_name: String,
    first_name: String,
    last_kana: String,
    first_kana: String,
    targets: Vec<usize>,
    #[serde(default)]
    forced_targets: Vec<usize>,
    #[serde(default)]
    close_to: Vec<u16>,
    #[serde(default)]
    forced_close_to: Vec<u16>,
    #[serde(default)]
    avoid: Vec<u16>,
    #[serde(default)]
    forced_avoid: Vec<u16>,
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
        Self::initial_state()
    }

    fn initial_state() -> Self {
        Self {
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            current_stage: UiStage::Setup,
            empty_seats: vec![false; DEFAULT_ROWS * DEFAULT_COLS],
            students: vec![StudentForm::default()],
            selected_student: Some(0),
            target_presets: Vec::new(),
            new_preset_name: String::new(),
            use_custom_date: false,
            custom_date: Local::now().format("%Y/%m/%d").to_string(),
            students_json_path: DEFAULT_STUDENTS_JSON_PATH.to_string(),
            seats_json_path: DEFAULT_SEATS_JSON_PATH.to_string(),
            typ_path: DEFAULT_TYP_PATH.to_string(),
            pdf_output_path: DEFAULT_PDF_OUTPUT_PATH.to_string(),
            png_output_path: DEFAULT_PNG_OUTPUT_PATH.to_string(),
            svg_output_path: DEFAULT_SVG_OUTPUT_PATH.to_string(),
            export_pdf: true,
            export_png: false,
            export_svg: false,
            png_ppi: 144,
            config: AnnealingConfig {
                seed: 0,
                budget: DEFAULT_ROWS * DEFAULT_COLS,
                randomness: 0.0,
            },
            target_edit_mode: TargetEditMode::Soft,
            result_display_mode: ResultDisplayMode::Random,
            result_fullscreen: false,
            result: None,
            last_error: None,
            last_info: None,
            is_solving: false,
            solver_rx: None,
            animation_displayed_indices: Vec::new(),
            animation_last_update: Instant::now(),
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
        self.empty_seats
            .iter()
            .filter(|is_empty| !**is_empty)
            .count()
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

    fn set_window_busy_state(&self, ctx: &egui::Context, busy: bool) {
        let title = if busy {
            format!("{} (席替え中...)", APP_TITLE)
        } else {
            APP_TITLE.to_string()
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn poll_solver_result(&mut self, ctx: &egui::Context) {
        if !self.is_solving {
            return;
        }

        let Some(rx) = &self.solver_rx else {
            self.is_solving = false;
            self.set_window_busy_state(ctx, false);
            return;
        };

        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.result = Some(result);
                self.animation_displayed_indices.clear();
                self.animation_last_update = Instant::now();
                self.set_info("席替えを実行しました。".to_string());
                self.is_solving = false;
                self.solver_rx = None;
                self.set_window_busy_state(ctx, false);
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
            }
            Ok(Err(err)) => {
                self.result = None;
                self.set_error(err);
                self.is_solving = false;
                self.solver_rx = None;
                self.set_window_busy_state(ctx, false);
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(80));
            }
            Err(TryRecvError::Disconnected) => {
                self.result = None;
                self.set_error("席替え処理のスレッドが切断されました。".to_string());
                self.is_solving = false;
                self.solver_rx = None;
                self.set_window_busy_state(ctx, false);
            }
        }
    }

    fn reset_all(&mut self, ctx: &egui::Context) {
        *self = Self::initial_state();
        self.set_info("すべてリセットしました。");
        self.set_window_busy_state(ctx, false);
    }

    fn bubble_symbol_button(ui: &mut egui::Ui, symbol: &str, enabled: bool) -> bool {
        let button = Button::new(
            RichText::new(symbol)
                .strong()
                .color(Color32::from_rgb(35, 35, 35)),
        )
        .min_size(egui::vec2(18.0, 18.0))
        .fill(Color32::from_rgb(240, 240, 245));

        ui.add_enabled(enabled, button).clicked()
    }

    fn bubble_pair_cell(
        ui: &mut egui::Ui,
        cell_size: [f32; 2],
        plus_enabled: bool,
        minus_enabled: bool,
    ) -> (bool, bool) {
        let mut plus = false;
        let mut minus = false;

        ui.allocate_ui_with_layout(
            egui::vec2(cell_size[0], cell_size[1]),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                plus = Self::bubble_symbol_button(ui, " + ", plus_enabled);
                minus = Self::bubble_symbol_button(ui, " - ", minus_enabled);
            },
        );

        (plus, minus)
    }

    fn ensure_valid_selected_student(&mut self) {
        if self.students.is_empty() {
            self.selected_student = None;
            return;
        }

        let needs_reset = match self.selected_student {
            Some(idx) => idx >= self.students.len(),
            None => true,
        };

        if needs_reset {
            self.selected_student = Some(0);
        }
    }

    fn current_stage_index(&self) -> usize {
        UiStage::ALL
            .iter()
            .position(|stage| *stage == self.current_stage)
            .unwrap_or(0)
    }

    fn go_prev_stage(&mut self) {
        let idx = self.current_stage_index();
        if idx > 0 {
            self.current_stage = UiStage::ALL[idx - 1];
        }
    }

    fn go_next_stage(&mut self) {
        let idx = self.current_stage_index();
        if idx + 1 < UiStage::ALL.len() {
            self.current_stage = UiStage::ALL[idx + 1];
        }
    }

    fn seat_cell_size(&self) -> [f32; 2] {
        [87.6, 40.0]
    }

    fn result_cell_size(&self) -> [f32; 2] {
        [112.0, 54.0]
    }

    fn render_result_display_mode_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("表示方法:");
            let mut changed = false;
            changed |= ui
                .selectable_value(
                    &mut self.result_display_mode,
                    ResultDisplayMode::All,
                    ResultDisplayMode::All.title(),
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut self.result_display_mode,
                    ResultDisplayMode::Random,
                    ResultDisplayMode::Random.title(),
                )
                .changed();

            ui.separator();
            // 全画面表示は表示モードとは独立したトグルとする
            if ui
                .checkbox(&mut self.result_fullscreen, "全画面表示")
                .changed()
            {
                changed = true;
            }
            if changed {
                self.animation_displayed_indices.clear();
                self.animation_last_update = Instant::now();
            }
        });

        if self.result_display_mode == ResultDisplayMode::Random {
            ui.label("(1秒ごとにランダムに生徒を表示)");
        } else {
            ui.label("(一括で表示)");
        }
    }

    fn apply_text_style(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        let base: f32 = 18.0;

        style.text_styles.insert(
            TextStyle::Small,
            FontId::new((base - 2.0).max(8.0), FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(base, FontFamily::Proportional));
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

    fn sanitize_relation_ids(ids: &[u16], self_id: u16, valid_ids: &HashSet<u16>) -> Vec<u16> {
        let mut out = ids
            .iter()
            .copied()
            .filter(|id| *id != self_id && valid_ids.contains(id))
            .collect::<Vec<_>>();
        out.sort_unstable();
        out.dedup();
        out
    }

    fn relation_ids(student: &StudentForm, relation: RelationKind, mode: TargetEditMode) -> &[u16] {
        match (relation, mode) {
            (RelationKind::CloseTo, TargetEditMode::Soft) => &student.close_to,
            (RelationKind::CloseTo, TargetEditMode::Forced) => &student.forced_close_to,
            (RelationKind::Avoid, TargetEditMode::Soft) => &student.avoid,
            (RelationKind::Avoid, TargetEditMode::Forced) => &student.forced_avoid,
        }
    }

    fn relation_ids_mut(
        student: &mut StudentForm,
        relation: RelationKind,
        mode: TargetEditMode,
    ) -> &mut Vec<u16> {
        match (relation, mode) {
            (RelationKind::CloseTo, TargetEditMode::Soft) => &mut student.close_to,
            (RelationKind::CloseTo, TargetEditMode::Forced) => &mut student.forced_close_to,
            (RelationKind::Avoid, TargetEditMode::Soft) => &mut student.avoid,
            (RelationKind::Avoid, TargetEditMode::Forced) => &mut student.forced_avoid,
        }
    }

    fn relation_summary(ids: &[u16], id_to_name: &BTreeMap<u16, String>) -> String {
        if ids.is_empty() {
            return "指定なし".to_string();
        }

        ids.iter()
            .map(|id| {
                id_to_name
                    .get(id)
                    .map_or_else(|| format!("#{}", id), |name| format!("#{} {}", id, name))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn normalize_relation_list(ids: &mut Vec<u16>, self_id: u16, valid_ids: &HashSet<u16>) -> bool {
        let sanitized = Self::sanitize_relation_ids(ids, self_id, valid_ids);
        if *ids == sanitized {
            false
        } else {
            *ids = sanitized;
            true
        }
    }

    fn normalize_student_relations(
        student: &mut StudentForm,
        self_id: u16,
        valid_ids: &HashSet<u16>,
    ) -> bool {
        let mut changed = false;
        changed |= Self::normalize_relation_list(&mut student.close_to, self_id, valid_ids);
        changed |= Self::normalize_relation_list(&mut student.forced_close_to, self_id, valid_ids);
        changed |= Self::normalize_relation_list(&mut student.avoid, self_id, valid_ids);
        changed |= Self::normalize_relation_list(&mut student.forced_avoid, self_id, valid_ids);
        changed
    }

    fn toggle_relation_ids(
        ids: &mut Vec<u16>,
        toggled_ids: &[u16],
        self_id: u16,
        valid_ids: &HashSet<u16>,
    ) -> bool {
        if toggled_ids.is_empty() {
            return false;
        }

        let before = ids.clone();
        for &id in toggled_ids {
            if id == self_id {
                continue;
            }
            if let Some(pos) = ids.iter().position(|target| *target == id) {
                ids.remove(pos);
            } else {
                ids.push(id);
            }
        }
        Self::normalize_relation_list(ids, self_id, valid_ids);
        *ids != before
    }

    fn sanitize_target_list(&self, targets: &[usize]) -> Vec<usize> {
        Self::sanitize_targets_for_grid(self.seat_count(), &self.empty_seats, targets)
    }

    fn targets_to_model_targets(&self, indices: &[usize]) -> Vec<Target> {
        let mut targets = self
            .sanitize_target_list(indices)
            .into_iter()
            .map(|seat_idx| Target::new(seat_idx % self.cols, seat_idx / self.cols))
            .collect::<Vec<_>>();
        targets.sort_by_key(|t| (t.r, t.c));
        targets.dedup();
        targets
    }

    fn toggle_target_list(targets: &mut Vec<usize>, seat_idx: usize) {
        if let Some(pos) = targets.iter().position(|idx| *idx == seat_idx) {
            targets.remove(pos);
        } else {
            targets.push(seat_idx);
            targets.sort_unstable();
            targets.dedup();
        }
    }

    fn normalize_student_targets(
        student: &mut StudentForm,
        seat_count: usize,
        empty_seats: &[bool],
    ) {
        student.targets =
            Self::sanitize_targets_for_grid(seat_count, empty_seats, &student.targets);
        student.forced_targets =
            Self::sanitize_targets_for_grid(seat_count, empty_seats, &student.forced_targets);
        let forced = student.forced_targets.clone();
        student
            .targets
            .retain(|seat_idx| !forced.contains(seat_idx));
    }

    fn apply_grid_transform(
        &mut self,
        new_rows: usize,
        new_cols: usize,
        old_to_new: Vec<Option<usize>>,
    ) {
        let old_rows = self.rows;
        let old_cols = self.cols;
        let old_count = old_rows * old_cols;
        if old_to_new.len() != old_count {
            return;
        }

        let old_default_budget = old_count;
        let old_empty = self.empty_seats.clone();
        let mut new_empty = vec![false; new_rows * new_cols];

        for old_idx in 0..old_count {
            if !old_empty[old_idx] {
                continue;
            }
            if let Some(new_idx) = old_to_new[old_idx]
                && new_idx < new_empty.len()
            {
                new_empty[new_idx] = true;
            }
        }

        self.rows = new_rows;
        self.cols = new_cols;
        self.empty_seats = new_empty;

        for student in &mut self.students {
            let mut mapped = student
                .targets
                .iter()
                .filter_map(|old_idx| old_to_new.get(*old_idx).copied().flatten())
                .filter(|new_idx| *new_idx < self.empty_seats.len() && !self.empty_seats[*new_idx])
                .collect::<Vec<_>>();
            mapped.sort_unstable();
            mapped.dedup();
            student.targets = mapped;

            let mut forced_mapped = student
                .forced_targets
                .iter()
                .filter_map(|old_idx| old_to_new.get(*old_idx).copied().flatten())
                .filter(|new_idx| *new_idx < self.empty_seats.len() && !self.empty_seats[*new_idx])
                .collect::<Vec<_>>();
            forced_mapped.sort_unstable();
            forced_mapped.dedup();
            student.forced_targets = forced_mapped;
        }

        for preset in &mut self.target_presets {
            let mut mapped = preset
                .targets
                .iter()
                .filter_map(|old_idx| old_to_new.get(*old_idx).copied().flatten())
                .filter(|new_idx| *new_idx < self.empty_seats.len() && !self.empty_seats[*new_idx])
                .collect::<Vec<_>>();
            mapped.sort_unstable();
            mapped.dedup();
            preset.targets = mapped;
        }

        if let Some(idx) = self.selected_student
            && idx >= self.students.len()
        {
            self.selected_student = self.students.len().checked_sub(1);
        }

        // 既定値(座席数)のままなら、グリッド変更時に budget も追従させる。
        if self.config.budget == old_default_budget {
            self.config.budget = self.seat_count();
        }

        self.clear_result_if_needed();
        self.clear_messages();
    }

    fn resize_grid(&mut self, new_rows: usize, new_cols: usize) {
        let old_rows = self.rows;
        let old_cols = self.cols;
        let old_count = old_rows * old_cols;
        let mut old_to_new = vec![None; old_count];

        for (old_idx, mapped) in old_to_new.iter_mut().enumerate().take(old_count) {
            let r = old_idx / old_cols;
            let c = old_idx % old_cols;
            if r < new_rows && c < new_cols {
                *mapped = Some(r * new_cols + c);
            }
        }

        self.apply_grid_transform(new_rows, new_cols, old_to_new);
    }

    fn insert_row_at(&mut self, insert_before: usize) {
        if insert_before > self.rows {
            return;
        }

        let old_rows = self.rows;
        let old_cols = self.cols;
        let old_count = old_rows * old_cols;
        let mut old_to_new = vec![None; old_count];

        for (old_idx, mapped) in old_to_new.iter_mut().enumerate().take(old_count) {
            let r = old_idx / old_cols;
            let c = old_idx % old_cols;
            let new_r = if r >= insert_before { r + 1 } else { r };
            *mapped = Some(new_r * old_cols + c);
        }

        self.apply_grid_transform(old_rows + 1, old_cols, old_to_new);
    }

    fn delete_row_at(&mut self, row_idx: usize) {
        if self.rows <= 1 || row_idx >= self.rows {
            return;
        }

        let old_rows = self.rows;
        let old_cols = self.cols;
        let old_count = old_rows * old_cols;
        let mut old_to_new = vec![None; old_count];

        for (old_idx, mapped) in old_to_new.iter_mut().enumerate().take(old_count) {
            let r = old_idx / old_cols;
            let c = old_idx % old_cols;
            if r == row_idx {
                continue;
            }
            let new_r = if r > row_idx { r - 1 } else { r };
            *mapped = Some(new_r * old_cols + c);
        }

        self.apply_grid_transform(old_rows - 1, old_cols, old_to_new);
    }

    fn insert_col_at(&mut self, insert_before: usize) {
        if insert_before > self.cols {
            return;
        }

        let old_rows = self.rows;
        let old_cols = self.cols;
        let old_count = old_rows * old_cols;
        let mut old_to_new = vec![None; old_count];

        for (old_idx, mapped) in old_to_new.iter_mut().enumerate().take(old_count) {
            let r = old_idx / old_cols;
            let c = old_idx % old_cols;
            let new_c = if c >= insert_before { c + 1 } else { c };
            *mapped = Some(r * (old_cols + 1) + new_c);
        }

        self.apply_grid_transform(old_rows, old_cols + 1, old_to_new);
    }

    fn delete_col_at(&mut self, col_idx: usize) {
        if self.cols <= 1 || col_idx >= self.cols {
            return;
        }

        let old_rows = self.rows;
        let old_cols = self.cols;
        let old_count = old_rows * old_cols;
        let mut old_to_new = vec![None; old_count];

        for (old_idx, mapped) in old_to_new.iter_mut().enumerate().take(old_count) {
            let r = old_idx / old_cols;
            let c = old_idx % old_cols;
            if c == col_idx {
                continue;
            }
            let new_c = if c > col_idx { c - 1 } else { c };
            *mapped = Some(r * (old_cols - 1) + new_c);
        }

        self.apply_grid_transform(old_rows, old_cols - 1, old_to_new);
    }

    fn set_empty_seat_state(&mut self, seat_idx: usize, is_empty: bool) -> bool {
        if seat_idx >= self.empty_seats.len() || self.empty_seats[seat_idx] == is_empty {
            return false;
        }

        self.empty_seats[seat_idx] = is_empty;
        if is_empty {
            for student in &mut self.students {
                student.targets.retain(|idx| *idx != seat_idx);
                student.forced_targets.retain(|idx| *idx != seat_idx);
            }
            for preset in &mut self.target_presets {
                preset.targets.retain(|idx| *idx != seat_idx);
            }
        }
        true
    }

    fn toggle_target(&mut self, student_idx: usize, seat_idx: usize, forced: bool) {
        if student_idx >= self.students.len()
            || seat_idx >= self.seat_count()
            || self.empty_seats[seat_idx]
        {
            return;
        }

        let student = &mut self.students[student_idx];

        if forced {
            if let Some(pos) = student
                .forced_targets
                .iter()
                .position(|idx| *idx == seat_idx)
            {
                student.forced_targets.remove(pos);
            } else {
                Self::toggle_target_list(&mut student.forced_targets, seat_idx);
                if let Some(pos) = student.targets.iter().position(|idx| *idx == seat_idx) {
                    student.targets.remove(pos);
                }
            }
        } else if let Some(pos) = student.targets.iter().position(|idx| *idx == seat_idx) {
            student.targets.remove(pos);
        } else {
            Self::toggle_target_list(&mut student.targets, seat_idx);
            if let Some(pos) = student
                .forced_targets
                .iter()
                .position(|idx| *idx == seat_idx)
            {
                student.forced_targets.remove(pos);
            }
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

    fn clear_forced_targets(&mut self, student_idx: usize) {
        if student_idx >= self.students.len() {
            return;
        }
        self.students[student_idx].forced_targets.clear();
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
            forced_targets: form.forced_targets.clone(),
            close_to: form.close_to.clone(),
            forced_close_to: form.forced_close_to.clone(),
            avoid: form.avoid.clone(),
            forced_avoid: form.forced_avoid.clone(),
        }
    }

    fn sanitize_preset_targets(&self, targets: &[usize]) -> Vec<usize> {
        Self::sanitize_targets_for_grid(self.seat_count(), &self.empty_seats, targets)
    }

    fn upsert_target_preset(&mut self, preset: TargetPreset) {
        let mut preset = preset;
        preset.targets = self.sanitize_preset_targets(&preset.targets);

        if let Some(existing_idx) = self
            .target_presets
            .iter()
            .position(|existing| existing.name == preset.name && existing.mode == preset.mode)
        {
            self.target_presets[existing_idx] = preset;
        } else {
            self.target_presets.push(preset);
        }
    }

    fn build_students(&self) -> Vec<Student> {
        let assigned_ids = self.assign_student_ids();
        let valid_ids = assigned_ids.iter().copied().collect::<HashSet<_>>();

        self.students
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let targets = self.targets_to_model_targets(&entry.targets);
                let forced_targets = self.targets_to_model_targets(&entry.forced_targets);

                let name = Self::student_display_name(entry, i);
                let number = assigned_ids.get(i).copied().unwrap_or(u16::MAX);
                let close_to = Self::sanitize_relation_ids(&entry.close_to, number, &valid_ids);
                let forced_close_to =
                    Self::sanitize_relation_ids(&entry.forced_close_to, number, &valid_ids);
                Student {
                    name,
                    number,
                    targets,
                    forced_targets,
                    close_to,
                    forced_close_to,
                }
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

    fn run_solver(&mut self, ctx: &egui::Context) {
        if self.is_solving {
            return;
        }

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

        let rows = self.rows;
        let cols = self.cols;
        let empty = self.empty_seat_indices();
        let config = self.config;

        let (tx, rx) = mpsc::channel::<Result<SeatingResult, String>>();
        thread::spawn(move || {
            let result = find_best_seating_with_blocked(&students, rows, cols, &empty, config)
                .map_err(|err| err.to_string());
            let _ = tx.send(result);
        });

        self.result = None;
        self.is_solving = true;
        self.solver_rx = Some(rx);
        self.set_info("席替え中...".to_string());
        self.set_window_busy_state(ctx, true);
        ctx.request_repaint_after(Duration::from_millis(40));
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

    fn forced_target_summary(&self, student_idx: usize) -> String {
        if student_idx >= self.students.len() {
            return String::new();
        }
        self.targets_to_summary(&self.students[student_idx].forced_targets)
    }

    fn avoid_summary(&self, student_idx: usize) -> String {
        if student_idx >= self.students.len() {
            return String::new();
        }
        let avoid_ids = &self.students[student_idx].avoid;
        avoid_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", ")
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

        let mode = self.target_edit_mode;
        let targets = match mode {
            TargetEditMode::Soft => {
                self.sanitize_preset_targets(&self.students[student_idx].targets)
            }
            TargetEditMode::Forced => {
                self.sanitize_preset_targets(&self.students[student_idx].forced_targets)
            }
        };

        if let Some(existing_idx) = self
            .target_presets
            .iter()
            .position(|preset| preset.name == name && preset.mode == mode)
        {
            self.target_presets[existing_idx].targets = targets;
            self.set_info(format!(
                "{}プリセット '{}' を更新しました。",
                mode.title(),
                name
            ));
        } else {
            self.target_presets.push(TargetPreset {
                name: name.clone(),
                mode,
                targets,
            });
            self.set_info(format!(
                "{}プリセット '{}' を追加しました。",
                mode.title(),
                name
            ));
        }
    }

    fn apply_preset_to_student(&mut self, student_idx: usize, preset_idx: usize) {
        if student_idx >= self.students.len() || preset_idx >= self.target_presets.len() {
            return;
        }

        let preset = self.target_presets[preset_idx].clone();
        let preset_name = preset.name.clone();
        let mode = self.target_edit_mode;
        let targets =
            Self::sanitize_targets_for_grid(self.seat_count(), &self.empty_seats, &preset.targets);

        let seat_count = self.seat_count();
        let empty_seats = self.empty_seats.clone();
        match mode {
            TargetEditMode::Soft => {
                self.students[student_idx].targets = targets;
            }
            TargetEditMode::Forced => {
                self.students[student_idx].forced_targets = targets;
            }
        }
        Self::normalize_student_targets(&mut self.students[student_idx], seat_count, &empty_seats);
        self.clear_result_if_needed();
        self.set_info(format!(
            "{}でプリセット '{}' を適用しました。",
            mode.title(),
            preset_name
        ));
    }

    fn preset_summary(&self, preset: &TargetPreset) -> String {
        format!(
            "[{}] {}",
            preset.mode.title(),
            self.targets_to_summary(&preset.targets)
        )
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

    fn pick_input_path(target: &mut String, filter_name: &str, extensions: &[&str]) {
        if let Some(path) = FileDialog::new()
            .add_filter(filter_name, extensions)
            .pick_file()
        {
            *target = path.to_string_lossy().to_string();
        }
    }

    fn pick_output_path(
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

    fn path_row(ui: &mut egui::Ui, label: &str, path: &mut String) -> bool {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.text_edit_singleline(path);
            ui.button("参照").clicked()
        })
        .inner
    }

    fn load_students_from_json(&mut self) {
        self.clear_messages();

        let path = Self::path_from_input(&self.students_json_path, DEFAULT_STUDENTS_JSON_PATH);
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

        let mut loaded_presets = Vec::new();
        let parsed_students: BTreeMap<u16, StudentProfile> =
            if let Ok(document) = serde_json::from_str::<StudentsJsonDocument>(&text) {
                loaded_presets = document.target_presets;
                document.students
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
                converted
            };

        self.students = parsed_students
            .into_iter()
            .map(|(id, profile)| StudentForm {
                id: Some(id),
                last_name: profile.last_name,
                first_name: profile.first_name,
                last_kana: profile.last_kana,
                first_kana: profile.first_kana,
                targets: profile.targets,
                forced_targets: profile.forced_targets,
                close_to: Vec::new(),
                forced_close_to: profile.forced_close_to,
                avoid: profile.avoid,
                forced_avoid: profile.forced_avoid,
            })
            .collect();

        let seat_count = self.seat_count();
        let empty_seats = self.empty_seats.clone();
        for student in &mut self.students {
            Self::normalize_student_targets(student, seat_count, &empty_seats);
        }

        for preset in loaded_presets {
            self.upsert_target_preset(preset);
        }

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

    #[cfg(feature = "google-fetch")]
    fn import_preferences_from_forms(&mut self) {
        self.clear_messages();

        if self.students.is_empty() {
            self.set_error("生徒がいません。先に生徒を追加してください。");
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

            // フェッチした席位置の希望は確定希望に設定
            student.forced_targets =
                parse_targets(&pref.seat_targets_raw, rows, cols, &config.seat_preferences);
            // forced_seat_targets_raw は targets に設定（ソフト希望として）
            student.targets = parse_targets(
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
                "取得したフォーム回答に、現在の生徒IDと一致するデータがありませんでした。",
            );
            return;
        }

        self.clear_result_if_needed();
        self.set_info(format!("フォームの希望を {} 人分反映しました。", updated));
    }

    fn write_json_value<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<(), String> {
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

        let json = serde_json::to_string_pretty(value)
            .map_err(|err| format!("{} のJSON生成に失敗しました: {}", label, err))?;

        fs::write(path, json).map_err(|err| {
            format!(
                "{} の書き込みに失敗しました: {} ({})",
                label,
                path.display(),
                err
            )
        })
    }

    fn build_students_json_document(&self) -> StudentsJsonDocument {
        let students = self.build_students_map();
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
            target_presets,
        }
    }

    fn export_students_json(&mut self) {
        self.clear_messages();

        let students_document = self.build_students_json_document();
        if students_document.students.is_empty() {
            self.set_error("書き出す生徒がいません。生徒を追加してください。");
            return;
        }

        let path = Self::path_from_input(&self.students_json_path, DEFAULT_STUDENTS_JSON_PATH);
        match Self::write_json_value(&path, &students_document, "students.json") {
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

        for (r, row) in seats.iter_mut().enumerate().take(self.rows) {
            for (c, slot) in row.iter_mut().enumerate().take(self.cols) {
                let seat_idx = r * self.cols + c;

                if self.empty_seats[seat_idx] {
                    *slot = None;
                    continue;
                }

                if let Some(student_idx) = result.layout.get(seat_idx).and_then(|x| *x)
                    && student_idx < assigned_ids.len()
                {
                    *slot = Some(assigned_ids[student_idx]);
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

        let path = Self::path_from_input(&self.seats_json_path, DEFAULT_SEATS_JSON_PATH);
        match Self::write_json_value(&path, &document, "seats.json") {
            Ok(()) => self.set_info(format!("seats.json を出力しました: {}", path.display())),
            Err(err) => self.set_error(err),
        }
    }

    fn ensure_typst_input_json(&mut self, document: &SeatsJsonDocument) -> Result<PathBuf, String> {
        let seats_path = Self::path_from_input(&self.seats_json_path, DEFAULT_SEATS_JSON_PATH);
        Self::write_json_value(&seats_path, document, "seats.json")?;

        let typ_path = Self::path_from_input(&self.typ_path, DEFAULT_TYP_PATH);
        Self::ensure_typst_template_exists(&typ_path)?;

        // seats.typ 側が json("seats.json") を参照する前提なので、同階層にも配置する。
        let typ_dir = typ_path.parent().unwrap_or_else(|| Path::new("."));
        let typ_local_json = typ_dir.join("seats.json");
        if Self::absolute_path(&typ_local_json) != Self::absolute_path(&seats_path) {
            Self::write_json_value(&typ_local_json, document, "seats.json")?;
        }

        Ok(typ_path)
    }

    fn ensure_typst_template_exists(typ_path: &Path) -> Result<(), String> {
        if typ_path.exists() {
            return Ok(());
        }

        Self::ensure_parent_dir(typ_path)?;
        fs::write(typ_path, DEFAULT_SEATS_TYP_TEMPLATE).map_err(|err| {
            format!(
                "Typst ファイルの初期生成に失敗しました: {} ({})",
                typ_path.display(),
                err
            )
        })
    }

    fn ensure_parent_dir(output_path: &Path) -> Result<(), String> {
        if let Some(parent) = output_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "出力先ディレクトリの作成に失敗しました: {} ({})",
                    parent.display(),
                    err
                )
            })?;
        }

        Ok(())
    }

    fn compile_typst_document(typ_path: &Path) -> Result<PagedDocument, String> {
        let typ_dir = typ_path.parent().unwrap_or_else(|| Path::new("."));
        let main_name = typ_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("Typst ファイル名が不正です: {}", typ_path.display()))?;

        let engine = TypstEngine::builder()
            .fonts([include_bytes!("fonts/UDEVGothic35NFLG-Regular.ttf").as_slice()])
            .with_file_system_resolver(typ_dir.to_path_buf())
            .build();

        let warned = engine.compile::<_, PagedDocument>(main_name);
        warned
            .output
            .map_err(|err| format!("Typst コンパイルに失敗しました: {}", err))
    }

    fn export_pdf_from_document(
        document: &PagedDocument,
        output_path: &Path,
    ) -> Result<(), String> {
        Self::ensure_parent_dir(output_path)?;

        let options = typst_pdf::PdfOptions::default();
        let buffer = typst_pdf::pdf(document, &options).map_err(|err| {
            format!(
                "PDF 生成に失敗しました ({}): {:?}",
                output_path.display(),
                err
            )
        })?;

        fs::write(output_path, buffer).map_err(|err| {
            format!(
                "PDF の書き込みに失敗しました: {} ({})",
                output_path.display(),
                err
            )
        })
    }

    fn single_page_for_image<'a>(
        document: &'a PagedDocument,
        output_path: &Path,
        format: &str,
    ) -> Result<&'a typst::layout::Page, String> {
        match document.pages.as_slice() {
            [] => Err(format!(
                "{} 生成に失敗しました ({}): ページがありません。",
                format,
                output_path.display()
            )),
            [page] => Ok(page),
            pages => Err(format!(
                "{} 生成に失敗しました ({}): {}ページあります。画像出力は1ページの文書のみ対応です。",
                format,
                output_path.display(),
                pages.len()
            )),
        }
    }

    fn export_png_from_document(
        document: &PagedDocument,
        output_path: &Path,
        ppi: u16,
    ) -> Result<(), String> {
        Self::ensure_parent_dir(output_path)?;
        let page = Self::single_page_for_image(document, output_path, "PNG")?;

        let pixmap = typst_render::render(page, f32::from(ppi) / 72.0);
        let png = pixmap.encode_png().map_err(|err| {
            format!(
                "PNG エンコードに失敗しました ({}): {}",
                output_path.display(),
                err
            )
        })?;

        fs::write(output_path, png).map_err(|err| {
            format!(
                "PNG の書き込みに失敗しました: {} ({})",
                output_path.display(),
                err
            )
        })
    }

    fn export_svg_from_document(
        document: &PagedDocument,
        output_path: &Path,
    ) -> Result<(), String> {
        Self::ensure_parent_dir(output_path)?;
        let page = Self::single_page_for_image(document, output_path, "SVG")?;

        let svg = typst_svg::svg(page);
        fs::write(output_path, svg.as_bytes()).map_err(|err| {
            format!(
                "SVG の書き込みに失敗しました: {} ({})",
                output_path.display(),
                err
            )
        })
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

        let typst_document = match Self::compile_typst_document(&typ_path) {
            Ok(doc) => doc,
            Err(err) => {
                self.set_error(err);
                return;
            }
        };

        let mut success = Vec::new();
        let mut failures = Vec::new();

        if self.export_pdf {
            let path = Self::path_from_input(&self.pdf_output_path, DEFAULT_PDF_OUTPUT_PATH);
            match Self::export_pdf_from_document(&typst_document, &path) {
                Ok(()) => success.push(format!("PDF: {}", path.display())),
                Err(err) => failures.push(err),
            }
        }

        if self.export_png {
            let path = Self::path_from_input(&self.png_output_path, DEFAULT_PNG_OUTPUT_PATH);
            match Self::export_png_from_document(&typst_document, &path, self.png_ppi) {
                Ok(()) => success.push(format!("PNG: {} ({} ppi)", path.display(), self.png_ppi)),
                Err(err) => failures.push(err),
            }
        }

        if self.export_svg {
            let path = Self::path_from_input(&self.svg_output_path, DEFAULT_SVG_OUTPUT_PATH);
            match Self::export_svg_from_document(&typst_document, &path) {
                Ok(()) => success.push(format!("SVG: {}", path.display())),
                Err(err) => failures.push(err),
            }
        }

        if failures.is_empty() {
            self.set_info(format!("Typst出力が完了しました。{}", success.join(" / ")));
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

    fn render_stage_navigation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (idx, stage) in UiStage::ALL.iter().enumerate() {
                let selected = *stage == self.current_stage;
                let label = format!("{}. {}", idx + 1, stage.title());
                if ui.selectable_label(selected, label).clicked() {
                    self.current_stage = *stage;
                }
            }
        });

        ui.add_space(6.0);
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

    fn render_message_area(&self, ui: &mut egui::Ui) {
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

    fn render_setup_stage(&mut self, ui: &mut egui::Ui) {
        let mut new_rows = self.rows;
        let mut new_cols = self.cols;
        let seat_cell_size = self.seat_cell_size();

        let mut insert_row_at: Option<usize> = None;
        let mut delete_row_at: Option<usize> = None;
        let mut insert_col_at: Option<usize> = None;
        let mut delete_col_at: Option<usize> = None;
        let mut clicked_seats = Vec::new();

        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(RichText::new("レイアウト・座席形状と探索").strong());
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("行数");
                    ui.add(eframe::egui::DragValue::new(&mut new_rows).range(1..=usize::MAX));
                    ui.label("列数");
                    ui.add(eframe::egui::DragValue::new(&mut new_cols).range(1..=usize::MAX));
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("seed");
                    ui.add(eframe::egui::DragValue::new(&mut self.config.seed).range(0..=u64::MAX));
                });
                ui.label("seed = 0 でシステム乱数を使用");

                ui.horizontal(|ui| {
                    ui.label("budget回数");
                    ui.add(
                        eframe::egui::DragValue::new(&mut self.config.budget).range(0..=2_000_000),
                    );
                });
                ui.label("budget = 0 のときは実行時に利用可能席数を使用");

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("希望ランダム性");
                    ui.add(
                        eframe::egui::Slider::new(&mut self.config.randomness, 0.0..=1.0)
                            .show_value(true),
                    );
                });
                ui.label("0 = 希望優先 / 1 = ソフト希望を無視してランダム寄り（確定希望のみ残る）");
            });
        });

        if new_rows != self.rows || new_cols != self.cols {
            self.resize_grid(new_rows, new_cols);
        }

        ui.add_space(10.0);
        ui.group(|ui| {
            ui.label(RichText::new("座席形状マップ").strong());
            ui.label("+ / - で行列を挿入・削除、座席クリックで空席を切り替え");

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

        ui.add_space(10.0);
        ui.group(|ui| {
            ui.label(
                RichText::new("現在の状態")
                    .strong()
                    .color(Color32::from_rgb(60, 90, 150)),
            );
            ui.label(format!(
                "総席数: {} / 空席: {} / 利用可能席: {} / 生徒数: {}",
                self.seat_count(),
                self.empty_seats
                    .iter()
                    .filter(|is_empty| **is_empty)
                    .count(),
                self.available_seat_count(),
                self.students.len()
            ));
        });
    }

    fn render_students_stage(&mut self, ui: &mut egui::Ui) {
        self.ensure_valid_selected_student();

        let mut pick_students_json = false;
        let mut load_students_json = false;
        #[cfg(feature = "google-fetch")]
        let mut import_preferences = false;
        let mut add_student = false;
        let mut remove_selected = false;
        let mut student_changed = false;

        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(RichText::new("生徒一覧").strong());
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if ui.button("生徒を追加").clicked() {
                        add_student = true;
                    }

                    if ui
                        .add_enabled(self.selected_student.is_some(), Button::new("選択中を削除"))
                        .clicked()
                    {
                        remove_selected = true;
                    }
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("students.json");
                    ui.text_edit_singleline(&mut self.students_json_path);
                });
                ui.horizontal(|ui| {
                    if ui.button("参照").clicked() {
                        pick_students_json = true;
                    }
                    if ui.button("読み込む").clicked() {
                        load_students_json = true;
                    }
                    #[cfg(feature = "google-fetch")]
                    if ui.button("フォームから希望を反映").clicked() {
                        import_preferences = true;
                    }
                });

                ui.separator();
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
                ui.label(RichText::new("選択中の生徒を編集").strong());
                ui.add_space(4.0);

                if let Some(student_idx) = self.selected_student {
                    if student_idx < self.students.len() {
                        ui.label(format!(
                            "現在の希望席: {}",
                            self.target_summary(student_idx)
                        ));
                        ui.label(format!(
                            "現在の遠ざかり希望: {}",
                            self.avoid_summary(student_idx)
                        ));
                        ui.add_space(8.0);

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
                        if ui.button("この生徒の希望席設定へ移動").clicked() {
                            self.current_stage = UiStage::Targets;
                        }
                    }
                } else {
                    ui.label("生徒を追加して選択してください。");
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

        #[cfg(feature = "google-fetch")]
        if import_preferences {
            self.import_preferences_from_forms();
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
                    "生徒数({}) が利用可能席数({})を超えています。",
                    self.students.len(),
                    self.available_seat_count()
                ),
            );
        }
    }

    fn render_relation_editor(
        &self,
        ui: &mut egui::Ui,
        relation: RelationKind,
        student_idx: usize,
        assigned_ids: &[u16],
        id_to_name: &BTreeMap<u16, String>,
    ) -> (bool, Vec<u16>) {
        let mut clear = false;
        let mut toggled_ids = Vec::new();

        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new(relation.title()).strong());
        ui.label("クリックでON/OFFを切り替え");

        let ids = Self::relation_ids(&self.students[student_idx], relation, self.target_edit_mode);
        ui.label(format!(
            "現在の{} ({}): {}",
            relation.summary_label(),
            self.target_edit_mode.title(),
            Self::relation_summary(ids, id_to_name)
        ));

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

    fn clear_relation(&mut self, student_idx: usize, relation: RelationKind, mode: TargetEditMode) {
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

    fn apply_relation_toggles(
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

    fn render_targets_stage(&mut self, ui: &mut egui::Ui) {
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

        let mut clear_targets_for_selected = false;
        let mut clear_forced_targets_for_selected = false;
        let mut clear_close_to_for_selected = false;
        let mut clear_avoid_for_selected = false;
        let mut toggle_close_to_ids = Vec::new();
        let mut toggle_avoid_ids = Vec::new();
        let mut relation_action_mode = self.target_edit_mode;
        let mut register_preset = false;
        let mut apply_preset_idx: Option<usize> = None;
        let mut remove_preset_idx: Option<usize> = None;

        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(RichText::new("編集対象の生徒").strong());
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

                if let Some(student_idx) = self.selected_student
                    && student_idx < self.students.len()
                {
                    let display_name =
                        Self::student_display_name(&self.students[student_idx], student_idx);
                    ui.label(format!("編集中: {}", display_name));
                    ui.label(format!(
                        "現在の希望席: {}",
                        self.target_summary(student_idx)
                    ));
                    ui.label(format!(
                        "現在の確定希望: {}",
                        self.forced_target_summary(student_idx)
                    ));

                    ui.horizontal(|ui| {
                        ui.label("編集モード");
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
                    });

                    if ui.button("この生徒の希望席をクリア").clicked() {
                        clear_targets_for_selected = true;
                    }

                    if ui.button("この生徒の確定希望をクリア").clicked() {
                        clear_forced_targets_for_selected = true;
                    }

                    relation_action_mode = self.target_edit_mode;
                    let (clear_close_to, close_to_toggles) = self.render_relation_editor(
                        ui,
                        RelationKind::CloseTo,
                        student_idx,
                        &assigned_ids,
                        &id_to_name,
                    );
                    clear_close_to_for_selected = clear_close_to;
                    toggle_close_to_ids = close_to_toggles;

                    let (clear_avoid, avoid_toggles) = self.render_relation_editor(
                        ui,
                        RelationKind::Avoid,
                        student_idx,
                        &assigned_ids,
                        &id_to_name,
                    );
                    clear_avoid_for_selected = clear_avoid;
                    toggle_avoid_ids = avoid_toggles;

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
                    RichText::new(format!("{}マップ", self.target_edit_mode.title())).strong(),
                );
                ui.add_space(6.0);

                if let Some(student_idx) = self.selected_student
                    && student_idx < self.students.len()
                {
                    egui::ScrollArea::both()
                        .id_salt("target-seat-map-scroll")
                        .auto_shrink([false, false])
                        .max_height(460.0)
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
                    ui.label("対象生徒が見つかりません。生徒入力ステージを確認してください。");
                } else {
                    ui.label("まず生徒を追加して選択してください。");
                }
            });
        });

        if clear_targets_for_selected && let Some(student_idx) = self.selected_student {
            self.clear_targets(student_idx);
        }

        if clear_forced_targets_for_selected && let Some(student_idx) = self.selected_student {
            self.clear_forced_targets(student_idx);
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

    fn render_result_grid(&mut self, ui: &mut egui::Ui, result: &SeatingResult, full_screen: bool) {
        let seat_cell_size = if full_screen {
            [168.0, 78.0]
        } else {
            self.result_cell_size()
        };
        let built_students = self.build_students();

        ui.label(RichText::new(format!("sekigae3 cost: {:.3}", result.cost)).strong());

        if self.result_display_mode == ResultDisplayMode::Random {
            // 1秒ごとにランダムに生徒を追加表示
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
                let ch = (avail_h / self.rows.max(1) as f32).clamp(30.0, 160.0);
                let cell = [cw, ch.min(cw * 0.32)];

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

                                let text = match result.layout.get(idx).and_then(|x| *x) {
                                    Some(student_idx) if student_idx < built_students.len() => {
                                        let student = &built_students[student_idx];
                                        format!("{}\n({})", student.name, student.number)
                                    }
                                    _ => "-".to_string(),
                                };

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

                                let text = match result.layout.get(idx).and_then(|x| *x) {
                                    Some(student_idx) if student_idx < built_students.len() => {
                                        // 表示モードで判定
                                        let should_display = if full_screen {
                                            true
                                        } else {
                                            match self.result_display_mode {
                                                ResultDisplayMode::All => true,
                                                ResultDisplayMode::Random => self
                                                    .animation_displayed_indices
                                                    .contains(&student_idx),
                                            }
                                        };

                                        if should_display {
                                            let student = &built_students[student_idx];
                                            format!("{}\n({})", student.name, student.number)
                                        } else {
                                            "?".to_string()
                                        }
                                    }
                                    _ => "-".to_string(),
                                };

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

    fn render_export_panel(&mut self, ui: &mut egui::Ui) {
        let mut pick_students_json = false;
        let mut export_students_json = false;
        let mut export_seats_json = false;
        let mut pick_pdf_file = false;
        let mut pick_png_file = false;
        let mut pick_svg_file = false;
        let mut generate_typst = false;

        ui.label(RichText::new("JSON / Typst 出力").strong());
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("students.json");
            ui.text_edit_singleline(&mut self.students_json_path);
        });
        ui.horizontal(|ui| {
            if ui.button("students.json 参照").clicked() {
                pick_students_json = true;
            }
            if ui.button("書き出す").clicked() {
                export_students_json = true;
            }
        });

        ui.add_space(8.0);
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
        });

        if self.use_custom_date {
            ui.horizontal(|ui| {
                ui.label("値");
                ui.text_edit_singleline(&mut self.custom_date);
            });
        }

        ui.separator();
        let pick_typ_file = Self::path_row(ui, "seats.typ", &mut self.typ_path);

        ui.horizontal(|ui| {
            ui.label("出力形式");
            ui.checkbox(&mut self.export_pdf, "PDF");
            ui.checkbox(&mut self.export_png, "PNG");
            ui.checkbox(&mut self.export_svg, "SVG");
        });

        if self.export_png {
            ui.horizontal(|ui| {
                ui.label("PNG PPI");
                ui.add(eframe::egui::DragValue::new(&mut self.png_ppi).range(72..=1200));
            });
        }

        if self.export_pdf {
            pick_pdf_file = Self::path_row(ui, "PDF 出力先", &mut self.pdf_output_path);
        }

        if self.export_png {
            pick_png_file = Self::path_row(ui, "PNG 出力先", &mut self.png_output_path);
        }

        if self.export_svg {
            pick_svg_file = Self::path_row(ui, "SVG 出力先", &mut self.svg_output_path);
        }

        if ui.button("Typstで選択形式を生成").clicked() {
            generate_typst = true;
        }

        if pick_students_json {
            Self::pick_input_path(&mut self.students_json_path, "JSON", &["json"]);
        }
        if export_students_json {
            self.export_students_json();
        }
        if export_seats_json {
            self.export_seats_json();
        }
        if pick_typ_file {
            Self::pick_input_path(&mut self.typ_path, "Typst", &["typ"]);
        }
        if pick_pdf_file {
            Self::pick_output_path(&mut self.pdf_output_path, "PDF", &["pdf"], "seats.pdf");
        }
        if pick_png_file {
            Self::pick_output_path(&mut self.png_output_path, "PNG", &["png"], "seats.png");
        }
        if pick_svg_file {
            Self::pick_output_path(&mut self.svg_output_path, "SVG", &["svg"], "seats.svg");
        }
        if generate_typst {
            self.generate_typst_outputs();
        }
    }

    fn render_solve_export_stage(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
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
        self.poll_solver_result(ctx);

        let should_fullscreen =
            self.current_stage == UiStage::SolveExport && self.result_fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(should_fullscreen));

        // 結果表示中はアニメーションが進行中のため、毎フレーム再描画
        if self.result.is_some() {
            ctx.request_repaint();
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
                    let stage_index = self.current_stage_index();
                    ui.label(
                        RichText::new(format!(
                            "ステージ {}/{}: {}",
                            stage_index + 1,
                            UiStage::ALL.len(),
                            self.current_stage.title()
                        ))
                        .strong(),
                    );
                    ui.add_space(8.0);

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
