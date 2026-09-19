// SPDX-License-Identifier: MIT
use lore_base::types::RepositoryId;
use lore_credential::token_store;
use lore_transport::auth::{authentication, exchange::load_authentication_token};
use lore_transport::error::ProtocolError;
use lore_transport::traits::Authentication;
use lore_transport::types::*;

const URL: &str = "refresh-test://auth.example.com";
const EXPIRED: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJsb3JlIiwic3ViIjoiYWxpY2UiLCJuYW1lIjoiQWxpY2UiLCJleHAiOjEwMDAwMDAwMDAsImF1ZCI6WyJleGFtcGxlLmNvbSJdfQ.signature";
const VALID: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJsb3JlIiwic3ViIjoiYWxpY2UiLCJleHAiOjIwMDAwMDAwMDAsImF1ZCI6WyJleGFtcGxlLmNvbSJdLCJuYW1lIjoiQWxpY2UifQ.signature";
struct Provider;
#[async_trait::async_trait]
impl Authentication for Provider {
    async fn start_auth_session(
        &self,
        _auth_url: &str,
        _client_state: &str,
        _correlation_id: &str,
    ) -> Result<AuthSession, ProtocolError> {
        unreachable!()
    }
    async fn poll_auth_session(
        &self,
        _auth_url: &str,
        _client_state: &str,
        _session_code: &str,
        _correlation_id: &str,
    ) -> Result<Option<AuthenticationToken>, ProtocolError> {
        unreachable!()
    }
    async fn exchange_external_token(
        &self,
        _auth_url: &str,
        _token: &str,
        _token_type: &str,
        _correlation_id: &str,
    ) -> Result<AuthenticationToken, ProtocolError> {
        unreachable!()
    }
    async fn refresh_authentication(
        &self,
        _auth_url: &str,
        refresh_token: &str,
        _correlation_id: &str,
    ) -> Result<AuthenticationToken, ProtocolError> {
        assert_eq!(refresh_token, "refresh-one");
        let marker =
            std::path::PathBuf::from(std::env::var("LORE_AUTH_PATH").unwrap()).join("refresh-used");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(marker)
            .unwrap();
        Ok(AuthenticationToken {
            token: VALID.into(),
            user_id: "alice".into(),
            user_name: "Alice".into(),
            expires_ms: 2_000_000_000_000,
            acceptable_root_domains: vec!["example.com".into()],
            refresh_token: Some("refresh-two".into()),
        })
    }
    async fn exchange_for_repository(
        &self,
        _auth_url: &str,
        _authn_token: &str,
        _repository: RepositoryId,
        _correlation_id: &str,
    ) -> Result<AuthorizationToken, ProtocolError> {
        unreachable!()
    }
    async fn exchange_for_custom_resource(
        &self,
        _auth_url: &str,
        _authn_token: &str,
        _resource_id: &str,
        _correlation_id: &str,
    ) -> Result<AuthorizationToken, ProtocolError> {
        unreachable!()
    }
    async fn get_user_info(
        &self,
        _auth_url: &str,
        _authz_token: &str,
        _repository: RepositoryId,
        _user_ids: &[String],
        _correlation_id: &str,
    ) -> Result<Vec<ResolvedUser>, ProtocolError> {
        unreachable!()
    }
    async fn get_user_id(
        &self,
        _auth_url: &str,
        _authz_token: &str,
        _repository: RepositoryId,
        _display_name: &str,
        _correlation_id: &str,
    ) -> Result<Option<ResolvedUser>, ProtocolError> {
        unreachable!()
    }
}

#[tokio::test]
async fn refresh_worker() {
    let Ok(mode) = std::env::var("LORE_REFRESH_TEST") else {
        return;
    };
    if mode == "seed" {
        let _guard = token_store::lock_refresh().await.unwrap();
        token_store::store_user_credentials(
            URL,
            "alice",
            EXPIRED,
            Some("refresh-one"),
            vec!["example.com".into()],
        )
        .await
        .unwrap();
        return;
    }
    authentication::add("refresh-test", std::sync::Arc::new(Provider)).unwrap();
    let token = load_authentication_token(
        URL,
        "alice",
        token_store::tokens_only_for_recipient_domain("example.com".into()),
        "",
        "",
    )
    .await
    .unwrap();
    assert_eq!(token, VALID);
    assert_eq!(
        token_store::load_refresh_token(URL, "alice").await.unwrap(),
        "refresh-two"
    );
}

#[test]
fn rotation_is_shared_across_processes() {
    let dir = std::env::temp_dir().join(format!(
        "lore-refresh-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir(&dir).unwrap();
    let worker = |mode: &str| {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "refresh_worker", "--nocapture"])
            .env("LORE_REFRESH_TEST", mode)
            .env("LORE_AUTH_PATH", &dir)
            .env("LORE_AUTH_STORE", "fallback")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let output = worker("seed").wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let workers: Vec<_> = (0..4).map(|_| worker("refresh")).collect();
    for child in workers {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    assert!(dir.join("refresh-used").exists());
    std::fs::remove_dir_all(dir).unwrap();
}
