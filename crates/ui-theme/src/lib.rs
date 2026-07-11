use egui::Color32;

/// 命名字体大小
#[derive(Clone, Debug)]
pub struct FontSizes {
    /// 10.0 — resize handles、group-by 指示器
    pub tiny: f32,
    /// 11.0 — kind 标签、历史表
    pub xs: f32,
    /// 11.5 — 摘要文字
    pub sm: f32,
    /// 12.0 — 表格单元格、侧边栏项、对话框基础
    pub base: f32,
    /// 12.5 — 标签页按钮、combo 项、对话框正文
    pub md: f32,
    /// 13.0 — 表头、autocomplete 标签、输入标签
    pub lg: f32,
    /// 14.0 — 输入框、右键菜单项
    pub xl: f32,
    /// 15.0 — SQL 代码编辑器
    pub code: f32,
    /// 36.0 — 空状态标题
    pub hero: f32,
}

impl Default for FontSizes {
    fn default() -> Self {
        Self {
            tiny: 10.0,
            xs: 11.0,
            sm: 11.5,
            base: 12.0,
            md: 12.5,
            lg: 13.0,
            xl: 14.0,
            code: 15.0,
            hero: 36.0,
        }
    }
}

/// 统一主题颜色
#[derive(Clone, Debug)]
pub struct ThemeColors {
    // ── 通用 UI ──
    pub toolbar_bg: Color32,
    pub sidebar_bg: Color32,
    pub workspace_bg: Color32,
    pub card_bg: Color32,
    pub table_header_bg: Color32,
    pub table_alt_bg: Color32,
    pub search_bg: Color32,
    pub border: Color32,
    pub soft_border: Color32,
    pub table_grid: Color32,
    pub selection_bg: Color32,
    pub selection_stroke: Color32,
    pub selection_text: Color32,
    pub expand_arrow: Color32,
    pub text: Color32,
    pub weak_text: Color32,
    pub muted_dot: Color32,
    pub success: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub tab_idle_bg: Color32,

    // ── 按钮 ──
    pub primary_button_bg: Color32,
    pub primary_button_stroke: Color32,
    pub primary_button_text: Color32,
    pub secondary_button_bg: Color32,
    pub secondary_button_stroke: Color32,
    pub secondary_button_text: Color32,
    pub accent_button_bg: Color32,
    pub accent_button_stroke: Color32,
    pub accent_button_text: Color32,
    pub accent_active_button_bg: Color32,
    pub accent_active_button_stroke: Color32,
    pub accent_active_button_text: Color32,
    pub modified_button_bg: Color32,
    pub modified_button_stroke: Color32,
    pub modified_button_text: Color32,
    pub subtle_button_bg: Color32,
    pub subtle_button_stroke: Color32,
    pub subtle_button_text: Color32,
    pub danger_button_bg: Color32,
    pub danger_button_stroke: Color32,
    pub danger_button_text: Color32,
    pub hide_button_bg: Color32,
    pub hide_button_stroke: Color32,
    pub hide_button_text: Color32,

    // ── 特殊标记 ──
    pub index_badge: Color32,
    pub new_row_bg: Color32,

    // ── 对话框 ──
    pub dialog_window_bg: Color32,
    pub dialog_border: Color32,
    pub dialog_section_bg: Color32,
    pub dialog_section_border: Color32,
    pub dialog_input_bg: Color32,
    pub dialog_input_hover_bg: Color32,
    pub dialog_input_active_bg: Color32,
    pub dialog_input_border: Color32,
    pub dialog_title: Color32,
    pub dialog_subtitle: Color32,
    pub dialog_text: Color32,
    pub dialog_weak_text: Color32,
    pub dialog_primary_button_bg: Color32,
    pub dialog_primary_button_stroke: Color32,
    pub dialog_primary_button_text: Color32,
    pub dialog_secondary_button_bg: Color32,
    pub dialog_secondary_button_stroke: Color32,
    pub dialog_secondary_button_text: Color32,

    // ── 编辑器 ──
    pub editor_panel_bg: Color32,
    pub editor_bg: Color32,
    pub editor_gutter_bg: Color32,
    pub editor_current_line_bg: Color32,
    pub editor_text: Color32,
    pub editor_line_number: Color32,
    pub editor_line_number_active: Color32,
    pub editor_keyword: Color32,
    pub editor_string: Color32,
    pub editor_number: Color32,
    pub editor_comment: Color32,

