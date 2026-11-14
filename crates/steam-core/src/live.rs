use crate::{Credentials, LoginError, Profile, SteamTransport, WalletUpdate};
use std::io::Cursor;
use std::sync::Arc;
use steam_auth::{EAuthTokenPlatformType, LoginSession};
use steam_client::{LogOnDetails, SteamClient as BrowserClient, SteamID};
use steam_vent::{
    Connection as VentConnection, ConnectionError, EResult, NetworkError, ServerList,
    auth::{
        AuthConfirmationHandler, ConfirmationAction, ConfirmationMethod, DeviceConfirmationHandler,
        NullGuardDataStore, UserProvidedAuthConfirmationHandler,
    },
};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

pub struct SteamClient;

pub type BrowserClientHandle = Arc<Mutex<BrowserClient>>;

#[derive(Clone)]
pub enum SteamConnection {
    SteamVent {
        connection: VentConnection,
        refresh_token: Option<Arc<Zeroizing<String>>>,
    },
    BrowserToken {
        client: BrowserClientHandle,
        steam_id: String,
    },
}

impl SteamConnection {
    pub fn access_token(&self) -> Option<&str> {
        match self {
            Self::SteamVent {
                connection,
                refresh_token,
            } => refresh_token
                .as_deref()
                .map(|token| token.as_str())
                .or_else(|| connection.access_token()),
            Self::BrowserToken { .. } => None,
        }
    }

    pub fn steam_vent(&self) -> Option<&VentConnection> {
        match self {
            Self::SteamVent { connection, .. } => Some(connection),
            Self::BrowserToken { .. } => None,
        }
    }

    pub fn browser_client(&self) -> Option<BrowserClientHandle> {
        match self {
            Self::SteamVent { .. } => None,
            Self::BrowserToken { client, .. } => Some(client.clone()),
        }
    }

    pub fn steam_id(&self) -> String {
        match self {
            Self::SteamVent { connection, .. } => connection.steam_id().steam64().to_string(),
            Self::BrowserToken { steam_id, .. } => steam_id.clone(),
        }
    }

    /// Wallet balance updates from the CM. Only the SteamVent transport
    /// receives `CMsgClientWalletInfoUpdate`; browser-token sessions return
    /// `None` (no balance available without the CM connection).
    pub fn wallet_stream(
        &self,
    ) -> Option<std::pin::Pin<Box<dyn tokio_stream::Stream<Item = WalletUpdate> + Send>>> {
        match self {
            Self::SteamVent { connection, .. } => Some(crate::wallet::wallet_stream(connection)),
            Self::BrowserToken { .. } => None,
        }
    }
}

struct DesktopConfirmation {
    code: Option<Zeroizing<String>>,
    shared_secret: Option<Zeroizing<String>>,
}

impl AuthConfirmationHandler for DesktopConfirmation {
    async fn handle_confirmation(
        self,
        allowed: &[ConfirmationMethod],
    ) -> Option<ConfirmationAction> {
        if allowed.iter().any(|method| !method.action_required()) {
            return Some(ConfirmationAction::None);
        }
        if let Some(secret) = self.shared_secret {
            let device: Vec<_> = allowed
                .iter()
                .filter(|method| {
                    matches!(
                        method.token_type(),
                        Some(steam_vent::auth::GuardTokenType::Device)
                    )
                })
                .cloned()
                .collect();
            if !device.is_empty() {
                let code = crate::totp::code(secret.as_str(), std::time::SystemTime::now()).ok()?;
                let handler = UserProvidedAuthConfirmationHandler::new(
                    Cursor::new(code.as_bytes()),
                    tokio::io::sink(),
                );
                return handler.handle_confirmation(&device).await;
            }
        }
        if let Some(code) = self.code.filter(|code| !code.trim().is_empty()) {
            let input = Cursor::new(code.trim().as_bytes());
            let handler = UserProvidedAuthConfirmationHandler::new(input, tokio::io::sink());
            if let Some(action) = handler.handle_confirmation(allowed).await {
                return Some(action);
            }
        }
        DeviceConfirmationHandler.handle_confirmation(allowed).await
    }
}

