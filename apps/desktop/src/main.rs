#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod autocomplete;
mod icon;

use app::DesktopApp;
use app_services::AppServices;
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use i18n::{self, tr, Locale};
use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> eframe::Result<()> {
    const APP_NAME: &str = "freedb";

    // Initialize i18n — load saved locale or detect from system
    // We need AppServices for UI state first, so this gets set after services init
    // For now, detect system locale for startup messages
    let mut locale = Locale::detect_system();
    i18n::set_locale(locale);

    // Single-instance lock via TCP port
    let _lock = TcpListener::bind("127.0.0.1:19919").unwrap_or_else(|_| {
        eprintln!("{}", tr!("freedb 已在运行中（端口 19919 已被占用）"));
        std::process::exit(1);
    });

    let log_buffer: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .with(LogBufferLayer {
            buffer: log_buffer.clone(),
        })
        .init();

    let runtime = Runtime::new().expect("failed to create tokio runtime");
    let services = AppServices::new().expect("failed to initialize services");

    // 加载已保存的语言偏好（优先于系统检测）
    if let Ok(Some(saved)) = services.load_ui_state("locale") {
        if let Some(loc) = Locale::from_code(&saved) {
            locale = loc;
            i18n::set_locale(loc);
        }
    }

    // ---- 原生菜单栏（macOS/Windows 使用 muda，Linux 跳过） ----
    // 菜单在 DesktopApp 首帧 update() 时才挂载到 NSApp，
    // 避免被 winit 事件循环启动时创建的默认菜单覆盖。
    let (menu_event_rx, native_menu, menu_file, menu_view, menu_settings, menu_shortcuts, menu_log, menu_lang, menu_scroll_speed, menu_theme) = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
        let (tx, rx) = mpsc::channel();
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            let _ = tx.send(event);
        }));

        let menu = muda::Menu::new();

        if cfg!(target_os = "macos") {
            let about_meta = muda::AboutMetadata { name: Some("FreeDB".into()), ..muda::from_cargo_metadata!() };
            let app_menu = muda::Submenu::with_items(
                "FreeDB",
                true,
                &[
                    &muda::PredefinedMenuItem::about(Some(&tr!("关于 FreeDB")), Some(about_meta)),
                    &muda::PredefinedMenuItem::separator(),
                    &muda::PredefinedMenuItem::quit(Some(&tr!("退出 FreeDB"))),
                ],
            )
            .unwrap();
            menu.append(&app_menu).unwrap();
        }

        let mi_new_conn = muda::MenuItem::with_id("新建连接", &tr!("新建连接"), true, None::<muda::accelerator::Accelerator>);
        let mi_new_query = muda::MenuItem::with_id("新建查询", &tr!("新建查询"), true, Some(muda::accelerator::Accelerator::new(Some(muda::accelerator::Modifiers::SUPER), muda::accelerator::Code::KeyD)));
        let mi_import = muda::MenuItem::with_id("导入配置", &tr!("导入配置"), true, None::<muda::accelerator::Accelerator>);
        let mi_export = muda::MenuItem::with_id("导出配置", &tr!("导出配置"), true, None::<muda::accelerator::Accelerator>);

        let file_menu = muda::Submenu::with_items(
            &tr!("文件"),
            true,
            &[
                &mi_new_conn as &dyn muda::IsMenuItem,
                &mi_new_query as &dyn muda::IsMenuItem,
                &muda::PredefinedMenuItem::separator(),
                &mi_import as &dyn muda::IsMenuItem,
                &mi_export as &dyn muda::IsMenuItem,
            ],
        )
        .unwrap();
        menu.append(&file_menu).unwrap();

        let mi_shortcuts = muda::MenuItem::with_id("快捷键速查表", &tr!("快捷键速查表"), true, None::<muda::accelerator::Accelerator>);
        let mi_log = muda::MenuItem::with_id("运行日志", &tr!("运行日志"), true, None::<muda::accelerator::Accelerator>);
        let lang_label = if locale == Locale::En { "中文" } else { "English" };
        let mi_lang = muda::MenuItem::with_id("切换语言", lang_label, true, None::<muda::accelerator::Accelerator>);
        let mi_scroll_speed = muda::MenuItem::with_id("滚动速度", &tr!("滚动速度"), true, None::<muda::accelerator::Accelerator>);
        let theme_submenu = muda::Submenu::new(&tr!("主题"), true);

        let view_menu = muda::Submenu::with_items(
            &tr!("查看"),
            true,
            &[
                &mi_shortcuts,
                &mi_log,
            ],
        )
        .unwrap();
        menu.append(&view_menu).unwrap();

        let settings_menu = muda::Submenu::new(&tr!("设置"), true);
        settings_menu.append(&mi_lang).unwrap();
        settings_menu.append(&theme_submenu).unwrap();
        settings_menu.append(&mi_scroll_speed).unwrap();
        if !cfg!(target_os = "macos") {
            let about_meta = muda::AboutMetadata { name: Some("FreeDB".into()), ..muda::from_cargo_metadata!() };
            settings_menu.append(&muda::PredefinedMenuItem::about(Some(&tr!("关于 FreeDB")), Some(about_meta))).unwrap();
        }
        menu.append(&settings_menu).unwrap();

        (Some(rx), Some(menu), Some(file_menu), Some(view_menu), Some(settings_menu), Some(mi_shortcuts), Some(mi_log), Some(mi_lang), Some(mi_scroll_speed), Some(theme_submenu))
    } else {
        (None, None, None, None, None, None, None, None, None, None)
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("")
            .with_icon(icon::app_icon_data(256))
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([1200.0, 760.0])
            .with_titlebar_shown(false)
            .with_fullsize_content_view(true),
        // 最大化由 DesktopApp::update() 中 Windows 原生 ShowWindow 处理，
        // 不走 ViewportBuilder，避免 winit 内部最大化状态与实际窗口不同步
        persist_window: false,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(DesktopApp::new(runtime, services, log_buffer, menu_event_rx, native_menu, menu_file, menu_view, menu_settings, menu_shortcuts, menu_log, menu_lang, menu_scroll_speed, menu_theme, locale)))
        }),
    )
}

