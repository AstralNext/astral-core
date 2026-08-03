//! TUI 状态机与事件循环。

use std::io::{self, stdout};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use service_manager::ServiceStatus;

use crate::service::{
    self, InstallOptions, RollbackOptions, ServiceActionOptions, UpdateOptions,
};

use super::ui;

/// 启动本地部署 TUI（阻塞至退出）。
pub fn run_wizard() -> Result<()> {
    enable_raw_mode().context("启用终端 raw mode 失败")?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen).context("进入备用屏幕失败")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("创建 TUI terminal 失败")?;

    let mut app = App::new();
    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            break;
        }
        if app.handle_key(key.code)? {
            break;
        }
    }
    Ok(())
}

/// 界面模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 主菜单。
    Menu,
    /// 安装表单。
    Install,
    /// 确认框。
    Confirm,
    /// 结果/日志。
    Result,
}

/// 确认动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// 执行安装。
    Install,
    /// 卸载。
    Uninstall,
    /// 更新。
    Update,
    /// 回滚。
    Rollback,
}

/// 安装表单字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallField {
    Name,
    Listen,
    DataDir,
    InstallRoot,
    Controller,
    ControllerToken,
    StartAfter,
}

impl InstallField {
    fn all() -> &'static [InstallField] {
        &[
            InstallField::Name,
            InstallField::Listen,
            InstallField::DataDir,
            InstallField::InstallRoot,
            InstallField::Controller,
            InstallField::ControllerToken,
            InstallField::StartAfter,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Name => "实例名",
            Self::Listen => "监听地址",
            Self::DataDir => "数据目录",
            Self::InstallRoot => "安装根目录",
            Self::Controller => "控制端 URL",
            Self::ControllerToken => "控制端 Token",
            Self::StartAfter => "装后启动",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Name => "字母数字，可含 - _",
            Self::Listen => "例如 127.0.0.1:50051",
            Self::DataDir => "空=平台默认",
            Self::InstallRoot => "空=平台默认",
            Self::Controller => "空=不配置出站",
            Self::ControllerToken => "启用控制端时必填",
            Self::StartAfter => "空格切换 是/否",
        }
    }
}

/// 主菜单项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Install,
    Start,
    Stop,
    Status,
    Update,
    Rollback,
    Versions,
    Uninstall,
    Quit,
}

impl MenuItem {
    fn all() -> &'static [MenuItem] {
        &[
            MenuItem::Install,
            MenuItem::Start,
            MenuItem::Stop,
            MenuItem::Status,
            MenuItem::Update,
            MenuItem::Rollback,
            MenuItem::Versions,
            MenuItem::Uninstall,
            MenuItem::Quit,
        ]
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Install => "安装为系统服务",
            Self::Start => "启动服务",
            Self::Stop => "停止服务",
            Self::Status => "查看状态",
            Self::Update => "更新版本",
            Self::Rollback => "回滚版本",
            Self::Versions => "列出已装版本",
            Self::Uninstall => "卸载服务",
            Self::Quit => "退出",
        }
    }

    pub(crate) fn subtitle(self) -> &'static str {
        match self {
            Self::Install => "首次部署推荐",
            Self::Start | Self::Stop | Self::Status => "需已安装",
            Self::Update | Self::Rollback | Self::Versions => "版本布局",
            Self::Uninstall => "从系统移除服务",
            Self::Quit => "Esc / q",
        }
    }
}

