use chrono::Local;
use eframe::egui::{
    self, Button, Color32, DragValue, FontData, FontDefinitions, FontFamily, FontId, RichText,
    Slider, TextEdit, TextStyle,
};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

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
const DEFAULT_PDF_OUTPUT_PATH: &str = "./seats.pdf";
const DEFAULT_PNG_OUTPUT_PATH: &str = "./seats.png";
const DEFAULT_SVG_OUTPUT_PATH: &str = "./seats.svg";
const SEAT_CELL_SIZE: [f32; 2] = [87.6, 40.0];
const RESULT_CELL_SIZE: [f32; 2] = [124.0, 68.0];

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
            RelationKind::CloseTo => "隣になりたい学生 (sekigae3)",
            RelationKind::Avoid => "遠ざかりたい学生 (sekigae3)",
        }
    }

    fn clear_button_label(self) -> &'static str {
        match self {
            RelationKind::CloseTo => "この学生の隣希望をクリア",
            RelationKind::Avoid => "この学生の遠ざかり希望をクリア",
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

#[derive(Clone, Copy, Debug)]
struct ResultGridStyle {
    cell_size: [f32; 2],
    text_size: f32,
    reveal_all: bool,
}

#[derive(Clone, Copy, Debug)]
enum GridEdit {
    InsertRow(usize),
    DeleteRow(usize),
    InsertCol(usize),
    DeleteCol(usize),
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
            UiStage::Students => "学生情報を入力し、編集対象の学生を決めます。",
            UiStage::Targets => "選択した学生の希望席と隣希望を設定します。",
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
    tag_forms: Vec<TagDefinition>,
    use_custom_date: bool,
    custom_date: String,
    students_json_path: String,
    seats_json_path: String,
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

    student_view: bool,
    teacher_view: bool,
}

#[derive(Clone, Debug, Default)]
struct StudentForm {
    id: Option<u16>,
    last_name: String,
    first_name: String,
    last_kana: String,
    first_kana: String,
    tags: Vec<String>,
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
    tags: BTreeMap<String, TagDefinition>,
    #[serde(default)]
    target_presets: Vec<TargetPreset>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct TagDefinition {
    label: String,
    #[serde(default, skip_serializing)]
    symbol: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StudentProfile {
    last_name: String,
    first_name: String,
    last_kana: String,
    first_kana: String,
    #[serde(default)]
    tags: Vec<String>,
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
    tags: BTreeMap<String, TagDefinition>,
}

mod app;
mod export;
mod io;
mod state;
mod views;
