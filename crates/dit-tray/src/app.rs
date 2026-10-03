//! The menu bar app's event loop (macOS). One icon, one menu; the menu is
//! rebuilt whenever what it shows changes — server state, the workspace
//! list, Open at Login — which is simpler than keeping a dozen items in
//! step and costs nothing at this size.

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::git;
use crate::icon;
use crate::server::{Server, ServerState};

/// The port `dit ui` uses by default, so a bookmark works either way.
/// `DIT_TRAY_PORT` picks another when something else holds it.
const PORT: u16 = 7700;
/// How long Stop and Quit wait for requests in flight before killing.
const PATIENCE: Duration = Duration::from_secs(8);
/// How often the menu looks at the server and the workspace list.
const TICK: Duration = Duration::from_secs(1);
/// The icon's size in pixels: 18 points at 2×.
const ICON_SIZE: u32 = 36;

#[derive(Debug)]
enum UserEvent {
    Menu(MenuEvent),
}

/// Everything the menu shows; when it changes, the menu is rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct View {
    git: bool,
    server: ServerState,
    workspaces: Vec<String>,
    at_login: bool,
}

struct App {
    dit: PathBuf,
    port: u16,
    server: Option<Server>,
    /// The last state of a server that is no longer running.
    ended: ServerState,
    git: bool,
    tray: Option<TrayIcon>,
    shown: Option<View>,
}

pub fn run() -> ExitCode {
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    // No Dock icon and no app menu: the menu bar icon is the whole app.
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Menu(event));
    }));

    let mut app = App {
        dit: dit_binary(),
        port: std::env::var("DIT_TRAY_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(PORT),
        server: None,
        ended: ServerState::Stopped,
        git: false,
        tray: None,
        shown: None,
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        match event {
            Event::NewEvents(StartCause::Init) => {
                app.git = git_present();
                if app.git {
                    app.start();
                }
                app.tray = build_tray();
                app.refresh();
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if !app.git {
                    app.git = git_present();
                    if app.git && app.server.is_none() {
                        app.start();
                    }
                }
                app.refresh();
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                if app.on_menu(event.id.as_ref()) {
                    app.stop();
                    app.tray = None;
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                app.refresh();
            }
            Event::LoopDestroyed => app.stop(),
            _ => {}
        }
    })
}

impl App {
    fn start(&mut self) {
        if self.server.is_some() {
            return;
        }
        match Server::start(&self.dit, self.port) {
            Ok(server) => self.server = Some(server),
            Err(e) => {
                self.ended =
                    ServerState::Failed(format!("cannot start {}: {e}", self.dit.display()))
            }
        }
    }

    fn stop(&mut self) {
        if let Some(server) = self.server.take() {
            self.ended = server.stop(PATIENCE);
        }
    }

    fn state(&mut self) -> ServerState {
        let Some(server) = self.server.as_mut() else {
            return self.ended.clone();
        };
        let state = server.state();
        if matches!(state, ServerState::Stopped | ServerState::Failed(_)) {
            // It ended on its own: remember why, and let Start try again.
            self.server = None;
            self.ended = state.clone();
        }
        state
    }

    fn url(&mut self) -> Option<String> {
        match self.state() {
            ServerState::Running { url } => Some(url),
            _ => None,
        }
    }

    /// Act on a menu item; true when the app should quit.
    fn on_menu(&mut self, id: &str) -> bool {
        match id {
            "open" => {
                if let Some(url) = self.url() {
                    open(&url);
                }
            }
            "start" => self.start(),
            "stop" => self.stop(),
            "install-git" => git::install_command_line_tools(),
            "at-login" => {
                if let Some(item) = dit_core::LoginItem::for_this_user() {
                    let result = if item.is_enabled() {
                        item.disable()
                    } else {
                        std::env::current_exe()
                            .map_err(dit_core::DitError::from)
                            .and_then(|exe| item.enable(&exe))
                    };
                    if let Err(e) = result {
                        log_line(&format!("Open at Login: {e}"));
                    }
                }
            }
            "quit" => return true,
            other => {
                if let (Some(name), Some(url)) = (other.strip_prefix("ws:"), self.url()) {
                    open(&workspace_url(&url, name));
                }
            }
        }
        false
    }

    fn view(&mut self) -> View {
        View {
            git: self.git,
            server: self.state(),
            workspaces: workspaces(),
            at_login: dit_core::LoginItem::for_this_user().is_some_and(|i| i.is_enabled()),
        }
    }

    /// Redraw the icon and the menu if what they show has changed.
    fn refresh(&mut self) {
        let view = self.view();
        if self.shown.as_ref() == Some(&view) {
            return;
        }
        if self.shown.as_ref().map(|v| &v.server) != Some(&view.server) {
            log_line(&status_line(&view));
        }
        if let Some(tray) = &self.tray {
            let running = matches!(view.server, ServerState::Running { .. });
            if let Ok(icon) = Icon::from_rgba(
                icon::mark(ICON_SIZE, if running { 1.0 } else { 0.4 }),
                ICON_SIZE,
                ICON_SIZE,
            ) {
                let _ = tray.set_icon_with_as_template(Some(icon), true);
            }
            let _ = tray.set_tooltip(Some(status_line(&view)));
            tray.set_menu(Some(Box::new(build_menu(&view))));
        }
        self.shown = Some(view);
    }
}