fn login_error(error: ConnectionError) -> LoginError {
    match error {
        ConnectionError::Network(NetworkError::ApiError(
            EResult::TwoFactorCodeMismatch
            | EResult::InvalidLoginAuthCode
            | EResult::ExpiredLoginAuthCode,
        )) => LoginError::GuardRejected,
        ConnectionError::Network(NetworkError::ApiError(result)) => {
            login_error(ConnectionError::LoginError(result.into()))
        }
        ConnectionError::Network(NetworkError::Timeout) => LoginError::TimedOut,
        ConnectionError::UnsupportedConfirmationAction(_) => LoginError::GuardRequired,
        ConnectionError::Aborted => LoginError::Cancelled,
        ConnectionError::LoginError(steam_vent::LoginError::SteamGuardRequired) => {
            LoginError::GuardRequired
        }
        ConnectionError::LoginError(steam_vent::LoginError::InvalidCredentials) => {
            LoginError::CredentialsRejected
        }
        ConnectionError::LoginError(steam_vent::LoginError::RateLimited) => LoginError::RateLimited,
        _ => LoginError::ConnectionFailed,
    }
}

impl SteamTransport for SteamClient {
    type Connection = SteamConnection;

    async fn login(
        &self,
        mut credentials: Credentials,
    ) -> Result<(SteamConnection, Profile), LoginError> {
        crate::totp::validate_options(&credentials)?;
        if credentials.web_logon_token.is_some() {
            return self.login_with_browser_token(credentials).await;
        }
        if let Some(challenge_tx) = credentials.qr_challenge_tx.take() {
            return self.login_with_qr(credentials, challenge_tx).await;
        }

        let shared_secret = crate::totp::shared_secret(credentials.shared_secret.as_deref())?;
        let servers = ServerList::discover()
            .await
            .map_err(|_| LoginError::ConnectionFailed)?;
        let handler = DesktopConfirmation {
            shared_secret,
            code: credentials
                .guard_code
                .as_ref()
                .map(|code| Zeroizing::new(code.clone())),
        };
        let token = credentials
            .refresh_token
            .as_deref()
            .map(|token| Arc::new(Zeroizing::new(token.to_owned())));
        let connection = if let Some(token) = token.as_deref() {
            VentConnection::access(&servers, &credentials.account, token.as_str())
                .await
                .map_err(saved_login_error)?
        } else {
            VentConnection::login(
                &servers,
                &credentials.account,
                &credentials.password,
                NullGuardDataStore,
                handler,
            )
            .await
            .map_err(login_error)?
        };
        let token = token.or_else(|| {
            connection
                .access_token()
                .filter(|token| !token.is_empty())
                .map(|token| Arc::new(Zeroizing::new(token.to_owned())))
        });
        let profile = Profile {
            account: credentials.account.clone(),
            steam_id: connection.steam_id().steam64().to_string(),
            generation: 0,
        };
        Ok((
            SteamConnection::SteamVent {
                connection,
                refresh_token: token,
            },
            profile,
        ))
    }
}

