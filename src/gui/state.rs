use super::*;

impl SekigaeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        log::debug!("creating SekigaeApp");
        super::app::install_japanese_fonts(&cc.egui_ctx);
        Self::initial_state()
    }

    pub(super) fn initial_state() -> Self {
        Self {
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            current_stage: UiStage::Setup,
            empty_seats: vec![false; DEFAULT_ROWS * DEFAULT_COLS],
            students: vec![StudentForm::default()],
            selected_student: Some(0),
            target_presets: Vec::new(),
            new_preset_name: String::new(),
            tag_forms: Vec::new(),
            use_custom_date: false,
            custom_date: Local::now().format("%Y/%m/%d").to_string(),
            students_json_path: DEFAULT_STUDENTS_JSON_PATH.to_string(),
            seats_json_path: DEFAULT_SEATS_JSON_PATH.to_string(),
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

            student_view: true,
            teacher_view: false,
        }
    }

    pub(super) fn seat_count(&self) -> usize {
        self.rows * self.cols
    }

    pub(super) fn coord_label(&self, seat_idx: usize) -> String {
        let r = seat_idx / self.cols + 1;
        let c = seat_idx % self.cols + 1;
        format!("{}-{}", r, c)
    }

    pub(super) fn available_seat_count(&self) -> usize {
        self.empty_seats
            .iter()
            .filter(|is_empty| !**is_empty)
            .count()
    }

    pub(super) fn clear_result_if_needed(&mut self) {
        self.result = None;
    }

    pub(super) fn clear_messages(&mut self) {
        self.last_error = None;
        self.last_info = None;
    }

    pub(super) fn set_error(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        log::warn!("{}", msg);
        self.last_error = Some(msg);
        self.last_info = None;
    }

    pub(super) fn set_info(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        log::info!("{}", msg);
        self.last_info = Some(msg);
        self.last_error = None;
    }

    pub(super) fn set_window_busy_state(&self, ctx: &egui::Context, busy: bool) {
        let title = if busy {
            format!("{} (席替え中...)", APP_TITLE)
        } else {
            APP_TITLE.to_string()
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    pub(super) fn poll_solver_result(&mut self, ctx: &egui::Context) {
        if !self.is_solving {
            return;
        }

        let Some(rx_result) = self.solver_rx.as_ref().map(|rx| rx.try_recv()) else {
            self.is_solving = false;
            self.set_window_busy_state(ctx, false);
            return;
        };

        match rx_result {
            Ok(Ok(result)) => {
                log::info!("solver result received: cost={}", result.cost);
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
                log::error!("solver failed: {}", err);
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
                log::error!("solver thread disconnected");
                self.result = None;
                self.set_error("席替え処理のスレッドが切断されました。".to_string());
                self.is_solving = false;
                self.solver_rx = None;
                self.set_window_busy_state(ctx, false);
            }
        }
    }

    pub(super) fn reset_all(&mut self, ctx: &egui::Context) {
        *self = Self {
            last_info: Some("すべてリセットしました。".to_string()),
            ..Self::initial_state()
        };
        self.set_window_busy_state(ctx, false);
    }

    pub(super) fn bubble_symbol_button(ui: &mut egui::Ui, symbol: &str, enabled: bool) -> bool {
        let button = Button::new(
            RichText::new(symbol)
                .strong()
                .color(Color32::from_rgb(35, 35, 35)),
        )
        .min_size(egui::vec2(18.0, 18.0))
        .fill(Color32::from_rgb(240, 240, 245));

        ui.add_enabled(enabled, button).clicked()
    }

    pub(super) fn bubble_pair_cell(
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

    pub(super) fn default_tag_form() -> TagForm {
        TagForm {
            symbol: String::new(),
            label: String::new(),
        }
    }

    pub(super) fn normalize_tag_symbol(symbol: &str) -> String {
        symbol
            .trim()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .take(4)
            .collect()
    }

    pub(super) fn tag_symbol_from_definition(key: &str, definition: &TagDefinition) -> String {
        let symbol = Self::normalize_tag_symbol(&definition.symbol);
        if symbol.is_empty() {
            Self::normalize_tag_symbol(key)
        } else {
            symbol
        }
    }

    pub(super) fn build_tags_map(&self) -> BTreeMap<String, TagDefinition> {
        let mut tags = BTreeMap::new();

        for form in &self.tag_forms {
            let symbol = Self::normalize_tag_symbol(&form.symbol);
            if symbol.is_empty() || tags.contains_key(&symbol) {
                continue;
            }

            tags.insert(
                symbol.clone(),
                TagDefinition {
                    label: form.label.trim().to_string(),
                    symbol,
                },
            );
        }

        tags
    }

    pub(super) fn build_tag_symbol_set(&self) -> HashSet<String> {
        self.build_tags_map().into_keys().collect()
    }

    pub(super) fn sanitize_student_tags(
        tags: &[String],
        valid_tags: &HashSet<String>,
    ) -> Vec<String> {
        let mut sanitized = Vec::new();
        let mut seen = HashSet::new();

        for tag in tags {
            let tag = tag.trim();
            if tag.is_empty() || !valid_tags.contains(tag) || !seen.insert(tag.to_string()) {
                continue;
            }
            sanitized.push(tag.to_string());
        }

        sanitized
    }

    pub(super) fn student_tag_symbols_with_defs(
        tags: &[String],
        tag_defs: &BTreeMap<String, TagDefinition>,
    ) -> String {
        tags.iter()
            .filter_map(|symbol| tag_defs.contains_key(symbol).then_some(symbol.trim()))
            .filter(|symbol| !symbol.is_empty())
            .map(|symbol| symbol.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(super) fn next_unused_tag_symbol(used: &HashSet<String>) -> String {
        for index in 1..=usize::MAX {
            let candidate = format!("T{}", index);
            if !used.contains(&candidate) {
                return candidate;
            }
        }

        "T".to_string()
    }

    pub(super) fn remove_tag_from_students(&mut self, tag_symbol: &str) {
        for student in &mut self.students {
            student.tags.retain(|tag| tag != tag_symbol);
        }
    }

    pub(super) fn rename_tag_in_students(&mut self, old_symbol: &str, new_symbol: &str) {
        if old_symbol.is_empty() || old_symbol == new_symbol {
            return;
        }

        if new_symbol.is_empty() {
            self.remove_tag_from_students(old_symbol);
            return;
        }

        for student in &mut self.students {
            let mut renamed = false;
            for tag in &mut student.tags {
                if tag == old_symbol {
                    *tag = new_symbol.to_string();
                    renamed = true;
                }
            }

            if renamed {
                let mut seen = HashSet::new();
                student.tags.retain(|tag| seen.insert(tag.clone()));
            }
        }
    }

    pub(super) fn assign_tag_to_student(
        student: &mut StudentForm,
        tag_symbol: &str,
        selected: bool,
    ) {
        if selected {
            if !student.tags.iter().any(|existing| existing == tag_symbol) {
                student.tags.push(tag_symbol.to_string());
            }
        } else {
            student.tags.retain(|existing| existing != tag_symbol);
        }
    }

    pub(super) fn ensure_valid_selected_student(&mut self) {
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

    pub(super) fn current_stage_index(&self) -> usize {
        UiStage::ALL
            .iter()
            .position(|stage| *stage == self.current_stage)
            .unwrap_or(0)
    }

    pub(super) fn go_prev_stage(&mut self) {
        let idx = self.current_stage_index();
        if idx > 0 {
            self.current_stage = UiStage::ALL[idx - 1];
        }
    }

    pub(super) fn go_next_stage(&mut self) {
        let idx = self.current_stage_index();
        if idx + 1 < UiStage::ALL.len() {
            self.current_stage = UiStage::ALL[idx + 1];
        }
    }

    pub(super) fn seat_cell_size(&self) -> [f32; 2] {
        [87.6, 40.0]
    }

    pub(super) fn result_cell_size(&self) -> [f32; 2] {
        [124.0, 68.0]
    }

    pub(super) fn render_result_display_mode_selector(&mut self, ui: &mut egui::Ui) {
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
            ui.label("(1秒ごとにランダムに学生を表示)");
        } else {
            ui.label("(一括で表示)");
        }
    }

    pub(super) fn apply_text_style(&self, ctx: &egui::Context) {
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

    pub(super) fn sanitize_targets_for_grid(
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

    pub(super) fn sanitize_relation_ids(
        ids: &[u16],
        self_id: u16,
        valid_ids: &HashSet<u16>,
    ) -> Vec<u16> {
        let mut out = ids
            .iter()
            .copied()
            .filter(|id| *id != self_id && valid_ids.contains(id))
            .collect::<Vec<_>>();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub(super) fn relation_ids(
        student: &StudentForm,
        relation: RelationKind,
        mode: TargetEditMode,
    ) -> &[u16] {
        match (relation, mode) {
            (RelationKind::CloseTo, TargetEditMode::Soft) => &student.close_to,
            (RelationKind::CloseTo, TargetEditMode::Forced) => &student.forced_close_to,
            (RelationKind::Avoid, TargetEditMode::Soft) => &student.avoid,
            (RelationKind::Avoid, TargetEditMode::Forced) => &student.forced_avoid,
        }
    }

    pub(super) fn relation_ids_mut(
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

    pub(super) fn normalize_relation_list(
        ids: &mut Vec<u16>,
        self_id: u16,
        valid_ids: &HashSet<u16>,
    ) -> bool {
        let sanitized = Self::sanitize_relation_ids(ids, self_id, valid_ids);
        if *ids == sanitized {
            false
        } else {
            *ids = sanitized;
            true
        }
    }

    pub(super) fn normalize_student_relations(
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

    pub(super) fn toggle_relation_ids(
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

    pub(super) fn sanitize_target_list(&self, targets: &[usize]) -> Vec<usize> {
        Self::sanitize_targets_for_grid(self.seat_count(), &self.empty_seats, targets)
    }

    pub(super) fn targets_to_model_targets(&self, indices: &[usize]) -> Vec<Target> {
        let mut targets = self
            .sanitize_target_list(indices)
            .into_iter()
            .map(|seat_idx| Target::new(seat_idx % self.cols, seat_idx / self.cols))
            .collect::<Vec<_>>();
        targets.sort_by_key(|t| (t.r, t.c));
        targets.dedup();
        targets
    }

    pub(super) fn toggle_target_list(targets: &mut Vec<usize>, seat_idx: usize) {
        if let Some(pos) = targets.iter().position(|idx| *idx == seat_idx) {
            targets.remove(pos);
        } else {
            targets.push(seat_idx);
            targets.sort_unstable();
            targets.dedup();
        }
    }

    pub(super) fn normalize_student_targets(
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

    pub(super) fn apply_grid_transform(
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

    pub(super) fn resize_grid(&mut self, new_rows: usize, new_cols: usize) {
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

    pub(super) fn insert_row_at(&mut self, insert_before: usize) {
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

    pub(super) fn delete_row_at(&mut self, row_idx: usize) {
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

    pub(super) fn insert_col_at(&mut self, insert_before: usize) {
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

    pub(super) fn delete_col_at(&mut self, col_idx: usize) {
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

    pub(super) fn set_empty_seat_state(&mut self, seat_idx: usize, is_empty: bool) -> bool {
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

    pub(super) fn toggle_target(&mut self, student_idx: usize, seat_idx: usize, forced: bool) {
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

    pub(super) fn clear_targets(&mut self, student_idx: usize) {
        if student_idx >= self.students.len() {
            return;
        }
        self.students[student_idx].targets.clear();
        self.clear_result_if_needed();
        self.clear_messages();
    }

    pub(super) fn clear_forced_targets(&mut self, student_idx: usize) {
        if student_idx >= self.students.len() {
            return;
        }
        self.students[student_idx].forced_targets.clear();
        self.clear_result_if_needed();
        self.clear_messages();
    }

    pub(super) fn next_unused_id(used: &HashSet<u16>, mut start: u16) -> u16 {
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

    pub(super) fn assign_student_ids(&self) -> Vec<u16> {
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

    pub(super) fn student_display_name(student: &StudentForm, idx: usize) -> String {
        let name = format!("{}{}", student.last_name.trim(), student.first_name.trim());
        if name.is_empty() {
            format!("学生{}", idx + 1)
        } else {
            name
        }
    }

    pub(super) fn profile_from_form(
        form: &StudentForm,
        idx: usize,
        valid_tags: &HashSet<String>,
    ) -> StudentProfile {
        let mut last_name = form.last_name.trim().to_string();
        let first_name = form.first_name.trim().to_string();

        if last_name.is_empty() && first_name.is_empty() {
            last_name = format!("学生{}", idx + 1);
        }

        StudentProfile {
            last_name,
            first_name,
            last_kana: form.last_kana.trim().to_string(),
            first_kana: form.first_kana.trim().to_string(),
            tags: Self::sanitize_student_tags(&form.tags, valid_tags),
            targets: form.targets.clone(),
            forced_targets: form.forced_targets.clone(),
            close_to: form.close_to.clone(),
            forced_close_to: form.forced_close_to.clone(),
            avoid: form.avoid.clone(),
            forced_avoid: form.forced_avoid.clone(),
        }
    }

    pub(super) fn sanitize_preset_targets(&self, targets: &[usize]) -> Vec<usize> {
        Self::sanitize_targets_for_grid(self.seat_count(), &self.empty_seats, targets)
    }

    pub(super) fn upsert_target_preset(&mut self, preset: TargetPreset) {
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

    pub(super) fn build_students(&self) -> Vec<Student> {
        let assigned_ids = self.assign_student_ids();
        let valid_ids = assigned_ids.iter().copied().collect::<HashSet<_>>();
        let valid_tags = self.build_tag_symbol_set();

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
                let avoid = Self::sanitize_relation_ids(&entry.avoid, number, &valid_ids);
                let forced_avoid =
                    Self::sanitize_relation_ids(&entry.forced_avoid, number, &valid_ids);
                let tags = Self::sanitize_student_tags(&entry.tags, &valid_tags);
                Student {
                    name,
                    number,
                    targets,
                    forced_targets,
                    tags,
                    close_to,
                    forced_close_to,
                    avoid,
                    forced_avoid,
                }
            })
            .collect()
    }

    pub(super) fn build_students_map(&self) -> BTreeMap<u16, StudentProfile> {
        let assigned_ids = self.assign_student_ids();
        let valid_tags = self.build_tag_symbol_set();
        let mut students = BTreeMap::new();

        for (idx, form) in self.students.iter().enumerate() {
            if idx >= assigned_ids.len() {
                continue;
            }
            let id = assigned_ids[idx];
            students.insert(id, Self::profile_from_form(form, idx, &valid_tags));
        }

        students
    }

    pub(super) fn output_date(&self) -> Result<String, String> {
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

    pub(super) fn empty_seat_indices(&self) -> Vec<usize> {
        self.empty_seats
            .iter()
            .enumerate()
            .filter_map(|(idx, is_empty)| if *is_empty { Some(idx) } else { None })
            .collect()
    }

    pub(super) fn run_solver(&mut self, ctx: &egui::Context) {
        if self.is_solving {
            return;
        }

        self.clear_messages();

        let students = self.build_students();
        if students.is_empty() {
            self.result = None;
            self.set_error("学生を1人以上追加してください。");
            return;
        }

        let available = self.available_seat_count();
        if students.len() > available {
            self.result = None;
            self.set_error(format!(
                "学生数({})が利用可能席数({})を超えています。",
                students.len(),
                available
            ));
            return;
        }

        let rows = self.rows;
        let cols = self.cols;
        let empty = self.empty_seat_indices();
        let config = self.config;
        log::info!(
            "solver started: students={}, rows={}, cols={}, empty_seats={}, budget={}, randomness={:.3}, seed={}",
            students.len(),
            rows,
            cols,
            empty.len(),
            config.budget,
            config.randomness,
            config.seed
        );

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

    pub(super) fn targets_to_summary(&self, targets: &[usize]) -> String {
        if targets.is_empty() {
            return "希望席なし(どこでも可)".to_string();
        }

        targets
            .iter()
            .map(|idx| self.coord_label(*idx))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(super) fn register_current_as_preset(&mut self, student_idx: usize) {
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

    pub(super) fn apply_preset_to_student(&mut self, student_idx: usize, preset_idx: usize) {
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

    pub(super) fn preset_summary(&self, preset: &TargetPreset) -> String {
        format!(
            "[{}] {}",
            preset.mode.title(),
            self.targets_to_summary(&preset.targets)
        )
    }
}
