use crate::app::{CasemoveApp, LoginMethod, profile_display_name, wallet_balance};
use crate::images;
use crate::theme;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    egui::ScrollArea::vertical()
        .id_salt("login-scroll")
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(96.0);
                ui.set_max_width(384.0);
                ui.label(egui::RichText::new("STEAM").size(30.0).strong());
                ui.add_space(24.0);
                if app.profile.is_some() {
                    session_details(ui, app);
                } else {
                    login_form(ui, app);
                }
                ui.add_space(64.0);
            });
        });
}

fn session_details(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    ui.add_space(32.0);
    let avatar_url = app
        .profile
        .as_ref()
        .and_then(|profile| profile.avatar_url.clone());
    images::avatar(ui, &mut app.images, avatar_url.as_deref(), 40.0);
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(profile_display_name(app.profile.clone()))
            .strong()
            .size(15.0),
    );
    if let Some(steam_id) = app.profile.as_ref().map(|profile| profile.steam_id.clone()) {
        ui.label(
            egui::RichText::new(steam_id)
                .monospace()
                .size(12.0)
                .color(theme::MUTED),
        );
    }
    let balance = wallet_balance(app.wallet.clone());
    if !balance.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(balance).size(13.0));
    }
    if !app.login_error.is_empty() {
        ui.label(
            egui::RichText::new(&app.login_error)
                .size(12.0)
                .color(theme::ERROR),
        );
    }
    ui.add_space(24.0);
    let width = ui.available_width();
    let open_inventory = egui::Button::new(
        egui::RichText::new(format!("{}  Inventory", egui_phosphor::regular::ARCHIVE))
            .color(egui::Color32::WHITE),
    )
    .fill(theme::ACCENT)
    .min_size(egui::vec2(width, 40.0));
    if ui.add_enabled(!app.session_busy, open_inventory).clicked() {
        app.inventory_open = true;
    }
    ui.add_space(12.0);
    let sign_out = egui::Button::new(format!(
        "{}  {}",
        egui_phosphor::regular::SIGN_OUT,
        if app.session_busy {
            "Signing out..."
        } else {
            "Sign out"
        }
    ))
    .min_size(egui::vec2(width, 38.0));
    if ui.add_enabled(!app.session_busy, sign_out).clicked() {
        app.sign_out();
    }
}

fn login_form(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut switch = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let tabs = [
            (LoginMethod::Qr, "QR", egui_phosphor::regular::QR_CODE),
            (
                LoginMethod::WebToken,
                "Webtoken",
                egui_phosphor::regular::WIFI_HIGH,
            ),
            (
                LoginMethod::Regular,
                "Regular",
                egui_phosphor::regular::LOCK_KEY,
            ),
        ];
        for (method, name, glyph) in tabs {
            let selected = app.method == method;
            let enabled = !app.login_busy && !app.session_busy;
            let label = format!("{glyph}  {name}");
            if ui
                .add_enabled(enabled, egui::Button::selectable(selected, label))
                .clicked()
            {
                switch = Some(method);
            }
        }
    });
    if let Some(method) = switch {
        app.switch_method(method);
    }

    ui.add_space(20.0);
    let heading = match app.method {
        LoginMethod::Regular => "Connect to Steam",
        LoginMethod::Qr => "Scan QR Code",
        LoginMethod::WebToken => "Connect from browser",
    };
    ui.label(egui::RichText::new(heading).size(28.0).strong());
    ui.add_space(22.0);

    let busy = app.login_busy || app.session_busy;
    let mut submit = false;
    match app.method {
        LoginMethod::Regular => regular_fields(ui, app, busy, &mut submit),
        LoginMethod::WebToken => web_token_fields(ui, app, busy),
        LoginMethod::Qr => qr_fields(ui, app, busy),
    }
    if submit {
        app.submit_login();
    }

    ui.add_space(22.0);
    status_line(ui, app);
    if !app.login_error.is_empty() {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(&app.login_error)
                .size(12.0)
                .color(theme::ERROR),
        );
    }
    ui.add_space(16.0);

    let width = ui.available_width();
    let text = if app.login_busy {
        "Connecting..."
    } else if app.method == LoginMethod::Qr {
        "Start QR login"
    } else {
        "Sign in"
    };
    let sign_in = egui::Button::new(
        egui::RichText::new(format!("{}  {text}", egui_phosphor::regular::SIGN_IN))
            .color(egui::Color32::WHITE),
    )
    .fill(theme::ACCENT)
    .min_size(egui::vec2(width, 40.0));
    let can_submit = !app.loading && !app.login_busy && !app.session_busy;
    if ui.add_enabled(can_submit, sign_in).clicked() {
        app.submit_login();
    }

    if app.login_busy {
        ui.add_space(12.0);
        let cancel = egui::Button::new("Cancel login").min_size(egui::vec2(width, 38.0));
        if ui.add_enabled(!app.session_busy, cancel).clicked() {
            app.sign_out();
        }
    }
}