    // ── 自动补全 ──
    pub autocomplete_popup_bg: Color32,
    pub autocomplete_border: Color32,
    pub autocomplete_text: Color32,
    pub autocomplete_weak_text: Color32,
    pub autocomplete_selected_bg: Color32,
    pub autocomplete_selected_text: Color32,
    pub autocomplete_match_blue: Color32,
    pub autocomplete_match_yellow: Color32,

    // ── 内联硬编码颜色（收编）──
    pub copy_button_bg: Color32,
    pub copy_button_stroke: Color32,
    pub delete_button_bg: Color32,
    pub delete_button_text: Color32,
    pub confirm_button_bg: Color32,
    pub warning_icon: Color32,
    pub context_menu_hover: Color32,
    pub context_menu_active: Color32,
    pub search_highlight_bg: Color32,
    pub search_highlight_fg: Color32,
    pub explain_scan: Color32,
    pub explain_sort: Color32,
    pub explain_join: Color32,
    pub explain_filter: Color32,
    pub update_success_bg: Color32,
    pub update_error_bg: Color32,
    pub update_success_text: Color32,
    pub update_error_text: Color32,

    // ── egui 全局视觉 ──
    pub panel_fill: Color32,
    pub window_fill: Color32,
    pub extreme_bg: Color32,
    pub faint_bg: Color32,
    pub code_bg: Color32,
    pub window_stroke: Color32,
    // widget 状态色
    pub widget_noninteractive_bg: Color32,
    pub widget_noninteractive_stroke: Color32,
    pub widget_noninteractive_fg: Color32,
    pub widget_inactive_bg: Color32,
    pub widget_inactive_stroke: Color32,
    pub widget_hovered_bg: Color32,
    pub widget_hovered_stroke: Color32,
    pub widget_active_bg: Color32,
    pub widget_active_stroke: Color32,
    pub widget_open_bg: Color32,
    pub widget_open_stroke: Color32,
    // selection（带透明度）
    pub egui_selection_bg: Color32,
    pub egui_selection_stroke: Color32,

    // ── 滚动条 ──
    pub scrollbar_dormant_opacity: f32,
    pub scrollbar_active_opacity: f32,
    pub scrollbar_interact_opacity: f32,
}

/// 统一主题
#[derive(Clone, Debug)]
pub struct Theme {
    pub colors: ThemeColors,
    pub fonts: FontSizes,
    pub dark_mode: bool,
}

impl Theme {
    pub fn new(dark_mode: bool) -> Self {
        if dark_mode { Self::dark() } else { Self::light() }
    }

    pub fn from_visuals(visuals: &egui::Visuals) -> Self {
        Self::new(visuals.dark_mode)
    }

