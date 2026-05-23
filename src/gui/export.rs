use super::*;
use eframe::egui::CollapsingHeader;
use std::path::Path;
use typst::foundations::{Bytes, Dict, IntoValue};
use typst::layout::PagedDocument;
use typst_as_lib::TypstEngine;

impl SekigaeApp {
    pub(super) fn render_export_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Export").strong());
        ui.horizontal(|ui| {
            ui.label("日付");
            ui.radio_value(&mut self.use_custom_date, false, "実行日");
            ui.radio_value(&mut self.use_custom_date, true, "カスタム");
        });

        if self.use_custom_date {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.custom_date);
            });
        }
        ui.horizontal(|ui| {
            ui.label("出力内容");
            ui.checkbox(&mut self.student_view, "学生側");
            ui.checkbox(&mut self.teacher_view, "教師側");
        });

        ui.horizontal(|ui| {
            ui.label("出力形式");
            ui.checkbox(&mut self.export_pdf, "PDF");
            ui.checkbox(&mut self.export_png, "PNG");
            ui.checkbox(&mut self.export_svg, "SVG");
        });

        if self.export_png {
            ui.horizontal(|ui| {
                ui.label("PNG PPI");
                ui.add(DragValue::new(&mut self.png_ppi).range(72..=1200));
            });
        }

        if self.export_pdf && Self::path_row(ui, "PDF 出力先", "参照", &mut self.pdf_output_path)
        {
            Self::pick_output_path(&mut self.pdf_output_path, "PDF", &["pdf"], "seats.pdf");
        }

        if self.export_png && Self::path_row(ui, "PNG 出力先", "参照", &mut self.png_output_path)
        {
            Self::pick_output_path(&mut self.png_output_path, "PNG", &["png"], "seats.png");
        }

        if self.export_svg && Self::path_row(ui, "SVG 出力先", "参照", &mut self.svg_output_path)
        {
            Self::pick_output_path(&mut self.svg_output_path, "SVG", &["svg"], "seats.svg");
        }

        if ui.button("座席表を生成").clicked() {
            self.generate_document_outputs();
        }

        ui.separator();
        CollapsingHeader::new("データ出力").show(ui, |ui| {
            if Self::path_row(ui, "座席データ", "出力", &mut self.seats_json_path) {
                self.export_seats_json();
            }
            if Self::path_row(ui, "学生データ", "出力", &mut self.students_json_path) {
                self.export_students_json();
            }
        });
    }

    pub(super) fn build_seats_json_document(&self) -> Result<SeatsJsonDocument, String> {
        let result = self
            .result
            .as_ref()
            .ok_or_else(|| "先に「席替えを実行」を押してください。".to_string())?;

        let assigned_ids = self.assign_student_ids();
        let mut seats = vec![vec![None; self.cols]; self.rows];

        for (r, row) in seats.iter_mut().enumerate() {
            for (c, slot) in row.iter_mut().enumerate() {
                let seat_idx = r * self.cols + c;
                if self.empty_seats[seat_idx] {
                    continue;
                }
                if let Some(student_idx) = result.layout.get(seat_idx).and_then(|x| *x)
                    && student_idx < assigned_ids.len()
                {
                    *slot = Some(assigned_ids[student_idx]);
                }
            }
        }

        Ok(SeatsJsonDocument {
            date: self.output_date()?,
            layout: SeatsLayout {
                rows: self.rows,
                cols: self.cols,
            },
            seats,
            students: self.build_students_map(),
            tags: self.build_tags_map(),
        })
    }

    pub(super) fn export_seats_json(&mut self) {
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

    fn compile_typst_document(
        document: &SeatsJsonDocument,
        student_view: bool,
        teacher_view: bool,
    ) -> Result<PagedDocument, String> {
        log::info!(
            "compiling embedded Typst template: student_view={}, teacher_view={}",
            student_view,
            teacher_view
        );

        let data = serde_json::to_vec(document)
            .map_err(|err| format!("seats.json のJSON生成に失敗しました: {}", err))?;

        let mut inputs = Dict::new();
        inputs.insert("data".into(), Bytes::new(data).into_value());
        inputs.insert("student_view".into(), student_view.into_value());
        inputs.insert("teacher_view".into(), teacher_view.into_value());

        TypstEngine::builder()
            .main_file(DEFAULT_SEATS_TYP_TEMPLATE)
            .fonts([include_bytes!("../fonts/UDEVGothic35NFLG-Regular.ttf").as_slice()])
            .build()
            .compile_with_input::<_, PagedDocument>(inputs)
            .output
            .map_err(|err| format!("Typst コンパイルに失敗しました: {}", err))
    }

    fn export_pdf_from_document(
        document: &PagedDocument,
        output_path: &Path,
    ) -> Result<(), String> {
        Self::ensure_parent_dir(output_path, "PDF")?;

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

    fn page_output_path(output_path: &Path, page_count: usize, page: usize, ext: &str) -> PathBuf {
        if page_count == 1 {
            return output_path.to_path_buf();
        }

        let stem = output_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("page");
        output_path.with_file_name(format!("{}-{}.{}", stem, page + 1, ext))
    }

    fn write_page_output(
        output_path: &Path,
        page_count: usize,
        page: usize,
        ext: &str,
        bytes: impl AsRef<[u8]>,
    ) -> Result<(), String> {
        fs::write(
            Self::page_output_path(output_path, page_count, page, ext),
            bytes,
        )
        .map_err(|err| format!("{} 書き込みに失敗しました: {}", ext.to_uppercase(), err))
    }

    fn export_png_from_document(
        document: &PagedDocument,
        output_path: &Path,
        ppi: u16,
    ) -> Result<(), String> {
        Self::ensure_parent_dir(output_path, "PNG")?;
        let scale = f32::from(ppi) / 72.0;

        for (i, page) in document.pages.iter().enumerate() {
            let pixmap = typst_render::render(page, scale);
            let png = pixmap
                .encode_png()
                .map_err(|err| format!("PNG エンコードに失敗しました (page {}): {}", i + 1, err))?;
            Self::write_page_output(output_path, document.pages.len(), i, "png", png)?;
        }

        Ok(())
    }

    fn export_svg_from_document(
        document: &PagedDocument,
        output_path: &Path,
    ) -> Result<(), String> {
        Self::ensure_parent_dir(output_path, "SVG")?;

        for (i, page) in document.pages.iter().enumerate() {
            Self::write_page_output(
                output_path,
                document.pages.len(),
                i,
                "svg",
                typst_svg::svg(page),
            )?;
        }

        Ok(())
    }

    fn push_export_result(
        success: &mut Vec<String>,
        failures: &mut Vec<String>,
        label: String,
        result: Result<(), String>,
    ) {
        match result {
            Ok(()) => success.push(label),
            Err(err) => failures.push(err),
        }
    }

    pub(super) fn generate_document_outputs(&mut self) {
        self.clear_messages();
        log::info!("generating document outputs");

        if !self.student_view && !self.teacher_view {
            self.set_error("出力内容に少なくとも「学生側」か「教師側」を選択してください。");
            return;
        }

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

        let typst_document =
            match Self::compile_typst_document(&document, self.student_view, self.teacher_view) {
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
            Self::push_export_result(
                &mut success,
                &mut failures,
                format!("PDF: {}", path.display()),
                Self::export_pdf_from_document(&typst_document, &path),
            );
        }

        if self.export_png {
            let path = Self::path_from_input(&self.png_output_path, DEFAULT_PNG_OUTPUT_PATH);
            Self::push_export_result(
                &mut success,
                &mut failures,
                format!("PNG: {} ({} ppi)", path.display(), self.png_ppi),
                Self::export_png_from_document(&typst_document, &path, self.png_ppi),
            );
        }

        if self.export_svg {
            let path = Self::path_from_input(&self.svg_output_path, DEFAULT_SVG_OUTPUT_PATH);
            Self::push_export_result(
                &mut success,
                &mut failures,
                format!("SVG: {}", path.display()),
                Self::export_svg_from_document(&typst_document, &path),
            );
        }

        if failures.is_empty() {
            self.set_info(format!("出力が完了しました。{}", success.join(" / ")));
        } else if success.is_empty() {
            self.set_error(failures.join("\n"));
        } else {
            self.set_error(format!(
                "一部出力は成功しました。成功: {}\n失敗: {}",
                success.join(" / "),
                failures.join("\n")
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_typst_template_compiles() {
        let students = BTreeMap::from([(
            1,
            StudentProfile {
                last_name: "山田".to_string(),
                first_name: "太郎".to_string(),
                last_kana: "ヤマダ".to_string(),
                first_kana: "タロウ".to_string(),
                ..Default::default()
            },
        )]);

        let document = SeatsJsonDocument {
            date: "2026/05/23".to_string(),
            layout: SeatsLayout { rows: 1, cols: 1 },
            seats: vec![vec![Some(1)]],
            students,
            tags: BTreeMap::new(),
        };

        let compiled = SekigaeApp::compile_typst_document(&document, true, false)
            .expect("embedded Typst template should compile");
        assert_eq!(compiled.pages.len(), 1);
    }
}
