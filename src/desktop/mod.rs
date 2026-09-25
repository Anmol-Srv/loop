//! The native macOS app.
//!
//! Feature-gated behind `app` so the server binary does not carry a GUI stack:
//!   cargo build --features app --bin acp-app

pub mod design;
pub mod net;
pub mod views;

// One credential store, shared with the CLI.
pub use crate::cli::creds;
use net::Net;

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    Home,
    MyTasks,
    Triage,
    Projects,
    Agents,
}

pub struct App {
    pub net: Option<Net>,
    pub tab: Tab,
    /// Set when a project is opened; `None` shows the project list.
    pub project: Option<String>,
    /// Set when a task is opened; takes over the central panel.
    pub task: Option<String>,
    pub scopes: Vec<String>,
    pub login: views::login::State,
    pub board: views::board::State,
    pub palette: views::palette::State,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        design::theme::install(&cc.egui_ctx);
        // The type scale is tuned for a comfortable reading size on a Mac's
        // scaled display, where egui's default points come out small. This is
        // one knob instead of nudging every token.
        cc.egui_ctx.set_zoom_factor(1.15);

        let mut app = Self {
            net: None,
            tab: Tab::Home,
            project: None,
            task: None,
            scopes: Vec::new(),
            login: views::login::State::default(),
            board: views::board::State::default(),
            palette: views::palette::State::default(),
        };

        if let Some(token) = creds::load() {
            app.connect(token, &cc.egui_ctx);
        }
        app
    }

    pub fn connect(&mut self, token: String, ctx: &egui::Context) {
        let net = Net::spawn(creds::base_url(), token, ctx.clone());
        self.net = Some(net);
        // `whoami` doubles as the credential check: the server rejects a bad
        // token before we show anything.
        if let Some(n) = self.net.as_mut() {
            n.get("__me", "/api/user/me");
        }
    }

    pub fn can_write(&self) -> bool {
        self.scopes.iter().any(|s| s == "write")
    }

    pub fn sign_out(&mut self) {
        let _ = creds::clear();
        self.net = None;
        self.scopes.clear();
        self.project = None;
        self.task = None;
        // Reset the whole login screen, error text included, so the next sign-in
        // does not open on the last one's failure.
        self.login = views::login::State::default();
    }

    fn absorb_identity(&mut self) {
        let Some(net) = self.net.as_mut() else { return };
        net.get_once("__me", "/api/user/me");

        if let Some(data) = net.data("__me") {
            if self.scopes.is_empty() {
                self.scopes = data
                    .get("scopes")
                    .and_then(|s| s.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
            }
        }

        // Any 401, from whichever view asked: the stored credential is no
        // longer good. Sign out first, since it clears the login screen, and
        // this one message is worth carrying back to it.
        if net.session_ended {
            self.sign_out();
            self.login.error = Some("Your session ended \u{2014} sign in again.".into());
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

impl App {
    /// One frame of the whole app. Public so `tests/page_render.rs` can drive
    /// the real app — real views, real fetches against a real server —
    /// without an `eframe::Frame`, which only a native window can make.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        if let Some(net) = self.net.as_mut() {
            net.pump();
        }

        if self.net.is_none() {
            views::login::ui(self, ui);
            return;
        }

        self.absorb_identity();
        if self.net.is_none() {
            views::login::ui(self, ui);
            return;
        }

        views::chrome::ui(self, ui);
        if let Some(net) = self.net.as_mut() {
            net.tick(ui.ctx());
        }
    }
}