fn configure_fonts(ctx: &egui::Context) {
    // 修改焦点交出策略：阻止单击编辑器外部无关区域导致选中消失。
    // 默认是 SurrenderFocusOn::Clicks — 点击任意位置即交出焦点。
    // 改为 SurrenderFocusOn::Never — 只有明确请求才交焦点。
    ctx.memory_mut(|mem| {
        mem.options.input_options.surrender_focus_on = egui::SurrenderFocusOn::Never;
    });

    let mut fonts = FontDefinitions::default();

    // ---- macOS ----
    if cfg!(target_os = "macos") {
        let platform_fonts = &[
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
        ];
        let mut found = false;
        for path in platform_fonts {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("system-cjk".into(), FontData::from_owned(bytes).into());
                found = true;
                break;
            }
        }
        if !found {
            let embedded = include_bytes!("../assets/fonts/NotoSansSC-Regular.otf");
            fonts
                .font_data
                .insert("system-cjk".into(), FontData::from_owned(embedded.to_vec()).into());
        }
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system-cjk".into());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("system-cjk".into());
    }

    // ---- Windows ----
    else if cfg!(target_os = "windows") {
        // CJK font for Chinese text
        let cjk_fonts = &[
            "C:\\Windows\\Fonts\\msyh.ttc",   // Microsoft YaHei 微软雅黑 (Win10+)
            "C:\\Windows\\Fonts\\simhei.ttf", // SimHei 黑体 (legacy)
            "C:\\Windows\\Fonts\\simsun.ttc", // SimSun 宋体 (legacy)
        ];
        let mut cjk_found = false;
        for path in cjk_fonts {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("system-cjk".into(), FontData::from_owned(bytes).into());
                cjk_found = true;
                break;
            }
        }
        if !cjk_found {
            let embedded = include_bytes!("../assets/fonts/NotoSansSC-Regular.otf");
            fonts
                .font_data
                .insert("system-cjk".into(), FontData::from_owned(embedded.to_vec()).into());
        }

        // Segoe UI Symbol for Geometric Shapes icons (◎ ◫ ◇ ▦ ◪ etc.)
        // Must come before CJK in the fallback chain because CJK fonts
        // don't contain these symbols and would render them as tofu.
        let proportional = fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default();
        if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\seguisym.ttf") {
            fonts
                .font_data
                .insert("system-symbol".into(), FontData::from_owned(bytes).into());
            proportional.insert(0, "system-symbol".into());
        }
        proportional.insert(0, "system-cjk".into());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("system-cjk".into());
    }

    // ---- Linux ----
    else {
        let platform_fonts = &[
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        ];
        let mut found = false;
        for path in platform_fonts {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("system-cjk".into(), FontData::from_owned(bytes).into());
                found = true;
                break;
            }
        }
        if !found {
            let embedded = include_bytes!("../assets/fonts/NotoSansSC-Regular.otf");
            fonts
                .font_data
                .insert("system-cjk".into(), FontData::from_owned(embedded.to_vec()).into());
        }
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system-cjk".into());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("system-cjk".into());
    }

    ctx.set_fonts(fonts);
}

// ---- 日志缓冲 Layer：将 tracing 日志写入内存供运行日志窗口读取 ----

struct LogBufferLayer {
    buffer: Arc<Mutex<Vec<String>>>,
}

impl<S> tracing_subscriber::Layer<S> for LogBufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = LogVisitor(String::new());
        event.record(&mut visitor);
        let level = event.metadata().level();
        let now = chrono::Local::now().format("%H:%M:%S%.3f");
        let line = format!("[{} {}] {}", now, level, visitor.0);
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(line);
            // 限制最大行数，防止内存无限增长
            if buf.len() > 10_000 {
                buf.drain(0..5_000);
            }
        }
    }
}

struct LogVisitor(String);

impl tracing::field::Visit for LogVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if self.0.is_empty() {
            self.0 = format!("{}: {:?}", field.name(), value);
        } else {
            self.0.push_str(&format!(" {}: {:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if self.0.is_empty() {
            self.0 = format!("{}: {}", field.name(), value);
        } else {
            self.0.push_str(&format!(" {}: {}", field.name(), value));
        }
    }
}
