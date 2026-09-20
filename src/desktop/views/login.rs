use crate::desktop::{creds, theme, App};

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.28);
        ui.heading("Airtribe Control Plane");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(creds::base_url())
                .monospace()
                .size(11.0)
                .color(theme::MUTED),
        );
        ui.add_space(20.0);

        ui.label(
            egui::RichText::new("Paste a token to sign in")
                .size(12.0)
                .color(theme::MUTED),
        );
        ui.add_space(6.0);

        let field = ui.add(
            egui::TextEdit::singleline(&mut app.token_input)
                .password(true)
                .desired_width(340.0)
                .hint_text("acp token"),
        );

        let submitted =
            field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.add_space(10.0);
        let clicked = ui.add_enabled(!app.token_input.trim().is_empty(), egui::Button::new("Sign in")).clicked();

        if submitted || clicked {
            let token = app.token_input.trim().to_string();
            match creds::store(&token) {
                Ok(()) => {
                    app.login_error = None;
                    let ctx = ui.ctx().clone();
                    app.connect(token, &ctx);
                }
                Err(e) => app.login_error = Some(e),
            }
        }

        if let Some(err) = &app.login_error {
            ui.add_space(12.0);
            ui.colored_label(theme::DANGER, err);
        }

        ui.add_space(24.0);
        ui.label(
            egui::RichText::new("acp-admin mint laptop --owner you@airtribe.live --scopes read,write")
                .monospace()
                .size(10.0)
                .color(theme::MUTED),
        );
    });
}