fn regular_fields(ui: &mut egui::Ui, app: &mut CasemoveApp, busy: bool, submit: &mut bool) {
    ui.add_enabled_ui(!busy, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let username = ui.add(
            egui::TextEdit::singleline(&mut app.form.username)
                .hint_text("Username")
                .desired_width(f32::INFINITY),
        );
        if username.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            *submit = true;
        }
        let password = ui.add(
            egui::TextEdit::singleline(&mut app.form.password)
                .hint_text("Password")
                .password(true)
                .desired_width(f32::INFINITY),
        );
        if password.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            *submit = true;
        }
        ui.add(
            egui::TextEdit::singleline(&mut app.form.guard_code)
                .hint_text("Authcode (optional)")
                .desired_width(f32::INFINITY),
        );
        if app.form.show_secret {
            ui.add(
                egui::TextEdit::singleline(&mut app.form.shared_secret)
                    .hint_text("Base64 shared secret")
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut app.form.remember, "Remember for later");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .checkbox(&mut app.form.show_secret, "Show secret field")
                    .changed()
                {
                    app.form.guard_code.clear();
                    app.form.shared_secret.clear();
                }
            });
        });
    });
}

fn web_token_fields(ui: &mut egui::Ui, app: &mut CasemoveApp, busy: bool) {
    ui.label("Browser login data");
    ui.add_enabled_ui(!busy, |ui| {
        ui.add(
            egui::TextEdit::multiline(&mut app.form.browser_token)
                .hint_text("Paste the Steam browser login JSON")
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
    });
}

fn qr_fields(ui: &mut egui::Ui, app: &mut CasemoveApp, busy: bool) {
    let challenge = app.qr_challenge.clone();
    let texture = match &challenge {
        Some(challenge) => {
            if app
                .qr_texture
                .as_ref()
                .is_none_or(|(key, _)| key != challenge)
            {
                match qr_texture(ui.ctx(), challenge) {
                    Ok(texture) => {
                        app.qr_error.clear();
                        app.qr_texture = Some((challenge.clone(), texture));
                    }
                    Err(error) => {
                        app.qr_texture = None;
                        app.qr_error = error;
                    }
                }
            }
            app.qr_texture.as_ref().map(|(_, texture)| texture.clone())
        }
        None => {
            app.qr_texture = None;
            None
        }
    };

    if let Some(texture) = texture {
        egui::Frame::NONE
            .fill(egui::Color32::WHITE)
            .corner_radius(6)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.add(
                    egui::Image::new((texture.id(), egui::vec2(219.0, 219.0)))
                        .fit_to_exact_size(egui::vec2(219.0, 219.0)),
                );
            });
    } else {
        egui::Frame::NONE
            .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(203.0, 203.0));
                ui.add_space(44.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(egui_phosphor::regular::QR_CODE)
                            .size(48.0)
                            .color(theme::MUTED),
                    );
                    ui.add_space(16.0);
                    let text = if busy {
                        "Waiting for Steam QR code..."
                    } else {
                        "Start a QR login to display a code"
                    };
                    ui.label(egui::RichText::new(text).size(13.0).color(theme::MUTED));
                });
            });
    }
    if !app.qr_error.is_empty() {
        ui.label(
            egui::RichText::new(&app.qr_error)
                .size(12.0)
                .color(theme::ERROR),
        );
    }
    ui.add_space(12.0);
    ui.add_enabled_ui(!busy, |ui| {
        ui.checkbox(&mut app.form.qr_remember, "Remember for later");
    });
}

fn status_line(ui: &mut egui::Ui, app: &CasemoveApp) {
    let status = if app.session_busy {
        "Cancelling Steam login..."
    } else if app.login_busy {
        if app.method == LoginMethod::Qr {
            "Scan the QR code with the Steam mobile app."
        } else {
            "Connecting to Steam. Approve in the Steam mobile app if prompted."
        }
    } else if app.method != LoginMethod::Regular {
        "Use your Steam mobile app or a signed-in Steam browser session."
    } else {
        "Steam Guard code or Steam mobile approval may be required."
    };
    ui.horizontal_top(|ui| {
        ui.label(
            egui::RichText::new(egui_phosphor::regular::INFO)
                .size(16.0)
                .color(theme::STATUS_TEXT),
        );
        ui.label(
            egui::RichText::new(status)
                .size(12.0)
                .color(theme::STATUS_TEXT),
        );
    });
}

fn qr_texture(ctx: &egui::Context, challenge: &str) -> Result<egui::TextureHandle, String> {
    let code = qrcode::QrCode::new(challenge).map_err(|error| error.to_string())?;
    let width = code.width();
    let colors = code.to_colors();
    let quiet = 4usize;
    let scale = 8usize;
    let side = (width + quiet * 2) * scale;
    let mut pixels = vec![egui::Color32::WHITE; side * side];
    for (index, module) in colors.iter().enumerate() {
        if !matches!(module, qrcode::Color::Dark) {
            continue;
        }
        let x0 = (index % width + quiet) * scale;
        let y0 = (index / width + quiet) * scale;
        for dy in 0..scale {
            for dx in 0..scale {
                pixels[(y0 + dy) * side + (x0 + dx)] = egui::Color32::BLACK;
            }
        }
    }
    Ok(ctx.load_texture(
        "steam-login-qr",
        egui::ColorImage::new([side, side], pixels),
        egui::TextureOptions::NEAREST,
    ))
}
