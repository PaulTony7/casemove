use crate::{Credentials, LoginError};
use base64::{Engine, engine::general_purpose::STANDARD};
use zeroize::Zeroizing;

pub(crate) fn shared_secret(value: Option<&str>) -> Result<Option<Zeroizing<String>>, LoginError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || value.len() > 64 {
        return Err(LoginError::InvalidSharedSecret);
    }
    let mut bytes = Zeroizing::new([0u8; 48]);
    let length = STANDARD
        .decode_slice(value, &mut *bytes)
        .map_err(|_| LoginError::InvalidSharedSecret)?;
    if length != 20 {
        return Err(LoginError::InvalidSharedSecret);
    }
    Ok(Some(Zeroizing::new(STANDARD.encode(&bytes[..length]))))
}

pub(crate) fn validate_options(credentials: &Credentials) -> Result<(), LoginError> {
    let secret = shared_secret(credentials.shared_secret.as_deref())?;
    if secret.is_some()
        && (credentials
            .guard_code
            .as_ref()
            .is_some_and(|code| !code.trim().is_empty())
            || credentials.refresh_token.is_some())
    {
        return Err(LoginError::ConflictingGuardOptions);
    }
    Ok(())
}

pub(crate) fn code(
    secret: &str,
    time: std::time::SystemTime,
) -> Result<Zeroizing<String>, LoginError> {
    let mut bytes = Zeroizing::new([0u8; 48]);
    let length = STANDARD
        .decode_slice(secret, &mut *bytes)
        .map_err(|_| LoginError::InvalidSharedSecret)?;
    let secret = totp_rs::Secret::new_stack(
        bytes[..length]
            .try_into()
            .map_err(|_| LoginError::InvalidSharedSecret)?,
    );
    let generator = totp_rs::Builder::new()
        .with_algorithm(totp_rs::Algorithm::Steam)
        .with_digits(5)
        .with_step_duration(30)
        .with_secret(secret)
        .build()
        .map_err(|_| LoginError::InvalidSharedSecret)?;
    let timestamp = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| LoginError::InvalidSharedSecret)?
        .as_secs();
    Ok(Zeroizing::new(generator.generate(timestamp).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_normalizes_a_steam_secret_without_exposing_it_in_errors() {
        let synthetic = STANDARD.encode([0u8; 20]);
        assert_eq!(
            shared_secret(Some(&format!(" {synthetic} ")))
                .unwrap()
                .unwrap()
                .as_str(),
            synthetic
        );
        for value in [
            "",
            "invalid!",
            "YQ==",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            assert!(matches!(
                shared_secret(Some(value)),
                Err(LoginError::InvalidSharedSecret)
            ));
        }
        assert!(shared_secret(None).unwrap().is_none());
        assert!(
            !LoginError::InvalidSharedSecret
                .to_string()
                .contains("invalid!")
        );
    }

    #[test]
    fn rejects_ambiguous_guard_and_saved_token_options() {
        let mut credentials = Credentials {
            account: "test".into(),
            password: "synthetic".into(),
            guard_code: Some("ABCDE".into()),
            refresh_token: None,
            shared_secret: Some(STANDARD.encode([0u8; 20])),
        };
        assert_eq!(
            validate_options(&credentials),
            Err(LoginError::ConflictingGuardOptions)
        );
        credentials.guard_code = None;
        assert!(validate_options(&credentials).is_ok());
        credentials.refresh_token = Some("synthetic-token".into());
        assert_eq!(
            validate_options(&credentials),
            Err(LoginError::ConflictingGuardOptions)
        );
    }

    #[test]
    fn steam_codes_use_five_characters_and_thirty_second_steps_without_clock_panics() {
        use std::time::{Duration, UNIX_EPOCH};
        let synthetic = STANDARD.encode([0u8; 20]);
        let first = code(&synthetic, UNIX_EPOCH).unwrap();
        assert_eq!(first.len(), 5);
        assert!(
            first
                .chars()
                .all(|letter| "23456789BCDFGHJKMNPQRTVWXY".contains(letter))
        );
        assert_eq!(
            first.as_str(),
            code(&synthetic, UNIX_EPOCH + Duration::from_secs(29))
                .unwrap()
                .as_str()
        );
        assert_ne!(
            first.as_str(),
            code(&synthetic, UNIX_EPOCH + Duration::from_secs(30))
                .unwrap()
                .as_str()
        );
        assert!(code(&synthetic, UNIX_EPOCH - Duration::from_secs(1)).is_err());
    }
}
