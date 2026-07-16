use egui::Color32;

/// 命名字体大小
#[derive(Clone, Copy, Debug)]
pub struct FontSizes {
    /// 9.0 — resize handles、group-by 指示器（HIG Mini）
    pub tiny: f32,
    /// 11.0 — kind 标签、历史表（HIG Small）
    pub xs: f32,
    /// 11.0 — 摘要文字（HIG Small）
    pub sm: f32,
    /// 13.0 — 表格单元格、侧边栏项、对话框基础（HIG Body）
    pub base: f32,
    /// 13.0 — 标签页按钮、combo 项、对话框正文（HIG Control）
    pub md: f32,
    /// 13.0 — 表头、autocomplete 标签、输入标签
    pub lg: f32,
    /// 13.0 — 输入框、右键菜单项（HIG Menu Item）
    pub xl: f32,
    /// 15.0 — SQL 代码编辑器
    pub code: f32,
    /// 18.0 — 对话框标题
    pub heading: f32,
    /// 22.0 — 页面大标题
    pub title: f32,
    /// 36.0 — 空状态标题
    pub hero: f32,
    /// 12.0 — monospace SQL 代码块
    pub mono: f32,
    /// 40.0 — loading spinner 直径
    pub spinner_size: f32,
}

impl Default for FontSizes {
    fn default() -> Self {
        Self {
            tiny: 9.0,
            xs: 11.0,
            sm: 11.0,
            base: 13.0,
            md: 13.0,
            lg: 13.0,
            xl: 13.0,
            code: 15.0,
            heading: 18.0,
            title: 22.0,
            hero: 36.0,
            mono: 12.0,
            spinner_size: 40.0,
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
    pub confirm_button_text: Color32,
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

    // ── 错误徽章 ──
    pub error_badge_fill: Color32,
    pub error_badge_stroke: Color32,
    pub error_badge_text: Color32,

    // ── 弹窗遮罩与卡片 ──
    pub dialog_backdrop: Color32,
    pub dialog_card_bg: Color32,
    pub dialog_card_shadow: Color32,
    pub dialog_card_border: Color32,
    pub dialog_sql_block_bg: Color32,

    // ── NULL 预览 ──
    pub null_preview: Color32,

    // ── Toast ──
    pub toast_bg: Color32,
    pub toast_text: Color32,

    // ── 搜索匹配 ──
    pub search_match_bg: Color32,
    pub search_match_stroke: Color32,
    pub search_current_match_bg: Color32,
    pub search_current_match_stroke: Color32,
    pub sidebar_search_match_fg: Color32,

    // ── 右键菜单文字 ──
    pub context_menu_fg: Color32,

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

    // ── 圆角系统 ──
    /// 2.0 — 最小圆角
    pub radius_sm: f32,
    /// 4.0 — 小组件
    pub radius_md: f32,
    /// 6.0 — 按钮/输入框
    pub radius_lg: f32,
    /// 10.0 — 弹窗/面板
    pub radius_xl: f32,
    /// 16.0 — 大弹窗
    pub radius_xxl: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DarkVariant {
    Standard,
    Cool,
    Soft,
    Warm,
    Layered,
}

impl DarkVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "dark",
            Self::Cool => "dark_cool",
            Self::Soft => "dark_soft",
            Self::Warm => "dark_warm",
            Self::Layered => "dark_layered",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "dark_cool" => Self::Cool,
            "dark_soft" => Self::Soft,
            "dark_warm" => Self::Warm,
            "dark_layered" => Self::Layered,
            _ => Self::Standard,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightVariant {
    Standard,
    Warm,
    Cool,
    EyeCare,
    SoftGray,
}

impl LightVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "light",
            Self::Warm => "light_warm",
            Self::Cool => "light_cool",
            Self::EyeCare => "light_eyecare",
            Self::SoftGray => "light_softgray",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "light_warm" => Self::Warm,
            "light_cool" => Self::Cool,
            "light_eyecare" => Self::EyeCare,
            "light_softgray" => Self::SoftGray,
            _ => Self::Standard,
        }
    }
}

