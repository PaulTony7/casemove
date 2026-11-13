use crate::inventory::InventoryView;
use app_commands::{
    InventorySnapshot, PasswordLoginParams, REFRESH_INVENTORY, STEAM_LOGIN, STEAM_LOGIN_SAVED, STEAM_SESSION,
    STEAM_SIGN_OUT, SavedAccount, SteamProfile,
};
use leptos::{ev::SubmitEvent, prelude::*, task::spawn_local};
use leptos_icons::Icon;
use serde::{Serialize, de::DeserializeOwned};
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

async fn desktop_command<Reply: DeserializeOwned>(
    command: &str,
    args: JsValue,
) -> Result<Reply, String> {
    let response = invoke(command, args).await.map_err(|error| {
        error
            .as_string()
            .unwrap_or_else(|| "The desktop request failed.".into())
    })?;
    serde_wasm_bindgen::from_value(response).map_err(|error| error.to_string())
}

#[derive(Serialize)]
struct RemoveAccountArgs {
    username: String,
}

#[derive(Serialize)]
struct LoginArgs<'a> {
    params: &'a PasswordLoginParams,
}

#[derive(Serialize)]
struct SavedLoginArgs {
    account: String,
}

#[component]
pub fn App() -> impl IntoView {
    let (method, set_method) = signal(LoginMethod::Regular);
    let (username, set_username) = signal(String::new());
    let (remember, set_remember) = signal(false);
    let (show_secret, set_show_secret) = signal(false);
    let (desktop, set_desktop) = signal(false);
    let (profile, set_profile) = signal(None::<SteamProfile>);
    let (login_error, set_login_error) = signal(String::new());
    let (login_busy, set_login_busy) = signal(false);
    let (session_busy, set_session_busy) = signal(false);
    let (inventory_open, set_inventory_open) = signal(false);
    let (inventory, set_inventory) = signal(None::<InventorySnapshot>);
    let (inventory_busy, set_inventory_busy) = signal(false);
    let (inventory_error, set_inventory_error) = signal(String::new());
    let attempt = RwSignal::new(0u64);
    let password_input = NodeRef::<leptos::html::Input>::new();
    let guard_input = NodeRef::<leptos::html::Input>::new();
    let secret_input = NodeRef::<leptos::html::Input>::new();
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
        set_desktop.set(true);
        spawn_local(async move {
            match desktop_command("store_get_accounts", JsValue::NULL).await {
                Ok(value) => set_accounts.set(value),
                Err(error) => set_account_error.set(error),
            }
            match desktop_command(STEAM_SESSION, JsValue::NULL).await {
                Ok(value) => set_profile.set(value),
                Err(error) => set_login_error.set(error),
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
                Ok(args) => desktop_command("store_delete_account", args).await,
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

    let complete_login = move |value: SteamProfile| {
        set_login_error.set(value.notice.clone().unwrap_or_default());
        set_profile.set(Some(value));
        spawn_local(async move {
            match desktop_command("store_get_accounts", JsValue::NULL).await {
                Ok(accounts) => set_accounts.set(accounts),
                Err(error) => set_account_error.set(error),
            }
        });
    };

    let login_saved = Callback::new(move |account: String| {
        if !desktop.get_untracked() || login_busy.get_untracked() || session_busy.get_untracked()
            || profile.get_untracked().is_some() { return; }
        set_username.set(account.clone());
        set_method.set(LoginMethod::Regular);
        set_login_error.set(String::new());
        set_login_busy.set(true);
        if let Some(field) = password_input.get_untracked() { field.set_value(""); }
        if let Some(field) = guard_input.get_untracked() { field.set_value(""); }
        if let Some(field) = secret_input.get_untracked() { field.set_value(""); }
        set_show_secret.set(false);
        attempt.update(|value| *value += 1);
        let current_attempt = attempt.get_untracked();
        spawn_local(async move {
            let result = match serde_wasm_bindgen::to_value(&SavedLoginArgs { account }) {
                Ok(args) => desktop_command(STEAM_LOGIN_SAVED, args).await,
                Err(_) => Err("Could not prepare the saved-login request.".into()),
            };
            if attempt.get_untracked() != current_attempt { return; }
            match result {
                Ok(value) => complete_login(value),
                Err(error) => set_login_error.set(error),
            }
            set_login_busy.set(false);
        });
    });

    let login = move |event: SubmitEvent| {
        event.prevent_default();
        if !desktop.get_untracked()
            || login_busy.get_untracked()
            || session_busy.get_untracked()
            || method.get_untracked() != LoginMethod::Regular
            || profile.get_untracked().is_some()
        {
            return;
        }
        let Some(password_field) = password_input.get_untracked() else {
            return;
        };
        let password = password_field.value();
        password_field.set_value("");
        let guard_code = guard_input.get_untracked().and_then(|field| {
            let code = field.value();
            field.set_value("");
            (!code.trim().is_empty()).then_some(code)
        });
        let shared_secret = if show_secret.get_untracked() {
            secret_input.get_untracked().map(|field| {
                let secret = field.value();
                field.set_value("");
                secret
            })
        } else { None };
        let mut params = PasswordLoginParams {
                account: username.get_untracked().trim().to_owned(),
                password,
                guard_code,
                shared_secret,
                remember: remember.get_untracked(),
        };
        let prepared = params.encode_and_clear(|params| serde_wasm_bindgen::to_value(&LoginArgs { params })
            .map_err(|_| "Could not prepare the Steam login request.".to_owned()));
        drop(params);
        set_login_error.set(String::new());
        set_login_busy.set(true);
        attempt.update(|value| *value += 1);
        let current_attempt = attempt.get_untracked();
        spawn_local(async move {
            let result = match prepared {
                Ok(args) => desktop_command(STEAM_LOGIN, args).await,
                Err(error) => Err(error),
            };
            if attempt.get_untracked() != current_attempt {
                return;
            }
            match result {
                Ok(value) => complete_login(value),
                Err(error) => set_login_error.set(error),
            }
            set_login_busy.set(false);
        });
    };

    let sign_out = Callback::new(move |()| {
        if session_busy.get_untracked() {
            return;
        }
        attempt.update(|value| *value += 1);
        if let Some(field) = secret_input.get_untracked() { field.set_value(""); }
        set_show_secret.set(false);
        set_inventory.set(None);
        set_inventory_open.set(false);
        set_inventory_error.set(String::new());
        set_inventory_busy.set(false);
        set_session_busy.set(true);
        spawn_local(async move {
            match desktop_command::<()>(STEAM_SIGN_OUT, JsValue::NULL).await {
                Ok(()) => {
                    set_profile.set(None);
                    set_login_error.set(String::new());
                }
                Err(error) => set_login_error.set(error),
            }
            set_login_busy.set(false);
            set_session_busy.set(false);
        });
    });

    let refresh_inventory = Callback::new(move |()| {
        if inventory_busy.get_untracked()
            || session_busy.get_untracked()
            || profile.get_untracked().is_none()
        {
            return;
        }
        let current_attempt = attempt.get_untracked();
        set_inventory_busy.set(true);
        set_inventory_error.set(String::new());
        spawn_local(async move {
            let result =
                desktop_command::<InventorySnapshot>(REFRESH_INVENTORY, JsValue::NULL).await;
            if attempt.get_untracked() != current_attempt {
                return;
            }
            match result {
                Ok(snapshot) => {
                    if profile
                        .get_untracked()
                        .is_some_and(|profile| profile.steam_id == snapshot.steam_id)
                    {
                        set_inventory.set(Some(snapshot));
                    } else {
                        set_inventory_error
                            .set("The Steam session changed. Load inventory again.".into());
                    }
                }
                Err(error) => set_inventory_error.set(error),
            }
            set_inventory_busy.set(false);
        });
    });

    view! {
        <div class="app-shell">
            <header class="mobile-header">
                <a class="brand" href="#/signin" on:click=move |_| { set_inventory_open.set(false); set_mobile_menu.set(false); }><img src="public/casemove.svg" alt=""/>"casemove"</a>
                <button class="icon-button" aria-label="Toggle navigation" title="Toggle navigation"
                    aria-expanded=move || mobile_menu.get().to_string()
                    on:click=move |_| set_mobile_menu.update(|open| *open = !*open)>
                    <Icon icon=icondata::LuMenu/>
                </button>
            </header>
            <aside class="sidebar" class:mobile-open=move || mobile_menu.get()>
                <a class="brand" href="#/signin" on:click=move |_| { set_inventory_open.set(false); set_mobile_menu.set(false); }><img src="public/casemove.svg" alt=""/>"casemove"</a>
                <div class="session-summary">
                    <div class="avatar"><Icon icon=icondata::LuUserRound/></div>
                    <div><strong>{move || profile.get().map(|value| value.account).unwrap_or_else(|| "Steam account".into())}</strong>
                        <span class:disconnected=move || profile.get().is_none() class:connected=move || profile.get().is_some()>
                            {move || if profile.get().is_some() { "Signed in to Steam" } else { "Not connected" }}
                        </span>
                    </div>
                </div>
                <nav aria-label="Main navigation">
                    <For each=move || vec![
                        ("Overview", icondata::LuChartNoAxesCombined),
                        ("Transfer | From", icondata::LuDownload),
                        ("Transfer | To", icondata::LuUpload),
                        ("Inventory", icondata::LuArchive),
                    ] key=|(name, _)| *name children=move |(name, icon)| view! {
                        <button class="nav-item" class:nav-current=move || name == "Inventory" && inventory_open.get()
                            disabled=move || name != "Inventory" || profile.get().is_none() || session_busy.get()
                            title=move || if profile.get().is_none() { "Sign in to Steam first" } else if name == "Inventory" { "Inventory" } else { "This page has not been ported yet" }
                            on:click=move |_| { set_inventory_open.set(true); set_mobile_menu.set(false); }><Icon icon=icon/>{name}</button>
                    }/>
                </nav>
                <div class="sidebar-bottom"><Icon icon=icondata::LuShieldCheck/><span>"Casemove"<small>"v0.1.0"</small></span></div>
            </aside>
            <Show when=move || !inventory_open.get()>
            <main class="login-layout">
                <section class="login-main" aria-labelledby="login-heading">
                    <div class="login-content">
                        <div class="steam-brand"><Icon icon=icondata::SiSteam/><span>"STEAM"</span></div>
                        <Show when=move || profile.get().is_none() fallback=move || view! {
                            <div class="session-details">
                                <h1 id="login-heading">"Steam session established"</h1>
                                <strong>{move || profile.get().map(|value| value.account).unwrap_or_default()}</strong>
                                <p class="steam-id">{move || profile.get().map(|value| value.steam_id).unwrap_or_default()}</p>
                                <button class="sign-in open-inventory" disabled=move || session_busy.get() on:click=move |_| set_inventory_open.set(true)>
                                    <Icon icon=icondata::LuArchive/>"Inventory"
                                </button>
                                <Show when=move || !login_error.get().is_empty()><p role="alert" class="account-error">{move || login_error.get()}</p></Show>
                                <button class="sign-in" disabled=move || session_busy.get() on:click=move |_| sign_out.run(())>
                                    <Icon icon=icondata::LuLogOut/>{move || if session_busy.get() { "Signing out..." } else { "Sign out" }}
                                </button>
                            </div>
                        }>
                        <div class="login-tabs" role="tablist" aria-label="Login method">
                            <For each=move || vec![
                                (LoginMethod::Qr, "QR", icondata::LuQrCode),
                                (LoginMethod::WebToken, "Webtoken", icondata::LuWifi),
                                (LoginMethod::Regular, "Regular", icondata::LuLockKeyhole),
                            ] key=|(_, name, _)| *name children=move |(value, name, icon)| view! {
                                <button role="tab" type="button" class:active=move || method.get() == value
                                    disabled=move || login_busy.get() || session_busy.get()
                                    aria-selected=move || (method.get() == value).to_string()
                                    aria-controls="login-panel" id=format!("tab-{name}")
                                    on:click=move |_| {
                                        if let Some(field) = password_input.get_untracked() { field.set_value(""); }
                                        if let Some(field) = guard_input.get_untracked() { field.set_value(""); }
                                        if let Some(field) = secret_input.get_untracked() { field.set_value(""); }
                                        set_show_secret.set(false);
                                        set_login_error.set(String::new());
                                        set_method.set(value);
                                    }><Icon icon=icon/>{name}</button>
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
                            <form on:submit=login aria-busy=move || login_busy.get().to_string()>
                                {move || match method.get() {
                                    LoginMethod::Regular => view! {
                                        <div class="credentials">
                                            <label class="sr-only" for="username">"Username"</label>
                                            <input id="username" name="username" placeholder="Username" autocomplete="username"
                                                required disabled=move || login_busy.get() || session_busy.get()
                                                spellcheck="false" prop:value=move || username.get()
                                                on:input=move |event| set_username.set(event_target_value(&event))/>
                                            <label class="sr-only" for="password">"Password"</label>
                                            <input id="password" node_ref=password_input name="password" type="password" placeholder="Password" autocomplete="current-password"
                                                required disabled=move || login_busy.get() || session_busy.get()/>
                                            <label class="sr-only" for="auth-code">"Steam Guard code (optional)"</label>
                                            <input id="auth-code" node_ref=guard_input name="authcode" placeholder="Authcode (optional)" autocomplete="one-time-code" spellcheck="false"
                                                disabled=move || login_busy.get() || session_busy.get() || show_secret.get()/>
                                            <Show when=move || show_secret.get()>
                                                <label class="sr-only" for="shared-secret">"Steam shared secret"</label>
                                                <input id="shared-secret" node_ref=secret_input type="password" placeholder="Base64 shared secret" autocomplete="off"
                                                    spellcheck="false" maxlength="64" required disabled=move || login_busy.get() || session_busy.get()/>
                                            </Show>
                                        </div>
                                        <div class="form-options">
                                            <label><input type="checkbox" prop:checked=move || remember.get()
                                                disabled=move || login_busy.get() || session_busy.get()
                                                on:change=move |event| set_remember.set(event_target_checked(&event))/>"Remember for later"</label>
                                            <label>"Show secret field"<input type="checkbox" prop:checked=move || show_secret.get()
                                                disabled=move || login_busy.get() || session_busy.get()
                                                on:change=move |event| {
                                                    if let Some(field) = guard_input.get_untracked() { field.set_value(""); }
                                                    if let Some(field) = secret_input.get_untracked() { field.set_value(""); }
                                                    set_show_secret.set(event_target_checked(&event));
                                                }/></label>
                                        </div>
                                    }.into_any(),
                                    LoginMethod::WebToken => view! {
                                        <label class="field-label" for="web-token">"Browser login data"</label>
                                        <textarea id="web-token" rows="4" placeholder="Paste data" spellcheck="false" autocomplete="off" disabled/>
                                    }.into_any(),
                                    LoginMethod::Qr => view! {
                                        <div class="qr-unavailable"><Icon icon=icondata::LuQrCode/><span>"No active QR session"</span></div>
                                        <label class="remember-qr"><input type="checkbox" disabled/>"Remember for later"</label>
                                    }.into_any(),
                                }}
                                <div id="sign-in-status" class="service-status" role="status"><Icon icon=icondata::LuInfo/>
                                    <span>{move || if !desktop.get() {
                                        "Desktop connection unavailable."
                                    } else if session_busy.get() {
                                        "Cancelling Steam login..."
                                    } else if login_busy.get() {
                                        "Connecting to Steam. Approve in the Steam mobile app if prompted."
                                    } else if method.get() != LoginMethod::Regular {
                                        "This login method is not available yet."
                                    } else {
                                        "Steam Guard code or Steam mobile approval may be required."
                                    }}</span>
                                </div>
                                <Show when=move || !login_error.get().is_empty()><p role="alert" class="account-error">{move || login_error.get()}</p></Show>
                                <button class="sign-in" type="submit" aria-describedby="sign-in-status"
                                    disabled=move || !desktop.get() || loading.get() || login_busy.get() || session_busy.get() || method.get() != LoginMethod::Regular>
                                    <Icon icon=icondata::LuLockKeyhole/>{move || if login_busy.get() { "Connecting..." } else { "Sign in" }}
                                </button>
                                <Show when=move || login_busy.get()>
                                    <button class="cancel-login" type="button" disabled=move || session_busy.get() on:click=move |_| sign_out.run(())>"Cancel login"</button>
                                </Show>
                            </form>
                        </div>
                        </Show>
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
                            <div><strong>"Nothing here"</strong><span>"No saved accounts"</span></div>
                        </div>
                    </Show>
                    <ul class="account-list">
                        <For each=move || accounts.get() key=|account| (account.username.clone(), account.has_saved_login, account.display_name.clone()) children=move |account| {
                            let selected_name = account.username.clone();
                            let removed_name = account.username.clone();
                            let login_name = account.username.clone();
                            let saved_login = account.has_saved_login.then(|| view! {
                                <button class="icon-button" title="Sign in with saved login" aria-label=format!("Sign in as {login_name}")
                                    disabled=move || login_busy.get() || session_busy.get() || profile.get().is_some()
                                    on:click=move |_| login_saved.run(login_name.clone())><Icon icon=icondata::LuLogIn/></button>
                            });
                            view! {
                                <li class="saved-account">
                                    <div class="avatar"><Icon icon=icondata::LuUserRound/></div>
                                    <button class="account-select" title="Select account" disabled=move || login_busy.get() || session_busy.get() || profile.get().is_some() on:click=move |_| {
                                        set_username.set(selected_name.clone());
                                        set_method.set(LoginMethod::Regular);
                                    }><strong>{account.display_name}</strong><span>{account.username}</span></button>
                                    {saved_login}
                                    <button class="icon-button delete-account" aria-label="Remove saved account" title="Remove saved account"
                                        disabled=move || login_busy.get() || session_busy.get()
                                        on:click=move |_| set_pending_removal.set(Some(removed_name.clone()))><Icon icon=icondata::LuTrash2/></button>
                                </li>
                            }
                        }/>
                    </ul>
                </aside>
            </main>
            </Show>
            <Show when=move || inventory_open.get() && profile.get().is_some()>
                <InventoryView snapshot=inventory busy=inventory_busy error=inventory_error on_refresh=refresh_inventory on_sign_out=sign_out/>
            </Show>
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