fn build_tray() -> Option<TrayIcon> {
    let icon = Icon::from_rgba(icon::mark(ICON_SIZE, 0.4), ICON_SIZE, ICON_SIZE).ok()?;
    TrayIconBuilder::new()
        .with_icon(icon)
        .with_icon_as_template(true)
        .with_tooltip("DIT")
        .build()
        .ok()
}

fn status_line(view: &View) -> String {
    if !view.git {
        return "DIT needs git to run".to_owned();
    }
    match &view.server {
        ServerState::Starting => "DIT is starting…".to_owned(),
        ServerState::Running { url } => {
            let base = url.split('#').next().unwrap_or(url);
            format!("DIT is running at {}", base.trim_end_matches('/'))
        }
        ServerState::Stopped => "DIT is stopped".to_owned(),
        ServerState::Failed(why) => format!("DIT stopped: {why}"),
    }
}

fn build_menu(view: &View) -> Menu {
    let menu = Menu::new();
    let running = matches!(view.server, ServerState::Running { .. });
    let starting = matches!(view.server, ServerState::Starting);
    let mut status = status_line(view);
    if status.chars().count() > 70 {
        status = status.chars().take(69).collect::<String>() + "…";
    }
    let _ = menu.append(&MenuItem::with_id("status", status, false, None));
    let _ = menu.append(&PredefinedMenuItem::separator());
    if !view.git {
        let _ = menu.append(&MenuItem::with_id(
            "install-git",
            "Install Git…",
            true,
            None,
        ));
        let _ = menu.append(&MenuItem::with_id(
            "git-hint",
            "DIT starts by itself once git is installed",
            false,
            None,
        ));
    } else {
        let _ = menu.append(&MenuItem::with_id("open", "Open DIT", running, None));
        let workspaces = Submenu::with_id(
            "workspaces",
            "Workspaces",
            running && !view.workspaces.is_empty(),
        );
        for name in &view.workspaces {
            let _ = workspaces.append(&MenuItem::with_id(format!("ws:{name}"), name, true, None));
        }
        let _ = menu.append(&workspaces);
        let _ = menu.append(&PredefinedMenuItem::separator());
        if running || starting {
            let _ = menu.append(&MenuItem::with_id("stop", "Stop DIT", true, None));
        } else {
            let _ = menu.append(&MenuItem::with_id("start", "Start DIT", true, None));
        }
    }
    let _ = menu.append(&CheckMenuItem::with_id(
        "at-login",
        "Open at Login",
        true,
        view.at_login,
        None,
    ));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("quit", "Quit DIT", true, None));
    menu
}

/// `http://127.0.0.1:7700/#token=T` → `http://127.0.0.1:7700/w/<name>/#token=T`.
fn workspace_url(url: &str, name: &str) -> String {
    let (base, fragment) = url.split_once('#').unwrap_or((url, ""));
    let base = base.trim_end_matches('/');
    if fragment.is_empty() {
        format!("{base}/w/{name}/")
    } else {
        format!("{base}/w/{name}/#{fragment}")
    }
}

fn workspaces() -> Vec<String> {
    dit_core::config_dir()
        .and_then(|dir| dit_core::Registry::load(&dir).ok())
        .map(|r| r.workspaces().iter().map(|w| w.name.clone()).collect())
        .unwrap_or_default()
}

fn git_present() -> bool {
    git::git_present(
        std::env::var_os("PATH").as_deref(),
        git::command_line_tools_installed,
    )
}

/// The `dit` beside this binary — `DIT.app/Contents/MacOS/dit` — or, in a
/// development build, the one on `PATH`.
fn dit_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("dit")))
        .filter(|dit| dit.is_file())
        .unwrap_or_else(|| PathBuf::from("dit"))
}

fn open(url: &str) {
    let _ = Command::new("/usr/bin/open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// The app has no console: what it would tell someone debugging it — each
/// change of server state, a failure it cannot show in the menu — goes to
/// stderr, which launchd keeps. Never the token: `status_line` drops it.
#[allow(clippy::print_stderr)]
fn log_line(message: &str) {
    eprintln!("dit-tray: {message}");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_opens_at_its_own_address_with_the_token() {
        assert_eq!(
            workspace_url("http://127.0.0.1:7700/#token=abc", "acme"),
            "http://127.0.0.1:7700/w/acme/#token=abc"
        );
        assert_eq!(
            workspace_url("http://127.0.0.1:7700/", "acme"),
            "http://127.0.0.1:7700/w/acme/"
        );
    }

    #[test]
    fn the_status_line_never_shows_the_token() {
        let view = View {
            git: true,
            server: ServerState::Running {
                url: "http://127.0.0.1:7700/#token=secret".into(),
            },
            workspaces: vec![],
            at_login: false,
        };
        let line = status_line(&view);
        assert_eq!(line, "DIT is running at http://127.0.0.1:7700");
        assert!(!line.contains("secret"));
    }
}