/// 统一主题
#[derive(Clone, Debug)]
pub struct Theme {
    pub colors: ThemeColors,
    pub fonts: FontSizes,
    pub dark_mode: bool,
    pub dark_variant: DarkVariant,
    pub light_variant: LightVariant,
}

impl Theme {
    pub fn new(dark_mode: bool, dark_variant: DarkVariant, light_variant: LightVariant) -> Self {
        if dark_mode {
            match dark_variant {
                DarkVariant::Standard => Self::dark(),
                DarkVariant::Cool => Self::dark_cool(),
                DarkVariant::Soft => Self::dark_soft(),
                DarkVariant::Warm => Self::dark_warm(),
                DarkVariant::Layered => Self::dark_layered(),
            }
        } else {
            match light_variant {
                LightVariant::Standard => Self::light(),
                LightVariant::Warm => Self::light_warm(),
                LightVariant::Cool => Self::light_cool(),
                LightVariant::EyeCare => Self::light_eyecare(),
                LightVariant::SoftGray => Self::light_softgray(),
            }
        }
    }

    pub fn from_visuals(visuals: &egui::Visuals, dark_variant: DarkVariant, light_variant: LightVariant) -> Self {
        Self::new(visuals.dark_mode, dark_variant, light_variant)
    }

    pub fn dark() -> Self {
        Self {
            dark_mode: true,
            dark_variant: DarkVariant::Standard,
            light_variant: LightVariant::Standard,
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
                weak_text: Color32::from_rgb(200, 200, 200),
                muted_dot: Color32::from_rgb(148, 148, 150),
                success: Color32::from_rgb(68, 188, 125),
                danger: Color32::from_rgb(255, 115, 115),
                warning: Color32::from_rgb(255, 190, 70),
                tab_idle_bg: Color32::from_rgb(52, 52, 54),

                // ── 按钮 ──
                primary_button_bg: Color32::TRANSPARENT,
                primary_button_stroke: Color32::from_rgb(120, 190, 255),
                primary_button_text: Color32::from_rgb(120, 190, 255),
                secondary_button_bg: Color32::from_rgb(99, 99, 102),
                secondary_button_stroke: Color32::from_rgb(120, 120, 123),
                secondary_button_text: Color32::from_rgb(245, 245, 245),
                accent_button_bg: Color32::from_rgb(44, 44, 46),
                accent_button_stroke: Color32::from_rgb(60, 155, 195),
                accent_button_text: Color32::from_rgb(60, 155, 195),
                accent_active_button_bg: Color32::from_rgb(44, 44, 46),
                accent_active_button_stroke: Color32::from_rgb(210, 165, 50),
                accent_active_button_text: Color32::from_rgb(210, 165, 50),
                modified_button_bg: Color32::from_rgb(165, 140, 46),
                modified_button_stroke: Color32::from_rgb(195, 170, 75),
                modified_button_text: Color32::WHITE,
                subtle_button_bg: Color32::from_rgb(52, 52, 54),
                subtle_button_stroke: Color32::from_rgb(105, 105, 108),
                subtle_button_text: Color32::from_rgb(232, 232, 232),
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
                dialog_primary_button_bg: Color32::TRANSPARENT,
                dialog_primary_button_stroke: Color32::from_rgb(120, 190, 255),
                dialog_primary_button_text: Color32::from_rgb(120, 190, 255),
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
                confirm_button_text: Color32::WHITE,
                warning_icon: Color32::from_rgb(255, 140, 0),
                context_menu_hover: Color32::from_rgb(50, 100, 170),
                context_menu_active: Color32::from_rgb(40, 80, 140),
                search_highlight_bg: Color32::from_rgb(124, 112, 48),
                search_highlight_fg: Color32::from_rgb(255, 240, 170),
                explain_scan: Color32::from_rgb(100, 149, 237),
                explain_sort: Color32::from_rgb(255, 165, 0),
                explain_join: Color32::from_rgb(72, 199, 142),
                explain_filter: Color32::from_rgb(255, 215, 0),
                update_success_bg: Color32::from_rgb(32, 60, 32),
                update_error_bg: Color32::from_rgb(60, 32, 32),
                update_success_text: Color32::from_rgb(80, 195, 120),
                update_error_text: Color32::from_rgb(220, 80, 80),

                // ── 错误徽章 ──
                error_badge_fill: Color32::from_rgb(60, 35, 35),
                error_badge_stroke: Color32::from_rgb(120, 50, 50),
                error_badge_text: Color32::from_rgb(255, 140, 140),

                // ── 弹窗遮罩与卡片 ──
                dialog_backdrop: Color32::from_rgba_premultiplied(0, 0, 0, 120),
                dialog_card_bg: Color32::from_rgb(40, 43, 50),
                dialog_card_shadow: Color32::from_rgba_premultiplied(0, 0, 0, 80),
                dialog_card_border: Color32::from_rgba_premultiplied(255, 255, 255, 25),
                dialog_sql_block_bg: Color32::from_rgb(30, 33, 40),

                // ── NULL 预览 ──
                null_preview: Color32::from_rgba_premultiplied(255, 128, 128, 200),

                // ── Toast ──
                toast_bg: Color32::from_rgba_premultiplied(40, 40, 40, 220),
                toast_text: Color32::WHITE,

                // ── 搜索匹配 ──
                search_match_bg: Color32::from_rgba_unmultiplied(36, 125, 168, 52),
                search_match_stroke: Color32::from_rgba_unmultiplied(104, 214, 255, 210),
                search_current_match_bg: Color32::from_rgba_unmultiplied(46, 112, 198, 168),
                search_current_match_stroke: Color32::from_rgba_unmultiplied(245, 249, 255, 235),
                sidebar_search_match_fg: Color32::from_rgb(148, 216, 255),

                // ── 右键菜单文字 ──
                context_menu_fg: Color32::WHITE,

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
                scrollbar_dormant_opacity: 0.0,
                scrollbar_active_opacity: 0.55,
                scrollbar_interact_opacity: 0.75,

                // ── 圆角系统 ──
                radius_sm: 2.0,
                radius_md: 4.0,
                radius_lg: 6.0,
                radius_xl: 10.0,
                radius_xxl: 16.0,
            },
        }
    }

    pub fn light() -> Self {
        Self {
            dark_mode: false,
            dark_variant: DarkVariant::Standard,
            light_variant: LightVariant::Standard,
            fonts: FontSizes::default(),
            colors: ThemeColors {
                // ── 通用 UI ──
                toolbar_bg: Color32::from_rgb(246, 246, 246),
                sidebar_bg: Color32::from_rgb(238, 238, 240),
                workspace_bg: Color32::from_rgb(246, 246, 246),
                card_bg: Color32::from_rgb(246, 246, 246),
                table_header_bg: Color32::from_rgb(243, 243, 243),
                table_alt_bg: Color32::from_rgb(248, 248, 248),
                search_bg: Color32::from_rgb(246, 246, 246),
                border: Color32::from_rgb(218, 218, 218),
                soft_border: Color32::from_rgb(232, 232, 232),
                table_grid: Color32::from_rgb(224, 224, 224),
                selection_bg: Color32::from_rgb(200, 220, 250),
                selection_stroke: Color32::from_rgb(125, 165, 225),
                selection_text: Color32::from_rgb(20, 60, 120),
                expand_arrow: Color32::from_rgb(68, 128, 200),
                text: Color32::from_rgb(40, 40, 40),
                weak_text: Color32::from_rgb(110, 110, 110),
                muted_dot: Color32::from_rgb(125, 125, 125),
                success: Color32::from_rgb(48, 167, 104),
                danger: Color32::from_rgb(220, 86, 86),
                warning: Color32::from_rgb(255, 179, 25),
                tab_idle_bg: Color32::from_rgb(245, 245, 245),

                // ── 按钮 ──
                primary_button_bg: Color32::TRANSPARENT,
                primary_button_stroke: Color32::from_rgb(60, 140, 230),
                primary_button_text: Color32::from_rgb(60, 140, 230),
                secondary_button_bg: Color32::from_rgb(180, 180, 183),
                secondary_button_stroke: Color32::from_rgb(155, 155, 158),
                secondary_button_text: Color32::WHITE,
                accent_button_bg: Color32::from_rgb(246, 246, 246),
                accent_button_stroke: Color32::from_rgb(60, 170, 220),
                accent_button_text: Color32::from_rgb(25, 110, 160),
                accent_active_button_bg: Color32::from_rgb(246, 246, 246),
                accent_active_button_stroke: Color32::from_rgb(190, 140, 20),
                accent_active_button_text: Color32::from_rgb(140, 105, 15),
                modified_button_bg: Color32::from_rgb(255, 243, 176),
                modified_button_stroke: Color32::from_rgb(228, 212, 118),
                modified_button_text: Color32::from_rgb(120, 100, 20),
                subtle_button_bg: Color32::from_rgb(248, 248, 248),
                subtle_button_stroke: Color32::from_rgb(200, 200, 200),
                subtle_button_text: Color32::from_rgb(55, 55, 55),
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
                dialog_primary_button_bg: Color32::TRANSPARENT,
                dialog_primary_button_stroke: Color32::from_rgb(60, 140, 230),
                dialog_primary_button_text: Color32::from_rgb(60, 140, 230),
                dialog_secondary_button_bg: Color32::from_rgb(180, 180, 183),
                dialog_secondary_button_stroke: Color32::from_rgb(155, 155, 158),
                dialog_secondary_button_text: Color32::from_rgb(50, 50, 50),

                // ── 编辑器 ──
                editor_panel_bg: Color32::from_rgb(252, 252, 252),
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
                confirm_button_text: Color32::WHITE,
                warning_icon: Color32::from_rgb(230, 130, 0),
                context_menu_hover: Color32::from_rgb(65, 125, 200),
                context_menu_active: Color32::from_rgb(50, 100, 170),
                search_highlight_bg: Color32::from_rgb(255, 235, 170),
                search_highlight_fg: Color32::from_rgb(118, 82, 18),
                explain_scan: Color32::from_rgb(65, 105, 225),
                explain_sort: Color32::from_rgb(210, 130, 0),
                explain_join: Color32::from_rgb(50, 170, 115),
                explain_filter: Color32::from_rgb(200, 170, 0),
                update_success_bg: Color32::from_rgb(220, 245, 220),
                update_error_bg: Color32::from_rgb(255, 225, 225),
                update_success_text: Color32::from_rgb(30, 120, 60),
                update_error_text: Color32::from_rgb(180, 50, 50),

                // ── 错误徽章 ──
                error_badge_fill: Color32::from_rgb(255, 235, 235),
                error_badge_stroke: Color32::from_rgb(220, 100, 100),
                error_badge_text: Color32::from_rgb(180, 30, 30),

                // ── 弹窗遮罩与卡片 ──
                dialog_backdrop: Color32::from_rgba_premultiplied(0, 0, 0, 60),
                dialog_card_bg: Color32::from_rgb(252, 252, 252),
                dialog_card_shadow: Color32::from_rgba_premultiplied(0, 0, 0, 25),
                dialog_card_border: Color32::from_rgba_premultiplied(0, 0, 0, 20),
                dialog_sql_block_bg: Color32::from_rgb(246, 248, 250),

                // ── NULL 预览 ──
                null_preview: Color32::from_rgba_premultiplied(255, 128, 128, 200),

                // ── Toast ──
                toast_bg: Color32::from_rgba_premultiplied(40, 40, 40, 220),
                toast_text: Color32::WHITE,

                // ── 搜索匹配 ──
                search_match_bg: Color32::from_rgba_unmultiplied(150, 210, 255, 58),
                search_match_stroke: Color32::from_rgba_unmultiplied(72, 150, 220, 188),
                search_current_match_bg: Color32::from_rgba_unmultiplied(91, 157, 255, 118),
                search_current_match_stroke: Color32::from_rgba_unmultiplied(255, 255, 255, 240),
                sidebar_search_match_fg: Color32::from_rgb(58, 126, 198),

                // ── 右键菜单文字 ──
                context_menu_fg: Color32::WHITE,

                // ── egui 全局视觉 ──
                panel_fill: Color32::from_rgb(246, 246, 246),
                window_fill: Color32::from_rgb(246, 246, 246),
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
                scrollbar_dormant_opacity: 0.0,
                scrollbar_active_opacity: 0.50,
                scrollbar_interact_opacity: 0.70,

                // ── 圆角系统 ──
                radius_sm: 2.0,
                radius_md: 4.0,
                radius_lg: 6.0,
                radius_xl: 10.0,
                radius_xxl: 16.0,
            },
        }
    }

    fn apply_dark_cool(c: &mut ThemeColors) {
        c.toolbar_bg = Color32::from_rgb(48, 48, 54);
        c.sidebar_bg = Color32::from_rgb(38, 38, 44);
        c.workspace_bg = Color32::from_rgb(48, 48, 54);
        c.card_bg = Color32::from_rgb(48, 48, 54);
        c.table_header_bg = Color32::from_rgb(56, 56, 64);
        c.table_alt_bg = Color32::from_rgb(44, 44, 52);
        c.search_bg = Color32::from_rgb(48, 48, 54);
        c.tab_idle_bg = Color32::from_rgb(46, 46, 52);
        c.accent_button_bg = Color32::from_rgb(38, 38, 44);
        c.accent_active_button_bg = Color32::from_rgb(38, 38, 44);
        c.subtle_button_bg = Color32::from_rgb(46, 46, 52);
        c.danger_button_bg = Color32::from_rgb(38, 38, 44);
        c.dialog_window_bg = Color32::from_rgb(44, 44, 50);
        c.dialog_section_bg = Color32::from_rgb(52, 52, 58);
        c.dialog_input_bg = Color32::from_rgb(66, 66, 72);
        c.dialog_input_hover_bg = Color32::from_rgb(74, 74, 80);
        c.dialog_input_active_bg = Color32::from_rgb(80, 80, 86);
        c.editor_panel_bg = Color32::from_rgb(48, 48, 54);
        c.editor_bg = Color32::from_rgb(36, 36, 42);
        c.editor_gutter_bg = Color32::from_rgb(34, 34, 40);
        c.autocomplete_popup_bg = Color32::from_rgb(40, 40, 46);
        c.panel_fill = Color32::from_rgb(48, 48, 54);
        c.window_fill = Color32::from_rgb(48, 48, 54);
        c.extreme_bg = Color32::from_rgb(66, 66, 75);
        c.faint_bg = Color32::from_rgb(52, 52, 58);
        c.code_bg = Color32::from_rgb(62, 62, 70);
        c.widget_noninteractive_bg = Color32::from_rgb(52, 52, 58);
    }

    fn apply_dark_soft(c: &mut ThemeColors) {
        c.toolbar_bg = Color32::from_rgb(58, 58, 62);
        c.sidebar_bg = Color32::from_rgb(48, 48, 52);
        c.workspace_bg = Color32::from_rgb(58, 58, 62);
        c.card_bg = Color32::from_rgb(58, 58, 62);
        c.table_header_bg = Color32::from_rgb(66, 66, 70);
        c.table_alt_bg = Color32::from_rgb(54, 54, 58);
        c.search_bg = Color32::from_rgb(58, 58, 62);
        c.tab_idle_bg = Color32::from_rgb(56, 56, 60);
        c.accent_button_bg = Color32::from_rgb(48, 48, 52);
        c.accent_active_button_bg = Color32::from_rgb(48, 48, 52);
        c.subtle_button_bg = Color32::from_rgb(56, 56, 60);
        c.danger_button_bg = Color32::from_rgb(48, 48, 52);
        c.dialog_window_bg = Color32::from_rgb(54, 54, 58);
        c.dialog_section_bg = Color32::from_rgb(62, 62, 66);
        c.dialog_input_bg = Color32::from_rgb(76, 76, 80);
        c.dialog_input_hover_bg = Color32::from_rgb(84, 84, 88);
        c.dialog_input_active_bg = Color32::from_rgb(90, 90, 94);
        c.editor_panel_bg = Color32::from_rgb(58, 58, 62);
        c.editor_bg = Color32::from_rgb(46, 46, 50);
        c.editor_gutter_bg = Color32::from_rgb(44, 44, 48);
        c.autocomplete_popup_bg = Color32::from_rgb(50, 50, 54);
        c.panel_fill = Color32::from_rgb(58, 58, 62);
        c.window_fill = Color32::from_rgb(58, 58, 62);
        c.extreme_bg = Color32::from_rgb(76, 76, 80);
        c.faint_bg = Color32::from_rgb(62, 62, 66);
        c.code_bg = Color32::from_rgb(72, 72, 76);
        c.widget_noninteractive_bg = Color32::from_rgb(62, 62, 66);
    }

    fn apply_dark_warm(c: &mut ThemeColors) {
        c.toolbar_bg = Color32::from_rgb(52, 50, 48);
        c.sidebar_bg = Color32::from_rgb(42, 40, 38);
        c.workspace_bg = Color32::from_rgb(52, 50, 48);
        c.card_bg = Color32::from_rgb(52, 50, 48);
        c.table_header_bg = Color32::from_rgb(60, 58, 56);
        c.table_alt_bg = Color32::from_rgb(48, 46, 44);
        c.search_bg = Color32::from_rgb(52, 50, 48);
        c.tab_idle_bg = Color32::from_rgb(50, 48, 46);
        c.accent_button_bg = Color32::from_rgb(42, 40, 38);
        c.accent_active_button_bg = Color32::from_rgb(42, 40, 38);
        c.subtle_button_bg = Color32::from_rgb(50, 48, 46);
        c.danger_button_bg = Color32::from_rgb(42, 40, 38);
        c.dialog_window_bg = Color32::from_rgb(48, 46, 44);
        c.dialog_section_bg = Color32::from_rgb(56, 54, 52);
        c.dialog_input_bg = Color32::from_rgb(70, 68, 66);
        c.dialog_input_hover_bg = Color32::from_rgb(78, 76, 74);
        c.dialog_input_active_bg = Color32::from_rgb(84, 82, 80);
        c.editor_panel_bg = Color32::from_rgb(52, 50, 48);
        c.editor_bg = Color32::from_rgb(40, 38, 36);
        c.editor_gutter_bg = Color32::from_rgb(38, 36, 34);
        c.autocomplete_popup_bg = Color32::from_rgb(44, 42, 40);
        c.panel_fill = Color32::from_rgb(52, 50, 48);
        c.window_fill = Color32::from_rgb(52, 50, 48);
        c.extreme_bg = Color32::from_rgb(70, 68, 66);
        c.faint_bg = Color32::from_rgb(56, 54, 52);
        c.code_bg = Color32::from_rgb(66, 64, 62);
        c.widget_noninteractive_bg = Color32::from_rgb(56, 54, 52);
    }

    fn apply_dark_layered(c: &mut ThemeColors) {
        // 层次方案：侧栏/工具栏保持深色，工作区/内容区更亮，形成明显层次
        c.toolbar_bg = Color32::from_rgb(48, 48, 52);
        c.sidebar_bg = Color32::from_rgb(40, 40, 44);
        c.workspace_bg = Color32::from_rgb(60, 60, 64);
        c.card_bg = Color32::from_rgb(60, 60, 64);
        c.table_header_bg = Color32::from_rgb(68, 68, 72);
        c.table_alt_bg = Color32::from_rgb(56, 56, 60);
        c.search_bg = Color32::from_rgb(60, 60, 64);
        c.tab_idle_bg = Color32::from_rgb(50, 50, 54);
        c.accent_button_bg = Color32::from_rgb(40, 40, 44);
        c.accent_active_button_bg = Color32::from_rgb(40, 40, 44);
        c.subtle_button_bg = Color32::from_rgb(50, 50, 54);
        c.danger_button_bg = Color32::from_rgb(40, 40, 44);
        c.dialog_window_bg = Color32::from_rgb(56, 56, 60);
        c.dialog_section_bg = Color32::from_rgb(64, 64, 68);
        c.dialog_input_bg = Color32::from_rgb(78, 78, 82);
        c.dialog_input_hover_bg = Color32::from_rgb(86, 86, 90);
        c.dialog_input_active_bg = Color32::from_rgb(92, 92, 96);
        c.editor_panel_bg = Color32::from_rgb(60, 60, 64);
        c.editor_bg = Color32::from_rgb(48, 48, 52);
        c.editor_gutter_bg = Color32::from_rgb(46, 46, 50);
        c.autocomplete_popup_bg = Color32::from_rgb(52, 52, 56);
        c.panel_fill = Color32::from_rgb(54, 54, 56);
        c.window_fill = Color32::from_rgb(54, 54, 56);
        c.extreme_bg = Color32::from_rgb(78, 78, 82);
        c.faint_bg = Color32::from_rgb(64, 64, 68);
        c.code_bg = Color32::from_rgb(74, 74, 78);
        c.widget_noninteractive_bg = Color32::from_rgb(64, 64, 68);
    }

    pub fn dark_cool() -> Self { let mut t = Self::dark(); t.dark_variant = DarkVariant::Cool; Self::apply_dark_cool(&mut t.colors); t }
    pub fn dark_soft() -> Self { let mut t = Self::dark(); t.dark_variant = DarkVariant::Soft; Self::apply_dark_soft(&mut t.colors); t }
    pub fn dark_warm() -> Self { let mut t = Self::dark(); t.dark_variant = DarkVariant::Warm; Self::apply_dark_warm(&mut t.colors); t }
    pub fn dark_layered() -> Self { let mut t = Self::dark(); t.dark_variant = DarkVariant::Layered; Self::apply_dark_layered(&mut t.colors); t }

    fn apply_light_warm(c: &mut ThemeColors) {
        let main   = Color32::from_rgb(248, 246, 242);
        let side   = Color32::from_rgb(240, 237, 231);
        let editor = Color32::from_rgb(252, 251, 249);
        let mid    = Color32::from_rgb(244, 241, 236);
        let light  = Color32::from_rgb(250, 249, 246);
        let input  = Color32::from_rgb(254, 253, 251);
        c.toolbar_bg = main; c.sidebar_bg = side; c.workspace_bg = main;
        c.card_bg = main; c.search_bg = main; c.panel_fill = main; c.window_fill = main;
        c.editor_panel_bg = editor; c.editor_bg = light; c.editor_gutter_bg = mid;
        c.table_header_bg = mid; c.table_alt_bg = light;
        c.tab_idle_bg = mid; c.accent_button_bg = mid; c.accent_active_button_bg = mid;
        c.subtle_button_bg = light; c.danger_button_bg = main;
        c.dialog_window_bg = mid; c.dialog_section_bg = Color32::from_rgb(252, 250, 247);
        c.dialog_input_bg = input; c.dialog_input_hover_bg = Color32::from_rgb(252, 250, 247); c.dialog_input_active_bg = input;
        c.autocomplete_popup_bg = light; c.faint_bg = light;
        c.extreme_bg = Color32::from_rgb(232, 229, 224); c.code_bg = side;
        c.widget_noninteractive_bg = mid;
    }

    fn apply_light_cool(c: &mut ThemeColors) {
        let main   = Color32::from_rgb(246, 247, 252);
        let side   = Color32::from_rgb(237, 239, 246);
        let editor = Color32::from_rgb(251, 251, 254);
        let mid    = Color32::from_rgb(242, 243, 249);
        let light  = Color32::from_rgb(249, 249, 253);
        let input  = Color32::from_rgb(253, 253, 255);
        c.toolbar_bg = main; c.sidebar_bg = side; c.workspace_bg = main;
        c.card_bg = main; c.search_bg = main; c.panel_fill = main; c.window_fill = main;
        c.editor_panel_bg = editor; c.editor_bg = light; c.editor_gutter_bg = mid;
        c.table_header_bg = mid; c.table_alt_bg = light;
        c.tab_idle_bg = mid; c.accent_button_bg = mid; c.accent_active_button_bg = mid;
        c.subtle_button_bg = light; c.danger_button_bg = main;
        c.dialog_window_bg = mid; c.dialog_section_bg = Color32::from_rgb(250, 250, 255);
        c.dialog_input_bg = input; c.dialog_input_hover_bg = Color32::from_rgb(250, 250, 255); c.dialog_input_active_bg = input;
        c.autocomplete_popup_bg = light; c.faint_bg = light;
        c.extreme_bg = Color32::from_rgb(230, 232, 240); c.code_bg = side;
        c.widget_noninteractive_bg = mid;
    }

    fn apply_light_eyecare(c: &mut ThemeColors) {
        let main   = Color32::from_rgb(248, 246, 240);
        let side   = Color32::from_rgb(240, 237, 228);
        let editor = Color32::from_rgb(252, 251, 248);
        let mid    = Color32::from_rgb(244, 241, 233);
        let light  = Color32::from_rgb(250, 249, 244);
        let input  = Color32::from_rgb(254, 253, 250);
        c.toolbar_bg = main; c.sidebar_bg = side; c.workspace_bg = main;
        c.card_bg = main; c.search_bg = main; c.panel_fill = main; c.window_fill = main;
        c.editor_panel_bg = editor; c.editor_bg = light; c.editor_gutter_bg = mid;
        c.table_header_bg = mid; c.table_alt_bg = light;
        c.tab_idle_bg = mid; c.accent_button_bg = mid; c.accent_active_button_bg = mid;
        c.subtle_button_bg = light; c.danger_button_bg = main;
        c.dialog_window_bg = mid; c.dialog_section_bg = Color32::from_rgb(252, 250, 245);
        c.dialog_input_bg = input; c.dialog_input_hover_bg = Color32::from_rgb(252, 250, 245); c.dialog_input_active_bg = input;
        c.autocomplete_popup_bg = light; c.faint_bg = light;
        c.extreme_bg = Color32::from_rgb(234, 231, 224); c.code_bg = side;
        c.widget_noninteractive_bg = mid;
    }

    fn apply_light_softgray(c: &mut ThemeColors) {
        let main   = Color32::from_rgb(244, 244, 248);
        let side   = Color32::from_rgb(235, 235, 240);
        let editor = Color32::from_rgb(250, 250, 253);
        let mid    = Color32::from_rgb(240, 240, 245);
        let light  = Color32::from_rgb(248, 248, 251);
        let input  = Color32::from_rgb(252, 252, 254);
        c.toolbar_bg = main; c.sidebar_bg = side; c.workspace_bg = main;
        c.card_bg = main; c.search_bg = main; c.panel_fill = main; c.window_fill = main;
        c.editor_panel_bg = editor; c.editor_bg = light; c.editor_gutter_bg = mid;
        c.table_header_bg = mid; c.table_alt_bg = light;
        c.tab_idle_bg = mid; c.accent_button_bg = mid; c.accent_active_button_bg = mid;
        c.subtle_button_bg = light; c.danger_button_bg = main;
        c.dialog_window_bg = mid; c.dialog_section_bg = Color32::from_rgb(251, 251, 254);
        c.dialog_input_bg = input; c.dialog_input_hover_bg = Color32::from_rgb(251, 251, 254); c.dialog_input_active_bg = input;
        c.autocomplete_popup_bg = light; c.faint_bg = light;
        c.extreme_bg = Color32::from_rgb(230, 230, 235); c.code_bg = side;
        c.widget_noninteractive_bg = mid;
    }

    pub fn light_warm() -> Self { let mut t = Self::light(); t.light_variant = LightVariant::Warm; Self::apply_light_warm(&mut t.colors); t }
    pub fn light_cool() -> Self { let mut t = Self::light(); t.light_variant = LightVariant::Cool; Self::apply_light_cool(&mut t.colors); t }
    pub fn light_eyecare() -> Self { let mut t = Self::light(); t.light_variant = LightVariant::EyeCare; Self::apply_light_eyecare(&mut t.colors); t }
    pub fn light_softgray() -> Self { let mut t = Self::light(); t.light_variant = LightVariant::SoftGray; Self::apply_light_softgray(&mut t.colors); t }
}
