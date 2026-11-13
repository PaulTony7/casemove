use app_commands::SavedAccount;
use leptos::{ev::SubmitEvent, prelude::*, task::spawn_local};
use leptos_icons::Icon;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

#[derive(Clone, Copy, PartialEq)]
enum LoginMethod {
    Regular,
    Qr,
    WebToken,
}

async fn accounts_command(command: &str, args: JsValue) -> Result<Vec<SavedAccount>, String> {
    let response = invoke(command, args).await.map_err(|error| {
        error
            .as_string()
            .unwrap_or_else(|| "Could not load saved accounts.".into())
    })?;
    serde_wasm_bindgen::from_value(response).map_err(|error| error.to_string())
}

#[derive(Serialize)]
struct RemoveAccountArgs {
    username: String,
}

#[component]
pub fn App() -> impl IntoView {
    let (method, set_method) = signal(LoginMethod::Regular);
    let (username, set_username) = signal(String::new());
    let (show_secret, set_show_secret) = signal(false);
    let (accounts, set_accounts) = signal(Vec::<SavedAccount>::new());
    let (account_error, set_account_error) = signal(String::new());
    let (loading, set_loading) = signal(true);
    let (pending_removal, set_pending_removal) = signal(None::<String>);
    let (removing, set_removing) = signal(false);
    let (mobile_menu, set_mobile_menu) = signal(false);
    let confirmation = NodeRef::<leptos::html::Dialog>::new();

    Effect::new(move |_| {
        if let Some(dialog) = confirmation.get() {
            if pending_removal.get().is_some() {
                if !dialog.open() {
                    let _ = dialog.show_modal();
                }
            } else {
                dialog.close();
            }
        }
    });

    Effect::new(move |_| {
        let is_desktop = js_sys::Reflect::has(&js_sys::global(), &JsValue::from_str("__TAURI__"))
            .unwrap_or(false);
        if !is_desktop {
            set_account_error.set("Desktop connection unavailable.".into());
            set_loading.set(false);
            return;
        }
        spawn_local(async move {
            match accounts_command("store_get_accounts", JsValue::NULL).await {
                Ok(value) => set_accounts.set(value),
                Err(error) => set_account_error.set(error),
            }
            set_loading.set(false);
        });
    });

    let remove_account = move |_| {
        let Some(username) = pending_removal.get_untracked() else {
            return;
        };
        set_removing.set(true);
        spawn_local(async move {
            let result = match serde_wasm_bindgen::to_value(&RemoveAccountArgs { username }) {
                Ok(args) => accounts_command("store_delete_account", args).await,
                Err(error) => Err(error.to_string()),
            };
            match result {
                Ok(value) => {
                    set_accounts.set(value);
                    set_account_error.set(String::new());
                    set_username.set(String::new());
                }
                Err(error) => set_account_error.set(error),
            }
            set_removing.set(false);
            set_pending_removal.set(None);
        });
    };

    view! {
        <div class="app-shell">
            <header class="mobile-header">
                <a class="brand" href="#/signin"><img src="public/casemove.svg" alt=""/>"casemove"</a>
                <button class="icon-button" aria-label="Toggle navigation" title="Toggle navigation"
                    aria-expanded=move || mobile_menu.get().to_string()
                    on:click=move |_| set_mobile_menu.update(|open| *open = !*open)>
                    <Icon icon=icondata::LuMenu/>
                </button>
            </header>
            <aside class="sidebar" class:mobile-open=move || mobile_menu.get()>
                <a class="brand" href="#/signin"><img src="public/casemove.svg" alt=""/>"casemove"</a>
                <div class="session-summary">
                    <div class="avatar"><Icon icon=icondata::LuUserRound/></div>
                    <div><strong>"Steam account"</strong><span class="disconnected">"Not connected"</span></div>
                </div>
                <nav aria-label="Main navigation">
                    <For each=move || vec![
                        ("Overview", icondata::LuChartNoAxesCombined),
                        ("Transfer | From", icondata::LuDownload),
                        ("Transfer | To", icondata::LuUpload),
                        ("Inventory", icondata::LuArchive),
                    ] key=|(name, _)| *name children=move |(name, icon)| view! {
                        <button class="nav-item" disabled title="Sign in to Steam first"><Icon icon=icon/>{name}</button>
                    }/>
                </nav>
                <div class="sidebar-bottom"><Icon icon=icondata::LuShieldCheck/><span>"Casemove"<small>"v0.1.0"</small></span></div>
            </aside>
            <main class="login-layout">
                <section class="login-main" aria-labelledby="login-heading">
                    <div class="login-content">
                        <div class="steam-brand"><Icon icon=icondata::SiSteam/><span>"STEAM"</span></div>
                        <div class="login-tabs" role="tablist" aria-label="Login method">
                            <For each=move || vec![
                                (LoginMethod::Qr, "QR", icondata::LuQrCode),
                                (LoginMethod::WebToken, "Webtoken", icondata::LuWifi),
                                (LoginMethod::Regular, "Regular", icondata::LuLockKeyhole),
                            ] key=|(_, name, _)| *name children=move |(value, name, icon)| view! {
                                <button role="tab" type="button" class:active=move || method.get() == value
                                    aria-selected=move || (method.get() == value).to_string()
                                    aria-controls="login-panel" id=format!("tab-{name}")
                                    on:click=move |_| set_method.set(value)><Icon icon=icon/>{name}</button>
                            }/>
                        </div>
                        <div id="login-panel" role="tabpanel" aria-labelledby=move || match method.get() {
                            LoginMethod::Regular => "tab-Regular",
                            LoginMethod::Qr => "tab-QR",
                            LoginMethod::WebToken => "tab-Webtoken",
                        }>
                            <h1 id="login-heading">{move || match method.get() {
                                LoginMethod::Regular => "Connect to Steam",
                                LoginMethod::Qr => "Scan QR Code",
                                LoginMethod::WebToken => "Connect from browser",
                            }}</h1>
                            <form on:submit=move |event: SubmitEvent| event.prevent_default()>
                                {move || match method.get() {
                                    LoginMethod::Regular => view! {
                                        <div class="credentials">
                                            <label class="sr-only" for="username">"Username"</label>
                                            <input id="username" name="username" placeholder="Username" autocomplete="username"
                                                spellcheck="false" prop:value=move || username.get()
                                                on:input=move |event| set_username.set(event_target_value(&event))/>
                                            <label class="sr-only" for="password">"Password"</label>
                                            <input id="password" name="password" type="password" placeholder="Password" autocomplete="current-password"/>
                                            <label class="sr-only" for="auth-code">"Steam Guard code (optional)"</label>
                                            <input id="auth-code" name="authcode" placeholder="Authcode (optional)" autocomplete="one-time-code" spellcheck="false"/>
                                            <Show when=move || show_secret.get()>
                                                <label class="sr-only" for="shared-secret">"Shared secret"</label>
                                                <input id="shared-secret" type="password" placeholder="Shared secret" autocomplete="off"/>
                                            </Show>
                                        </div>
                                        <div class="form-options">
                                            <label><input type="checkbox"/>"Remember for later"</label>
                                            <label>"Show secret field"<input type="checkbox" prop:checked=move || show_secret.get()
                                                on:change=move |event| set_show_secret.set(event_target_checked(&event))/></label>
                                        </div>
                                    }.into_any(),
                                    LoginMethod::WebToken => view! {
                                        <label class="field-label" for="web-token">"Browser login data"</label>
                                        <textarea id="web-token" rows="4" placeholder="Paste data" spellcheck="false" autocomplete="off"/>
                                    }.into_any(),
                                    LoginMethod::Qr => view! {
                                        <div class="qr-unavailable"><Icon icon=icondata::LuQrCode/><span>"No active QR session"</span></div>
                                        <label class="remember-qr"><input type="checkbox"/>"Remember for later"</label>
                                    }.into_any(),
                                }}
                                <div class="service-status" role="status"><Icon icon=icondata::LuInfo/>
                                    <span>"Steam sign-in is not available in this Rust build yet."</span>
                                </div>
                                <button class="sign-in" type="submit" disabled aria-describedby="sign-in-status">
                                    <Icon icon=icondata::LuLockKeyhole/>"Sign in"
                                </button>
                                <p id="sign-in-status" class="sr-only">"Steam authentication is not implemented."</p>
                            </form>
                        </div>
                    </div>
                </section>
                <aside class="accounts-sidebar" aria-labelledby="accounts-heading">
                    <h2 id="accounts-heading">"Saved accounts"</h2>
                    <Show when=move || loading.get()><p role="status" class="muted">"Loading accounts..."</p></Show>
                    <Show when=move || !account_error.get().is_empty()>
                        <p role="alert" class="account-error">{move || account_error.get()}</p>
                    </Show>
                    <Show when=move || !loading.get() && accounts.get().is_empty()>
                        <div class="empty-account"><div class="avatar"><Icon icon=icondata::LuUserRound/></div>
                            <div><strong>"Nothing here"</strong><span>"Login to add user"</span></div>
                        </div>
                    </Show>
                    <ul class="account-list">
                        <For each=move || accounts.get() key=|account| account.username.clone() children=move |account| {
                            let selected_name = account.username.clone();
                            let removed_name = account.username.clone();
                            view! {
                                <li class="saved-account">
                                    <div class="avatar"><Icon icon=icondata::LuUserRound/></div>
                                    <button class="account-select" title="Select account" on:click=move |_| {
                                        set_username.set(selected_name.clone());
                                        set_method.set(LoginMethod::Regular);
                                    }><strong>{account.display_name}</strong><span>{account.username}</span></button>
                                    <button class="icon-button delete-account" aria-label="Remove saved account" title="Remove saved account"
                                        on:click=move |_| set_pending_removal.set(Some(removed_name.clone()))><Icon icon=icondata::LuTrash2/></button>
                                </li>
                            }
                        }/>
                    </ul>
                </aside>
            </main>
                    <dialog class="confirm-dialog" node_ref=confirmation role="alertdialog" aria-labelledby="remove-heading" aria-describedby="remove-description"
                        on:keydown=move |event: leptos::ev::KeyboardEvent| {
                            if event.key() == "Escape" {
                                event.prevent_default();
                                if !removing.get_untracked() {
                                    set_pending_removal.set(None);
                                }
                            }
                        }
                        on:cancel=move |event: leptos::ev::Event| {
                            event.prevent_default();
                            if !removing.get_untracked() {
                                set_pending_removal.set(None);
                            }
                        }>
                        <h2 id="remove-heading">"Remove saved account?"</h2>
                        <p id="remove-description">{move || pending_removal.get().unwrap_or_default()}</p>
                        <div class="dialog-actions">
                            <button autofocus disabled=move || removing.get() on:click=move |_| set_pending_removal.set(None)>"Cancel"</button>
                            <button class="danger" disabled=move || removing.get() on:click=remove_account>
                                {move || if removing.get() { "Removing..." } else { "Remove account" }}
                            </button>
                        </div>
                    </dialog>
        </div>
    }
}
