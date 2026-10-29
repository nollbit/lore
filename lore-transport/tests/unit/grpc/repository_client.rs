// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

use lore_base::types::RepositoryId;
use lore_proto::lore::model::v1 as model_v1;
use lore_proto::lore::repository::v1 as repository_v1;
use lore_proto::lore::repository::v1::repository_service_server::RepositoryService;
use lore_proto::lore::repository::v1::repository_service_server::RepositoryServiceServer;
use lore_transport::Repository;
use lore_transport::auth::AuthEndpoint;
use lore_transport::connection::SuppliedCredentials;
use lore_transport::grpc::GRPCConnection;
use lore_transport::grpc::RequestLoggerLayer;
use lore_transport::grpc::repository_client;
use tokio_stream::Stream;
use tonic::Request;
use tonic::Response;
use tonic::Status;

const REPOSITORY: &str = "00112233445566778899aabbccddeeff";

/// Answers `RepositoryGet` with the repository asked for, as a server that requires a bearer
/// token does, and keeps the `authorization` header of every lookup it receives.
#[derive(Default)]
struct RecordingServer {
    authorizations: Arc<Mutex<Vec<Option<String>>>>,
}

#[tonic::async_trait]
impl RepositoryService for RecordingServer {
    type RepositoryListStream =
        Pin<Box<dyn Stream<Item = Result<repository_v1::RepositoryListResponse, Status>> + Send>>;

    async fn repository_create(
        &self,
        _request: Request<repository_v1::RepositoryCreateRequest>,
    ) -> Result<Response<repository_v1::RepositoryCreateResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn repository_delete(
        &self,
        _request: Request<repository_v1::RepositoryDeleteRequest>,
    ) -> Result<Response<repository_v1::RepositoryDeleteResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn repository_get(
        &self,
        request: Request<repository_v1::RepositoryGetRequest>,
    ) -> Result<Response<repository_v1::RepositoryGetResponse>, Status> {
        let authorization = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        self.authorizations
            .lock()
            .unwrap()
            .push(authorization.clone());
        if authorization.is_none() {
            return Err(Status::unauthenticated("authorization header required"));
        }
        let id = match request.into_inner().query {
            Some(repository_v1::repository_get_request::Query::Id(id)) => id,
            _ => return Err(Status::not_found("only lookups by id are answered")),
        };
        Ok(Response::new(repository_v1::RepositoryGetResponse {
            repository: Some(model_v1::Repository {
                id,
                name: "repo".to_string(),
                ..Default::default()
            }),
        }))
    }

    async fn repository_list(
        &self,
        _request: Request<repository_v1::RepositoryListRequest>,
    ) -> Result<Response<Self::RepositoryListStream>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn repository_metadata_get(
        &self,
        _request: Request<repository_v1::RepositoryMetadataGetRequest>,
    ) -> Result<Response<repository_v1::RepositoryMetadataGetResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn repository_metadata_set(
        &self,
        _request: Request<repository_v1::RepositoryMetadataSetRequest>,
    ) -> Result<Response<repository_v1::RepositoryMetadataSetResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }
}

/// A repository client for a call that supplied `identity_token` and `access_token`, over a
/// [`RecordingServer`], and the `authorization` headers that server receives.
async fn start_repository_client(
    identity_token: &str,
    access_token: &str,
) -> (Arc<dyn Repository>, Arc<Mutex<Vec<Option<String>>>>) {
    let server = RecordingServer::default();
    let authorizations = server.authorizations.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    #[allow(clippy::disallowed_methods)] // Test-local server task.
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(RepositoryServiceServer::new(server))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    let channel = tonic::transport::Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect to test server");
    let channel = tower::ServiceBuilder::new()
        .layer(RequestLoggerLayer {})
        .service(channel);
    let connection = Arc::new(GRPCConnection::for_test(
        format!("http://{addr}").parse().expect("test url"),
        channel,
    ));

    let repository = repository_client(
        connection,
        &AuthEndpoint::resolve("ucs-auth://auth.example.com").expect("a registered scheme"),
        "alice",
        &Arc::new(SuppliedCredentials::new(identity_token, access_token)),
    )
    .await
    .expect("repository client");
    (repository, authorizations)
}

/// With only an access token there is no login token to send, so a lookup by ID carries the
/// access token, which authorizes the repository it is scoped to.
#[tokio::test]
async fn access_token_alone_authorizes_a_lookup_by_id() {
    let (repository, authorizations) = start_repository_client("", "supplied-authz").await;
    let id: RepositoryId = REPOSITORY.parse().expect("a valid repository id");

    let data = repository
        .query(Some(id), None)
        .await
        .expect("the access token authorizes the lookup");

    assert_eq!(data.id, id);
    assert_eq!(
        authorizations.lock().unwrap().as_slice(),
        [Some("Bearer supplied-authz".to_string())]
    );
}

/// An identity token is the login token a lookup sends, whether or not an access token came
/// with it.
#[tokio::test]
async fn an_identity_token_still_authorizes_a_lookup_by_id() {
    for access_token in ["", "supplied-authz"] {
        let (repository, authorizations) =
            start_repository_client("supplied-authn", access_token).await;
        let id: RepositoryId = REPOSITORY.parse().expect("a valid repository id");

        repository
            .query(Some(id), None)
            .await
            .expect("the identity token authorizes the lookup");

        assert_eq!(
            authorizations.lock().unwrap().as_slice(),
            [Some("Bearer supplied-authn".to_string())],
            "access token {access_token:?}"
        );
    }
}