impl SteamClient {
    async fn login_with_qr(
        &self,
        _credentials: Credentials,
        challenge_tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<(SteamConnection, Profile), LoginError> {
        let mut auth = LoginSession::new(
            EAuthTokenPlatformType::KEAuthTokenPlatformTypeSteamClient,
            None,
        );
        let challenge = auth
            .start_with_qr()
            .await
            .map_err(|_| LoginError::QrLoginFailed)?
            .qr_challenge_url
            .ok_or(LoginError::QrLoginFailed)?;
        challenge_tx
            .send(challenge)
            .map_err(|_| LoginError::Cancelled)?;

        let result = loop {
            if let Some(result) = auth.poll().await.map_err(|_| LoginError::QrLoginFailed)? {
                break result;
            }
            let seconds = auth.poll_interval().clamp(0.1, 30.0);
            tokio::time::sleep(std::time::Duration::from_secs_f32(seconds)).await;
        };
        let account = result.account_name;
        let token = Zeroizing::new(result.refresh_token);
        let servers = ServerList::discover()
            .await
            .map_err(|_| LoginError::ConnectionFailed)?;
        let connection = VentConnection::access(&servers, &account, token.as_str())
            .await
            .map_err(saved_login_error)?;
        let profile = Profile {
            account,
            steam_id: connection.steam_id().steam64().to_string(),
            generation: 0,
        };
        Ok((
            SteamConnection::SteamVent {
                connection,
                refresh_token: Some(Arc::new(token)),
            },
            profile,
        ))
    }

    async fn login_with_browser_token(
        &self,
        mut credentials: Credentials,
    ) -> Result<(SteamConnection, Profile), LoginError> {
        let account = credentials.account.clone();
        let token = credentials
            .web_logon_token
            .take()
            .ok_or(LoginError::InvalidWebToken)?;
        let steam_id = credentials
            .steam_id
            .take()
            .ok_or(LoginError::InvalidWebToken)?;
        let steam_id_value: SteamID = steam_id.parse().map_err(|_| LoginError::InvalidWebToken)?;
        let mut client = BrowserClient::new(Default::default());
        let response = client
            .log_on(LogOnDetails {
                account_name: Some(account.clone()),
                web_logon_token: Some(token),
                steam_id: Some(steam_id_value),
                ..Default::default()
            })
            .await
            .map_err(|_| LoginError::InvalidWebToken)?;
        let steam_id = response.steam_id.steam_id64().to_string();
        let profile = Profile {
            account,
            steam_id: steam_id.clone(),
            generation: 0,
        };
        Ok((
            SteamConnection::BrowserToken {
                client: Arc::new(Mutex::new(client)),
                steam_id,
            },
            profile,
        ))
    }
}

fn saved_login_error(error: ConnectionError) -> LoginError {
    match error {
        ConnectionError::AccessToken(_)
        | ConnectionError::Network(NetworkError::ApiError(EResult::Expired | EResult::Revoked))
        | ConnectionError::LoginError(steam_vent::LoginError::Unknown(
            EResult::Expired | EResult::Revoked,
        )) => LoginError::SavedLoginRejected,
        error => match login_error(error) {
            LoginError::CredentialsRejected | LoginError::GuardRequired => {
                LoginError::SavedLoginRejected
            }
            error => error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steam_vent::auth::GuardTokenType;
    use steam_vent_proto_steam::steammessages_auth_steamclient::{
        CAuthentication_AllowedConfirmation, EAuthSessionGuardType,
    };

    fn confirmation(kind: EAuthSessionGuardType) -> ConfirmationMethod {
        let mut method = CAuthentication_AllowedConfirmation::new();
        method.set_confirmation_type(kind);
        method.into()
    }

    #[test]
    fn reports_safe_actionable_errors_without_raw_network_details() {
        assert_eq!(
            login_error(ConnectionError::LoginError(
                steam_vent::LoginError::InvalidCredentials
            )),
            LoginError::CredentialsRejected
        );
        assert_eq!(
            login_error(ConnectionError::LoginError(
                steam_vent::LoginError::RateLimited
            )),
            LoginError::RateLimited
        );
        assert_eq!(
            login_error(ConnectionError::LoginError(
                steam_vent::LoginError::SteamGuardRequired
            )),
            LoginError::GuardRequired
        );
        assert_eq!(
            login_error(ConnectionError::UnsupportedConfirmationAction(vec![])),
            LoginError::GuardRequired
        );
        assert_eq!(login_error(ConnectionError::Aborted), LoginError::Cancelled);
        assert_eq!(
            login_error(ConnectionError::Network(NetworkError::ApiError(
                EResult::TwoFactorCodeMismatch
            ))),
            LoginError::GuardRejected
        );
        assert_eq!(
            login_error(ConnectionError::Network(NetworkError::ApiError(
                EResult::InvalidPassword
            ))),
            LoginError::CredentialsRejected
        );
        assert_eq!(
            login_error(ConnectionError::Network(NetworkError::Timeout)),
            LoginError::TimedOut
        );
    }

    #[test]
    fn expired_saved_tokens_request_password_login_but_network_errors_stay_distinct() {
        for result in [EResult::Expired, EResult::Revoked] {
            assert_eq!(
                saved_login_error(ConnectionError::Network(NetworkError::ApiError(result))),
                LoginError::SavedLoginRejected
            );
            assert_eq!(
                saved_login_error(ConnectionError::LoginError(
                    steam_vent::LoginError::Unknown(result)
                )),
                LoginError::SavedLoginRejected
            );
        }
        assert_eq!(
            saved_login_error(ConnectionError::Network(NetworkError::Timeout)),
            LoginError::TimedOut
        );
        assert_eq!(
            saved_login_error(ConnectionError::LoginError(
                steam_vent::LoginError::RateLimited
            )),
            LoginError::RateLimited
        );
    }

    #[tokio::test]
    async fn unsupported_guard_does_not_prompt_on_stdin() {
        let handler = DesktopConfirmation {
            code: None,
            shared_secret: None,
        };
        assert!(handler.handle_confirmation(&[]).await.is_none());
    }

    #[tokio::test]
    async fn code_required_without_a_code_returns_without_prompting() {
        let allowed = [confirmation(
            EAuthSessionGuardType::k_EAuthSessionGuardType_DeviceCode,
        )];
        let handler = DesktopConfirmation {
            code: None,
            shared_secret: None,
        };
        assert!(handler.handle_confirmation(&allowed).await.is_none());
    }

    #[tokio::test]
    async fn supplies_guard_code_for_device_or_email_confirmation() {
        for (kind, device) in [
            (
                EAuthSessionGuardType::k_EAuthSessionGuardType_DeviceCode,
                true,
            ),
            (
                EAuthSessionGuardType::k_EAuthSessionGuardType_EmailCode,
                false,
            ),
        ] {
            let allowed = [confirmation(kind)];
            let handler = DesktopConfirmation {
                code: Some(Zeroizing::new(" ABCDE ".into())),
                shared_secret: None,
            };
            let result = handler.handle_confirmation(&allowed).await;
            if device {
                assert!(matches!(
                    result,
                    Some(ConfirmationAction::GuardToken(_, GuardTokenType::Device))
                ));
            } else {
                assert!(matches!(
                    result,
                    Some(ConfirmationAction::GuardToken(_, GuardTokenType::Email))
                ));
            }
        }
    }

    #[tokio::test]
    async fn no_guard_or_mobile_approval_does_not_require_a_code() {
        for kind in [
            EAuthSessionGuardType::k_EAuthSessionGuardType_None,
            EAuthSessionGuardType::k_EAuthSessionGuardType_DeviceConfirmation,
        ] {
            let allowed = [confirmation(kind)];
            let handler = DesktopConfirmation {
                code: None,
                shared_secret: None,
            };
            assert!(matches!(
                handler.handle_confirmation(&allowed).await,
                Some(ConfirmationAction::None)
            ));
        }
    }

    #[tokio::test]
    async fn shared_secret_generates_only_device_codes_and_never_email_codes() {
        use base64::Engine;
        let secret = base64::engine::general_purpose::STANDARD.encode([0u8; 20]);
        let device = [
            confirmation(EAuthSessionGuardType::k_EAuthSessionGuardType_EmailCode),
            confirmation(EAuthSessionGuardType::k_EAuthSessionGuardType_DeviceCode),
        ];
        let handler = DesktopConfirmation {
            code: None,
            shared_secret: Some(Zeroizing::new(secret.clone())),
        };
        assert!(matches!(
            handler.handle_confirmation(&device).await,
            Some(ConfirmationAction::GuardToken(_, GuardTokenType::Device))
        ));
        let email = [confirmation(
            EAuthSessionGuardType::k_EAuthSessionGuardType_EmailCode,
        )];
        let handler = DesktopConfirmation {
            code: None,
            shared_secret: Some(Zeroizing::new(secret)),
        };
        assert!(handler.handle_confirmation(&email).await.is_none());
    }
}