    pub fn dark() -> Self {
        Self {
            dark_mode: true,
            fonts: FontSizes::default(),
            colors: ThemeColors {
                // ── 通用 UI ──
                toolbar_bg: Color32::from_rgb(54, 54, 56),
                sidebar_bg: Color32::from_rgb(44, 44, 46),
                workspace_bg: Color32::from_rgb(54, 54, 56),
                card_bg: Color32::from_rgb(54, 54, 56),
                table_header_bg: Color32::from_rgb(62, 62, 64),
                table_alt_bg: Color32::from_rgb(50, 50, 52),
                search_bg: Color32::from_rgb(54, 54, 56),
                border: Color32::from_rgb(86, 86, 89),
                soft_border: Color32::from_rgb(70, 70, 73),
                table_grid: Color32::from_rgb(74, 74, 77),
                selection_bg: Color32::from_rgb(60, 110, 175),
                selection_stroke: Color32::from_rgb(130, 165, 220),
                selection_text: Color32::from_rgb(255, 255, 255),
                expand_arrow: Color32::from_rgb(60, 110, 175),
                text: Color32::from_rgb(245, 245, 245),
                weak_text: Color32::from_rgb(180, 180, 180),
                muted_dot: Color32::from_rgb(130, 130, 132),
                success: Color32::from_rgb(68, 188, 125),
                danger: Color32::from_rgb(255, 115, 115),
                warning: Color32::from_rgb(255, 190, 70),
                tab_idle_bg: Color32::from_rgb(52, 52, 54),

                // ── 按钮 ──
                primary_button_bg: Color32::from_rgb(10, 132, 255),
                primary_button_stroke: Color32::from_rgb(65, 155, 255),
                primary_button_text: Color32::WHITE,
                secondary_button_bg: Color32::from_rgb(99, 99, 102),
                secondary_button_stroke: Color32::from_rgb(120, 120, 123),
                secondary_button_text: Color32::from_rgb(245, 245, 245),
                accent_button_bg: Color32::from_rgb(44, 44, 46),
                accent_button_stroke: Color32::from_rgb(100, 210, 255),
                accent_button_text: Color32::from_rgb(100, 210, 255),
                accent_active_button_bg: Color32::from_rgb(44, 44, 46),
                accent_active_button_stroke: Color32::from_rgb(210, 165, 50),
                accent_active_button_text: Color32::from_rgb(210, 165, 50),
                modified_button_bg: Color32::from_rgb(165, 140, 46),
                modified_button_stroke: Color32::from_rgb(195, 170, 75),
                modified_button_text: Color32::WHITE,
                subtle_button_bg: Color32::from_rgb(52, 52, 54),
                subtle_button_stroke: Color32::from_rgb(72, 72, 75),
                subtle_button_text: Color32::from_rgb(200, 200, 200),
                danger_button_bg: Color32::from_rgb(44, 44, 46),
                danger_button_stroke: Color32::from_rgb(255, 69, 58),
                danger_button_text: Color32::from_rgb(255, 69, 58),
                hide_button_bg: Color32::from_rgb(43, 92, 92),
                hide_button_stroke: Color32::from_rgb(68, 128, 128),
                hide_button_text: Color32::from_rgb(210, 240, 240),

                // ── 特殊标记 ──
                index_badge: Color32::from_rgb(68, 188, 125),
                new_row_bg: Color32::from_rgba_premultiplied(40, 80, 40, 60),

                // ── 对话框 ──
                dialog_window_bg: Color32::from_rgb(50, 50, 52),
                dialog_border: Color32::from_rgb(84, 84, 86),
                dialog_section_bg: Color32::from_rgb(58, 58, 60),
                dialog_section_border: Color32::from_rgb(92, 92, 95),
                dialog_input_bg: Color32::from_rgb(72, 72, 75),
                dialog_input_hover_bg: Color32::from_rgb(80, 80, 83),
                dialog_input_active_bg: Color32::from_rgb(86, 86, 89),
                dialog_input_border: Color32::from_rgb(100, 100, 103),
                dialog_title: Color32::from_rgb(255, 255, 255),
                dialog_subtitle: Color32::from_rgb(190, 190, 190),
                dialog_text: Color32::from_rgb(245, 245, 245),
                dialog_weak_text: Color32::from_rgb(180, 180, 180),
                dialog_primary_button_bg: Color32::from_rgb(10, 132, 255),
                dialog_primary_button_stroke: Color32::from_rgb(60, 150, 255),
                dialog_primary_button_text: Color32::WHITE,
                dialog_secondary_button_bg: Color32::from_rgb(99, 99, 102),
                dialog_secondary_button_stroke: Color32::from_rgb(120, 120, 123),
                dialog_secondary_button_text: Color32::from_rgb(250, 250, 250),

                // ── 编辑器 ──
                editor_panel_bg: Color32::from_rgb(54, 54, 56),
                editor_bg: Color32::from_rgb(42, 42, 44),
                editor_gutter_bg: Color32::from_rgb(40, 40, 42),
                editor_current_line_bg: Color32::from_rgb(36, 50, 72),
                editor_text: Color32::from_rgb(220, 220, 220),
                editor_line_number: Color32::from_rgb(110, 110, 112),
                editor_line_number_active: Color32::from_rgb(225, 225, 225),
                editor_keyword: Color32::from_rgb(85, 155, 212),
                editor_string: Color32::from_rgb(205, 143, 118),
                editor_number: Color32::from_rgb(180, 205, 166),
                editor_comment: Color32::from_rgb(105, 152, 84),

                // ── 自动补全 ──
                autocomplete_popup_bg: Color32::from_rgb(40, 40, 42),
                autocomplete_border: Color32::from_rgb(80, 80, 83),
                autocomplete_text: Color32::from_rgb(220, 220, 220),
                autocomplete_weak_text: Color32::from_rgb(110, 110, 112),
                autocomplete_selected_bg: Color32::from_rgb(8, 68, 140),
                autocomplete_selected_text: Color32::from_rgb(255, 255, 255),
                autocomplete_match_blue: Color32::from_rgb(86, 156, 214),
                autocomplete_match_yellow: Color32::from_rgb(255, 255, 120),

                // ── 内联硬编码颜色 ──
                copy_button_bg: Color32::from_rgb(56, 108, 176),
                copy_button_stroke: Color32::from_rgb(82, 138, 210),
                delete_button_bg: Color32::from_rgb(255, 69, 58),
                delete_button_text: Color32::WHITE,
                confirm_button_bg: Color32::from_rgb(0, 122, 255),
                warning_icon: Color32::from_rgb(255, 140, 0),
                context_menu_hover: Color32::from_rgb(50, 100, 170),
                context_menu_active: Color32::from_rgb(40, 80, 140),
                search_highlight_bg: Color32::from_rgb(255, 230, 0),
                search_highlight_fg: Color32::from_rgb(80, 60, 0),
                explain_scan: Color32::from_rgb(100, 149, 237),
                explain_sort: Color32::from_rgb(255, 165, 0),
                explain_join: Color32::from_rgb(72, 199, 142),
                explain_filter: Color32::from_rgb(255, 215, 0),
                update_success_bg: Color32::from_rgb(32, 60, 32),
                update_error_bg: Color32::from_rgb(60, 32, 32),
                update_success_text: Color32::from_rgb(80, 195, 120),
                update_error_text: Color32::from_rgb(220, 80, 80),

                // ── egui 全局视觉 ──
                panel_fill: Color32::from_rgb(54, 54, 56),
                window_fill: Color32::from_rgb(54, 54, 56),
                extreme_bg: Color32::from_rgb(72, 72, 75),
                faint_bg: Color32::from_rgb(58, 58, 60),
                code_bg: Color32::from_rgb(68, 68, 70),
                window_stroke: Color32::from_rgb(86, 86, 89),
                widget_noninteractive_bg: Color32::from_rgb(58, 58, 60),
                widget_noninteractive_stroke: Color32::from_rgb(86, 86, 89),
                widget_noninteractive_fg: Color32::from_rgb(190, 190, 190),
                widget_inactive_bg: Color32::from_rgb(72, 72, 75),
                widget_inactive_stroke: Color32::from_rgb(100, 100, 103),
                widget_hovered_bg: Color32::from_rgb(78, 78, 81),
                widget_hovered_stroke: Color32::from_rgb(100, 130, 170),
                widget_active_bg: Color32::from_rgb(74, 74, 77),
                widget_active_stroke: Color32::from_rgb(105, 135, 175),
                widget_open_bg: Color32::from_rgb(78, 78, 81),
                widget_open_stroke: Color32::from_rgb(100, 100, 103),
                egui_selection_bg: Color32::from_rgba_premultiplied(80, 138, 205, 100),
                egui_selection_stroke: Color32::from_rgba_premultiplied(140, 175, 230, 130),

                // ── 滚动条 ──
                scrollbar_dormant_opacity: 0.35,
                scrollbar_active_opacity: 0.55,
                scrollbar_interact_opacity: 0.75,
            },
        }
    }

