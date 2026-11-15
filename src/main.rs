mod app;
mod export;
mod icons;
mod images;
mod inventory;
mod inventory_view;
mod login;
mod messages;
mod overview_view;
mod settings_view;
mod storage_view;
mod theme;
mod tradeup_view;

use app_core::{AppEvent, Core, EventSink};
use eframe::egui;
use messages::Message;

fn main() -> eframe::Result {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the async runtime");

    let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let (evt_tx, events_rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Casemove")
        .with_inner_size([1124.0, 850.0])
        .with_min_inner_size([960.0, 620.0]);
    if let Some(icon) = icons::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Casemove",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let repaint = std::sync::Arc::new(move || ctx.request_repaint());
            let events = EventSink::new(evt_tx, repaint);
            let core = Core::build(events).map_err(std::io::Error::other)?;
            let app = app::CasemoveApp::new(cc, core, rt, msg_tx, msg_rx, events_rx);
            Ok(Box::new(app))
        }),
    )
}
