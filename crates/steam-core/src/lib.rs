//! Steam connection lifecycle: login strategies, session, CM events.

mod live;

pub use live::SteamClient;

use std::{future::Future, sync::Mutex, time::Duration};

pub struct Credentials {
    pub account: String,
    pub password: String,
    pub guard_code: Option<String>,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.password.zeroize();
        self.guard_code.zeroize();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub account: String,
    pub steam_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LoginError {
    #[error("Enter a Steam username and password.")]
    InvalidCredentials,
    #[error("A Steam login is already in progress.")]
    Busy,
    #[error("Sign out before connecting another Steam account.")]
    AlreadyConnected,
    #[error("Sign in to Steam first.")]
    NotConnected,
    #[error("Enter your Steam Guard code and try again.")]
    GuardRequired,
    #[error("Steam could not verify the Guard code. Try again.")]
    GuardRejected,
    #[error("Steam rejected the username or password.")]
    CredentialsRejected,
    #[error("Steam is limiting login attempts. Wait before trying again.")]
    RateLimited,
    #[error("Steam login was cancelled.")]
    Cancelled,
    #[error("Steam login timed out. Check your connection and try again.")]
    TimedOut,
    #[error("Could not connect to Steam. Check your credentials and connection.")]
    ConnectionFailed,
    #[error("The Steam session could not be accessed.")]
    SessionUnavailable,
}

pub trait SteamTransport: Send + Sync {
    type Connection: Send;

    fn login(
        &self,
        credentials: Credentials,
    ) -> impl Future<Output = Result<(Self::Connection, Profile), LoginError>> + Send;
}

struct Session<Connection> {
    generation: u64,
    connecting: bool,
    connected: Option<(Connection, Profile)>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

pub struct SessionManager<Transport: SteamTransport> {
    transport: Transport,
    session: Mutex<Session<Transport::Connection>>,
    login_timeout: Duration,
    changed: tokio::sync::watch::Sender<u64>,
}

struct LoginAttempt<'a, Connection> {
    session: &'a Mutex<Session<Connection>>,
    generation: u64,
}

impl<Connection> Drop for LoginAttempt<'_, Connection> {
    fn drop(&mut self) {
        if let Ok(mut session) = self.session.lock()
            && session.generation == self.generation
        {
            session.connecting = false;
            session.cancel.take();
        }
    }
}

impl<Transport: SteamTransport> SessionManager<Transport> {
    pub fn new(transport: Transport, login_timeout: Duration) -> Self {
        Self {
            transport,
            session: Mutex::new(Session {
                generation: 0,
                connecting: false,
                connected: None,
                cancel: None,
            }),
            login_timeout,
            changed: tokio::sync::watch::channel(0).0,
        }
    }