    pub fn light() -> Self {
        Self {
            dark_mode: false,
            fonts: FontSizes::default(),
            colors: ThemeColors {
                // ── 通用 UI ──
                toolbar_bg: Color32::from_rgb(255, 255, 255),
                sidebar_bg: Color32::from_rgb(237, 237, 239),
                workspace_bg: Color32::from_rgb(255, 255, 255),
                card_bg: Color32::from_rgb(255, 255, 255),
                table_header_bg: Color32::from_rgb(243, 243, 243),
                table_alt_bg: Color32::from_rgb(248, 248, 248),
                search_bg: Color32::from_rgb(255, 255, 255),
                border: Color32::from_rgb(218, 218, 218),
                soft_border: Color32::from_rgb(232, 232, 232),
                table_grid: Color32::from_rgb(224, 224, 224),
                selection_bg: Color32::from_rgb(200, 220, 250),
                selection_stroke: Color32::from_rgb(125, 165, 225),
                selection_text: Color32::from_rgb(20, 60, 120),
                expand_arrow: Color32::from_rgb(68, 128, 200),
                text: Color32::from_rgb(40, 40, 40),
                weak_text: Color32::from_rgb(105, 105, 105),
                muted_dot: Color32::from_rgb(152, 152, 152),
                success: Color32::from_rgb(48, 167, 104),
                danger: Color32::from_rgb(220, 86, 86),
                warning: Color32::from_rgb(255, 179, 25),
                tab_idle_bg: Color32::from_rgb(245, 245, 245),

                // ── 按钮 ──
                primary_button_bg: Color32::from_rgb(0, 122, 255),
                primary_button_stroke: Color32::from_rgb(0, 114, 238),
                primary_button_text: Color32::WHITE,
                secondary_button_bg: Color32::from_rgb(180, 180, 183),
                secondary_button_stroke: Color32::from_rgb(155, 155, 158),
                secondary_button_text: Color32::WHITE,
                accent_button_bg: Color32::from_rgb(246, 246, 246),
                accent_button_stroke: Color32::from_rgb(90, 200, 250),
                accent_button_text: Color32::from_rgb(40, 140, 195),
                accent_active_button_bg: Color32::from_rgb(246, 246, 246),
                accent_active_button_stroke: Color32::from_rgb(190, 140, 20),
                accent_active_button_text: Color32::from_rgb(140, 105, 15),
                modified_button_bg: Color32::from_rgb(255, 243, 176),
                modified_button_stroke: Color32::from_rgb(228, 212, 118),
                modified_button_text: Color32::from_rgb(120, 100, 20),
                subtle_button_bg: Color32::from_rgb(248, 248, 248),
                subtle_button_stroke: Color32::from_rgb(228, 228, 228),
                subtle_button_text: Color32::from_rgb(80, 80, 80),
                danger_button_bg: Color32::from_rgb(255, 255, 255),
                danger_button_stroke: Color32::from_rgb(255, 59, 48),
                danger_button_text: Color32::from_rgb(255, 59, 48),
                hide_button_bg: Color32::from_rgb(230, 245, 245),
                hide_button_stroke: Color32::from_rgb(175, 210, 210),
                hide_button_text: Color32::from_rgb(50, 110, 110),

                // ── 特殊标记 ──
                index_badge: Color32::from_rgb(48, 167, 104),
                new_row_bg: Color32::from_rgba_premultiplied(40, 120, 40, 40),

                // ── 对话框 ──
                dialog_window_bg: Color32::from_rgb(246, 246, 246),
                dialog_border: Color32::from_rgb(218, 218, 218),
                dialog_section_bg: Color32::from_rgb(252, 252, 252),
                dialog_section_border: Color32::from_rgb(226, 226, 226),
                dialog_input_bg: Color32::from_rgb(255, 255, 255),
                dialog_input_hover_bg: Color32::from_rgb(252, 253, 255),
                dialog_input_active_bg: Color32::from_rgb(255, 255, 255),
                dialog_input_border: Color32::from_rgb(210, 210, 210),
                dialog_title: Color32::from_rgb(30, 30, 30),
                dialog_subtitle: Color32::from_rgb(100, 100, 100),
                dialog_text: Color32::from_rgb(40, 40, 40),
                dialog_weak_text: Color32::from_rgb(109, 118, 130),
                dialog_primary_button_bg: Color32::from_rgb(0, 122, 255),
                dialog_primary_button_stroke: Color32::from_rgb(0, 114, 240),
                dialog_primary_button_text: Color32::WHITE,
                dialog_secondary_button_bg: Color32::from_rgb(180, 180, 183),
                dialog_secondary_button_stroke: Color32::from_rgb(155, 155, 158),
                dialog_secondary_button_text: Color32::from_rgb(50, 50, 50),

                // ── 编辑器 ──
                editor_panel_bg: Color32::from_rgb(255, 255, 255),
                editor_bg: Color32::from_rgb(250, 250, 250),
                editor_gutter_bg: Color32::from_rgb(244, 244, 244),
                editor_current_line_bg: Color32::from_rgb(228, 238, 252),
                editor_text: Color32::from_rgb(36, 36, 36),
                editor_line_number: Color32::from_rgb(125, 125, 125),
                editor_line_number_active: Color32::from_rgb(42, 70, 115),
                editor_keyword: Color32::from_rgb(0, 90, 195),
                editor_string: Color32::from_rgb(165, 86, 48),
                editor_number: Color32::from_rgb(55, 128, 82),
                editor_comment: Color32::from_rgb(108, 118, 108),

                // ── 自动补全 ──
                autocomplete_popup_bg: Color32::from_rgb(248, 248, 248),
                autocomplete_border: Color32::from_rgb(205, 205, 205),
                autocomplete_text: Color32::from_rgb(36, 36, 36),
                autocomplete_weak_text: Color32::from_rgb(125, 125, 125),
                autocomplete_selected_bg: Color32::from_rgb(8, 95, 212),
                autocomplete_selected_text: Color32::WHITE,
                autocomplete_match_blue: Color32::from_rgb(0, 100, 200),
                autocomplete_match_yellow: Color32::from_rgb(180, 160, 0),

                // ── 内联硬编码颜色 ──
                copy_button_bg: Color32::from_rgb(82, 138, 210),
                copy_button_stroke: Color32::from_rgb(60, 115, 185),
                delete_button_bg: Color32::from_rgb(255, 59, 48),
                delete_button_text: Color32::WHITE,
                confirm_button_bg: Color32::from_rgb(0, 122, 255),
                warning_icon: Color32::from_rgb(230, 130, 0),
                context_menu_hover: Color32::from_rgb(65, 125, 200),
                context_menu_active: Color32::from_rgb(50, 100, 170),
                search_highlight_bg: Color32::from_rgb(255, 230, 0),
                search_highlight_fg: Color32::from_rgb(80, 60, 0),
                explain_scan: Color32::from_rgb(65, 105, 225),
                explain_sort: Color32::from_rgb(210, 130, 0),
                explain_join: Color32::from_rgb(50, 170, 115),
                explain_filter: Color32::from_rgb(200, 170, 0),
                update_success_bg: Color32::from_rgb(220, 245, 220),
                update_error_bg: Color32::from_rgb(255, 225, 225),
                update_success_text: Color32::from_rgb(30, 120, 60),
                update_error_text: Color32::from_rgb(180, 50, 50),

                // ── egui 全局视觉 ──
                panel_fill: Color32::from_rgb(255, 255, 255),
                window_fill: Color32::from_rgb(255, 255, 255),
                extreme_bg: Color32::from_rgb(230, 230, 230),
                faint_bg: Color32::from_rgb(248, 248, 248),
                code_bg: Color32::from_rgb(240, 240, 240),
                window_stroke: Color32::from_rgb(218, 218, 218),
                widget_noninteractive_bg: Color32::from_rgb(240, 240, 240),
                widget_noninteractive_stroke: Color32::from_rgb(218, 218, 218),
                widget_noninteractive_fg: Color32::from_rgb(120, 120, 120),
                widget_inactive_bg: Color32::from_rgb(245, 245, 245),
                widget_inactive_stroke: Color32::from_rgb(210, 210, 210),
                widget_hovered_bg: Color32::from_rgb(238, 240, 245),
                widget_hovered_stroke: Color32::from_rgb(160, 180, 220),
                widget_active_bg: Color32::from_rgb(215, 225, 245),
                widget_active_stroke: Color32::from_rgb(120, 160, 215),
                widget_open_bg: Color32::from_rgb(238, 240, 245),
                widget_open_stroke: Color32::from_rgb(210, 210, 210),
                egui_selection_bg: Color32::from_rgba_premultiplied(140, 200, 255, 100),
                egui_selection_stroke: Color32::from_rgba_premultiplied(0, 80, 120, 130),

                // ── 滚动条 ──
                scrollbar_dormant_opacity: 0.30,
                scrollbar_active_opacity: 0.50,
                scrollbar_interact_opacity: 0.70,
            },
        }
    }
}