/// TUI 应用状态。
pub struct App {
    pub mode: Mode,
    pub menu_idx: usize,
    pub install_focus: usize,
    pub name: String,
    pub listen: String,
    pub data_dir: String,
    pub install_root: String,
    pub controller: String,
    pub controller_token: String,
    pub start_after: bool,
    pub status_line: String,
    pub log: Vec<String>,
    pub confirm: Option<ConfirmKind>,
    pub confirm_yes: bool,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            mode: Mode::Menu,
            menu_idx: 0,
            install_focus: 0,
            name: "default".into(),
            listen: "127.0.0.1:50051".into(),
            data_dir: String::new(),
            install_root: String::new(),
            controller: String::new(),
            controller_token: String::new(),
            start_after: true,
            status_line: String::new(),
            log: vec![
                "Up/Down 选择  Enter 确认  Tab 切换字段  Esc 返回  q 退出".into(),
                "Windows 安装/启停服务请以管理员身份打开本终端。".into(),
            ],
            confirm: None,
            confirm_yes: true,
        };
        app.refresh_status();
        app
    }

    pub fn menu_items(&self) -> &'static [MenuItem] {
        MenuItem::all()
    }

    pub fn install_fields(&self) -> &'static [InstallField] {
        InstallField::all()
    }

    pub fn focused_field(&self) -> InstallField {
        InstallField::all()[self.install_focus]
    }

    pub fn field_value(&self, f: InstallField) -> String {
        match f {
            InstallField::Name => self.name.clone(),
            InstallField::Listen => self.listen.clone(),
            InstallField::DataDir => self.data_dir.clone(),
            InstallField::InstallRoot => self.install_root.clone(),
            InstallField::Controller => self.controller.clone(),
            InstallField::ControllerToken => {
                if self.controller_token.is_empty() {
                    String::new()
                } else {
                    "*".repeat(self.controller_token.chars().count().min(24))
                }
            }
            InstallField::StartAfter => {
                if self.start_after {
                    "是".into()
                } else {
                    "否".into()
                }
            }
        }
    }

    pub fn field_label(f: InstallField) -> &'static str {
        f.label()
    }

    pub fn field_hint(f: InstallField) -> &'static str {
        f.hint()
    }

    fn push_log(&mut self, line: impl AsRef<str>) {
        self.log.push(line.as_ref().to_string());
        if self.log.len() > 200 {
            let drain = self.log.len() - 200;
            self.log.drain(0..drain);
        }
    }

    fn refresh_status(&mut self) {
        let opts = ServiceActionOptions {
            name: self.name.clone(),
            user: false,
        };
        match service::status(opts) {
            Ok(st) => {
                self.status_line = format!("实例 {} | {}", self.name, status_text(&st));
            }
            Err(e) => {
                self.status_line = format!("实例 {} | 状态读取失败: {e}", self.name);
            }
        }
    }

    /// 处理按键；返回 true 表示退出。
    fn handle_key(&mut self, code: KeyCode) -> Result<bool> {
        match self.mode {
            Mode::Menu => self.handle_menu(code),
            Mode::Install => self.handle_install(code),
            Mode::Confirm => self.handle_confirm(code),
            Mode::Result => self.handle_result(code),
        }
    }

    fn handle_menu(&mut self, code: KeyCode) -> Result<bool> {
        let items = MenuItem::all();
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.menu_idx == 0 {
                    self.menu_idx = items.len() - 1;
                } else {
                    self.menu_idx -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.menu_idx = (self.menu_idx + 1) % items.len();
            }
            KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
            KeyCode::Enter => {
                match items[self.menu_idx] {
                    MenuItem::Quit => return Ok(true),
                    MenuItem::Install => {
                        self.mode = Mode::Install;
                        self.install_focus = 0;
                    }
                    MenuItem::Start => self.run_simple("start")?,
                    MenuItem::Stop => self.run_simple("stop")?,
                    MenuItem::Status => {
                        self.refresh_status();
                        self.push_log(self.status_line.clone());
                        self.show_result("状态已刷新");
                    }
                    MenuItem::Versions => self.run_versions()?,
                    MenuItem::Uninstall => {
                        self.confirm = Some(ConfirmKind::Uninstall);
                        self.confirm_yes = false;
                        self.mode = Mode::Confirm;
                    }
                    MenuItem::Update => {
                        self.confirm = Some(ConfirmKind::Update);
                        self.confirm_yes = true;
                        self.mode = Mode::Confirm;
                    }
                    MenuItem::Rollback => {
                        self.confirm = Some(ConfirmKind::Rollback);
                        self.confirm_yes = true;
                        self.mode = Mode::Confirm;
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_install(&mut self, code: KeyCode) -> Result<bool> {
        let fields = InstallField::all();
        let focus = fields[self.install_focus];
        match code {
            KeyCode::Esc => {
                self.mode = Mode::Menu;
                self.refresh_status();
            }
            KeyCode::Tab | KeyCode::Down => {
                self.install_focus = (self.install_focus + 1) % fields.len();
            }
            KeyCode::BackTab | KeyCode::Up => {
                if self.install_focus == 0 {
                    self.install_focus = fields.len() - 1;
                } else {
                    self.install_focus -= 1;
                }
            }
            KeyCode::Enter => {
                if focus == InstallField::StartAfter {
                    self.start_after = !self.start_after;
                } else {
                    self.confirm = Some(ConfirmKind::Install);
                    self.confirm_yes = true;
                    self.mode = Mode::Confirm;
                }
            }
            KeyCode::Char(' ') if focus == InstallField::StartAfter => {
                self.start_after = !self.start_after;
            }
            KeyCode::Backspace => self.edit_field(focus, Edit::Backspace),
            KeyCode::Char(c) => {
                if focus != InstallField::StartAfter {
                    self.edit_field(focus, Edit::Push(c));
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_confirm(&mut self, code: KeyCode) -> Result<bool> {
        match code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('h') | KeyCode::Char('l') => {
                self.confirm_yes = !self.confirm_yes;
            }
            KeyCode::Esc | KeyCode::Char('n') => {
                self.confirm = None;
                self.mode = Mode::Menu;
            }
            KeyCode::Enter | KeyCode::Char('y') => {
                let kind = self.confirm.take();
                if self.confirm_yes {
                    if let Some(kind) = kind {
                        self.execute_confirm(kind)?;
                    }
                } else {
                    self.mode = Mode::Menu;
                    self.push_log("已取消");
                }
            }
            KeyCode::Char('Y') => {
                if let Some(kind) = self.confirm.take() {
                    self.confirm_yes = true;
                    self.execute_confirm(kind)?;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_result(&mut self, code: KeyCode) -> Result<bool> {
        match code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::Menu;
                self.refresh_status();
            }
            _ => {}
        }
        Ok(false)
    }

    fn edit_field(&mut self, f: InstallField, edit: Edit) {
        let slot = match f {
            InstallField::Name => &mut self.name,
            InstallField::Listen => &mut self.listen,
            InstallField::DataDir => &mut self.data_dir,
            InstallField::InstallRoot => &mut self.install_root,
            InstallField::Controller => &mut self.controller,
            InstallField::ControllerToken => &mut self.controller_token,
            InstallField::StartAfter => return,
        };
        match edit {
            Edit::Backspace => {
                slot.pop();
            }
            Edit::Push(c) => {
                if !c.is_control() {
                    slot.push(c);
                }
            }
        }
    }

    fn show_result(&mut self, title: &str) {
        self.push_log(format!("-- {title} --"));
        self.mode = Mode::Result;
    }

    fn execute_confirm(&mut self, kind: ConfirmKind) -> Result<()> {
        match kind {
            ConfirmKind::Install => self.run_install()?,
            ConfirmKind::Uninstall => self.run_simple("uninstall")?,
            ConfirmKind::Update => self.run_update()?,
            ConfirmKind::Rollback => self.run_rollback()?,
        }
        Ok(())
    }

    fn run_install(&mut self) -> Result<()> {
        if let Err(e) = validate_install(self) {
            self.push_log(format!("校验失败: {e}"));
            self.show_result("安装未开始");
            return Ok(());
        }
        let listen: SocketAddr = self.listen.parse().context("监听地址")?;
        let opts = InstallOptions {
            name: self.name.clone(),
            listen,
            data_dir: nonempty_path(&self.data_dir),
            program: None,
            install_root: nonempty_path(&self.install_root),
            version: None,
            retain: 3,
            user: false,
            start_after_install: self.start_after,
            controller: nonempty_string(&self.controller),
            controller_token: nonempty_string(&self.controller_token),
            controller_tls_ca: None,
            controller_tls_domain: None,
        };
        self.push_log(format!(
            "正在安装 dev.astral.core-{} @ {} …",
            self.name, self.listen
        ));
        match service::install(opts) {
            Ok(()) => {
                self.push_log("安装成功");
                if self.start_after {
                    self.push_log(
                        "服务应已启动；首次启动后查看数据目录 bootstrap_token.txt",
                    );
                }
                self.refresh_status();
                self.show_result("安装完成");
            }
            Err(e) => {
                self.push_log(format!("安装失败: {e}"));
                self.push_log(admin_hint());
                self.show_result("安装失败");
            }
        }
        Ok(())
    }

    fn run_simple(&mut self, action: &str) -> Result<()> {
        let opts = ServiceActionOptions {
            name: self.name.clone(),
            user: false,
        };
        let res = match action {
            "start" => service::start(opts).map(|_| "已启动".to_string()),
            "stop" => service::stop(opts).map(|_| "已停止".to_string()),
            "uninstall" => service::uninstall(opts).map(|_| "已卸载".to_string()),
            _ => bail!("未知动作 {action}"),
        };
        match res {
            Ok(msg) => {
                self.push_log(msg);
                self.refresh_status();
                self.show_result("操作成功");
            }
            Err(e) => {
                self.push_log(format!("{action} 失败: {e}"));
                self.push_log(admin_hint());
                self.show_result("操作失败");
            }
        }
        Ok(())
    }

    fn run_update(&mut self) -> Result<()> {
        let opts = UpdateOptions {
            program: None,
            version: None,
            install_root: nonempty_path(&self.install_root),
            names: Some(vec![self.name.clone()]),
            retain: 3,
            no_start: false,
        };
        match service::update(opts) {
            Ok(()) => {
                self.push_log("更新完成");
                self.refresh_status();
                self.show_result("更新成功");
            }
            Err(e) => {
                self.push_log(format!("更新失败: {e}"));
                self.push_log(admin_hint());
                self.show_result("更新失败");
            }
        }
        Ok(())
    }

    fn run_rollback(&mut self) -> Result<()> {
        let opts = RollbackOptions {
            version: None,
            names: Some(vec![self.name.clone()]),
            no_start: false,
        };
        match service::rollback(opts) {
            Ok(()) => {
                self.push_log("回滚完成");
                self.refresh_status();
                self.show_result("回滚成功");
            }
            Err(e) => {
                self.push_log(format!("回滚失败: {e}"));
                self.push_log(admin_hint());
                self.show_result("回滚失败");
            }
        }
        Ok(())
    }

    fn run_versions(&mut self) -> Result<()> {
        match service::list_versions_report() {
            Ok(report) => {
                for line in report.lines() {
                    self.push_log(line.to_string());
                }
                self.show_result("版本列表");
            }
            Err(e) => {
                self.push_log(format!("读取版本失败: {e}"));
                self.show_result("版本列表失败");
            }
        }
        Ok(())
    }
}

enum Edit {
    Backspace,
    Push(char),
}

fn nonempty_path(s: &str) -> Option<PathBuf> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(PathBuf::from(t))
    }
}

fn nonempty_string(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn validate_install(app: &App) -> Result<()> {
    if app.name.is_empty()
        || !app
            .name
            .chars()
            .enumerate()
            .all(|(i, c)| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' => true,
                '-' | '_' => i > 0,
                _ => false,
            })
    {
        bail!("实例名无效");
    }
    let _: SocketAddr = app.listen.parse().context("监听地址格式错误")?;
    if !app.controller.trim().is_empty() && app.controller_token.trim().is_empty() {
        bail!("已填控制端 URL 时必须填写 Token");
    }
    if app.controller.trim().is_empty() ^ app.controller_token.trim().is_empty() {
        // XOR already handled above partially
    }
    Ok(())
}

fn status_text(st: &ServiceStatus) -> String {
    match st {
        ServiceStatus::NotInstalled => "未安装".into(),
        ServiceStatus::Running => "运行中".into(),
        ServiceStatus::Stopped(reason) => match reason {
            Some(r) => format!("已停止 ({r})"),
            None => "已停止".into(),
        },
    }
}

fn admin_hint() -> String {
    #[cfg(windows)]
    {
        "提示: Windows 下请右键「以管理员身份运行」终端后再试。".into()
    }
    #[cfg(not(windows))]
    {
        "提示: 系统级服务可能需要 sudo。".into()
    }
}