    pub async fn login(&self, credentials: Credentials) -> Result<Profile, LoginError> {
        if credentials.account.trim().is_empty() || credentials.password.is_empty() {
            return Err(LoginError::InvalidCredentials);
        }
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let generation = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| LoginError::SessionUnavailable)?;
            if session.connecting {
                return Err(LoginError::Busy);
            }
            if session.connected.is_some() {
                return Err(LoginError::AlreadyConnected);
            }
            session.generation += 1;
            session.connecting = true;
            session.cancel = Some(cancel);
            session.generation
        };
        let _attempt = LoginAttempt {
            session: &self.session,
            generation,
        };
        let (connection, profile) = tokio::select! {
            biased;
            _ = cancelled => return Err(LoginError::Cancelled),
            result = tokio::time::timeout(self.login_timeout, self.transport.login(credentials)) => {
                result.map_err(|_| LoginError::TimedOut)??
            }
        };
        let mut session = self
            .session
            .lock()
            .map_err(|_| LoginError::SessionUnavailable)?;
        if session.generation != generation {
            return Err(LoginError::Cancelled);
        }
        session.connected = Some((connection, profile.clone()));
        Ok(profile)
    }

    pub fn profile(&self) -> Result<Option<Profile>, LoginError> {
        let session = self
            .session
            .lock()
            .map_err(|_| LoginError::SessionUnavailable)?;
        Ok(session
            .connected
            .as_ref()
            .map(|(_, profile)| profile.clone()))
    }

    pub async fn with_connection<Output, Operation, Work>(
        &self,
        operation: Operation,
    ) -> Result<Output, LoginError>
    where
        Transport::Connection: Clone,
        Operation: FnOnce(Transport::Connection, u64) -> Work,
        Work: Future<Output = Output>,
    {
        let mut changed = self.changed.subscribe();
        let (generation, connection) = {
            let session = self
                .session
                .lock()
                .map_err(|_| LoginError::SessionUnavailable)?;
            let (connection, _) = session.connected.as_ref().ok_or(LoginError::NotConnected)?;
            changed.borrow_and_update();
            (session.generation, connection.clone())
        };
        tokio::select! {
            biased;
            _ = changed.changed() => Err(LoginError::Cancelled),
            output = async { operation(connection, generation).await } => {
                let session = self.session.lock().map_err(|_| LoginError::SessionUnavailable)?;
                if session.generation != generation {
                    return Err(LoginError::Cancelled);
                }
                Ok(output)
            }
        }
    }

    pub fn sign_out(&self) -> Result<(), LoginError> {
        let (connection, cancel) = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| LoginError::SessionUnavailable)?;
            session.generation += 1;
            session.connecting = false;
            self.changed.send_replace(session.generation);
            (session.connected.take(), session.cancel.take())
        };
        drop(cancel);
        drop(connection);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::Notify;

    #[derive(Clone)]
    struct TestConnection(Arc<AtomicBool>);

    impl Drop for TestConnection {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    struct TestTransport {
        ready: Arc<Notify>,
        dropped: Arc<AtomicBool>,
    }

    impl SteamTransport for TestTransport {
        type Connection = TestConnection;

        async fn login(
            &self,
            credentials: Credentials,
        ) -> Result<(TestConnection, Profile), LoginError> {
            self.ready.notified().await;
            Ok((
                TestConnection(self.dropped.clone()),
                Profile {
                    account: credentials.account.clone(),
                    steam_id: "76561198000000000".into(),
                },
            ))
        }
    }

    fn credentials() -> Credentials {
        Credentials {
            account: "test-account".into(),
            password: "test-password".into(),
            guard_code: None,
        }
    }

    fn manager() -> SessionManager<TestTransport> {
        SessionManager::new(
            TestTransport {
                ready: Arc::new(Notify::new()),
                dropped: Arc::new(AtomicBool::new(false)),
            },
            Duration::from_secs(30),
        )
    }

    #[tokio::test]
    async fn rejects_empty_credentials_without_starting_a_session() {
        let manager = manager();
        let mut input = credentials();
        input.password.clear();
        assert_eq!(
            manager.login(input).await,
            Err(LoginError::InvalidCredentials)
        );
        assert_eq!(manager.profile().unwrap(), None);
    }

    #[tokio::test]
    async fn retains_connection_until_sign_out_and_allows_relogin() {
        let manager = manager();
        manager.transport.ready.notify_one();
        let profile = manager.login(credentials()).await.unwrap();
        assert_eq!(manager.profile().unwrap(), Some(profile));
        assert_eq!(
            manager.login(credentials()).await,
            Err(LoginError::AlreadyConnected)
        );
        assert!(!manager.transport.dropped.load(Ordering::SeqCst));
        manager.sign_out().unwrap();
        assert!(manager.transport.dropped.load(Ordering::SeqCst));
        assert_eq!(manager.profile().unwrap(), None);
        manager.transport.ready.notify_one();
        assert!(manager.login(credentials()).await.is_ok());
    }

    #[tokio::test]
    async fn rejects_concurrent_login_and_recovers_when_attempt_is_dropped() {
        let manager = manager();
        let mut first = Box::pin(manager.login(credentials()));
        tokio::select! { biased; _ = &mut first => panic!("login should wait"), _ = std::future::ready(()) => {} }
        assert_eq!(manager.login(credentials()).await, Err(LoginError::Busy));
        drop(first);
        manager.transport.ready.notify_one();
        assert!(manager.login(credentials()).await.is_ok());
    }

    #[tokio::test]
    async fn late_login_cannot_restore_a_signed_out_session() {
        let manager = manager();
        let mut first = Box::pin(manager.login(credentials()));
        tokio::select! { biased; _ = &mut first => panic!("login should wait"), _ = std::future::ready(()) => {} }
        manager.sign_out().unwrap();
        manager.transport.ready.notify_one();
        assert_eq!(first.await, Err(LoginError::Cancelled));
        assert_eq!(manager.profile().unwrap(), None);
        assert!(!manager.transport.dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn sign_out_cancels_pending_network_work_without_waiting_for_timeout() {
        let manager = manager();
        let mut first = Box::pin(manager.login(credentials()));
        tokio::select! { biased; _ = &mut first => panic!("login should wait"), _ = std::future::ready(()) => {} }
        manager.sign_out().unwrap();
        assert_eq!(first.await, Err(LoginError::Cancelled));
        manager.transport.ready.notify_one();
        assert!(manager.login(credentials()).await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_clears_the_pending_attempt() {
        let manager = manager();
        assert_eq!(
            manager.login(credentials()).await,
            Err(LoginError::TimedOut)
        );
        manager.transport.ready.notify_one();
        assert!(manager.login(credentials()).await.is_ok());
    }

    #[tokio::test]
    async fn connection_work_requires_a_signed_in_session() {
        let manager = manager();
        assert_eq!(
            manager.with_connection(|_, _| async { 42 }).await,
            Err(LoginError::NotConnected)
        );
        manager.transport.ready.notify_one();
        manager.login(credentials()).await.unwrap();
        assert_eq!(manager.with_connection(|_, _| async { 42 }).await, Ok(42));
    }

    #[tokio::test]
    async fn sign_out_cancels_pending_connection_work() {
        let manager = manager();
        manager.transport.ready.notify_one();
        manager.login(credentials()).await.unwrap();
        let mut work = Box::pin(manager.with_connection(|_, _| std::future::pending::<()>()));
        tokio::select! { biased; _ = &mut work => panic!("work should wait"), _ = std::future::ready(()) => {} }
        manager.sign_out().unwrap();
        assert_eq!(work.await, Err(LoginError::Cancelled));
    }

    #[tokio::test]
    async fn old_connection_reply_cannot_cross_a_relogin() {
        let manager = manager();
        manager.transport.ready.notify_one();
        manager.login(credentials()).await.unwrap();
        let ready = Notify::new();
        let mut work = Box::pin(manager.with_connection(|_, _| async {
            ready.notified().await;
            "old inventory"
        }));
        tokio::select! { biased; _ = &mut work => panic!("work should wait"), _ = std::future::ready(()) => {} }
        manager.sign_out().unwrap();
        manager.transport.ready.notify_one();
        manager.login(credentials()).await.unwrap();
        ready.notify_one();
        assert_eq!(work.await, Err(LoginError::Cancelled));
        assert!(manager.profile().unwrap().is_some());
    }

    #[tokio::test]
    async fn relogin_has_a_distinct_connection_generation_for_cache_ownership() {
        let manager = manager();
        manager.transport.ready.notify_one();
        manager.login(credentials()).await.unwrap();
        let first = manager
            .with_connection(|_, generation| async move { generation })
            .await
            .unwrap();
        manager.sign_out().unwrap();
        manager.transport.ready.notify_one();
        manager.login(credentials()).await.unwrap();
        let second = manager
            .with_connection(|_, generation| async move { generation })
            .await
            .unwrap();
        assert_ne!(first, second);
    }
}
