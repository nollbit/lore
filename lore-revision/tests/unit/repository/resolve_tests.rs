// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! Coverage for [`resolve_on`], the lookup `clone` and `link add` resolve a repository with.
use std::sync::Mutex;

use async_trait::async_trait;
use lore_base::error::NotAuthenticated;
use lore_base::error::NotFound;
use lore_base::types::Context;
use lore_base::types::Hash;
use lore_revision::lore::RepositoryId;
use lore_revision::repository::resolve_on;
use lore_transport::MetadataSetResult;
use lore_transport::ProtocolError;
use lore_transport::Repository;
use lore_transport::RepositoryData;

const REPOSITORY: &str = "00112233445566778899aabbccddeeff";

#[derive(Debug, Clone, PartialEq)]
enum Lookup {
    Id(RepositoryId),
    Name(String),
}

/// A repository service holding one repository, `repo` with the ID [`REPOSITORY`], that keeps
/// every lookup it receives.
struct Service {
    /// Whether lookups by name are refused as unauthenticated, as for a caller holding only an
    /// access token scoped to the repository.
    refuse_names: bool,
    lookups: Mutex<Vec<Lookup>>,
}

impl Service {
    fn new(refuse_names: bool) -> Self {
        Self {
            refuse_names,
            lookups: Mutex::new(vec![]),
        }
    }

    fn lookups(&self) -> Vec<Lookup> {
        self.lookups.lock().unwrap().clone()
    }
}

fn repository_id() -> RepositoryId {
    REPOSITORY.parse().expect("a valid repository id")
}

fn repository_data() -> RepositoryData {
    RepositoryData {
        id: repository_id(),
        name: "repo".to_string(),
        metadata: Hash::default(),
    }
}

#[async_trait]
impl Repository for Service {
    async fn create(
        &self,
        _id: RepositoryId,
        _name: &str,
        _description: &str,
        _default_branch_id: Context,
        _default_branch_name: &str,
        _creator: &str,
        _created: u64,
    ) -> Result<RepositoryData, ProtocolError> {
        Err(ProtocolError::internal("not used by this test"))
    }

    async fn delete(&self, _id: RepositoryId) -> Result<(), ProtocolError> {
        Err(ProtocolError::internal("not used by this test"))
    }

    async fn query(
        &self,
        id: Option<RepositoryId>,
        name: Option<&str>,
    ) -> Result<RepositoryData, ProtocolError> {
        match (id, name) {
            (Some(id), _) => {
                self.lookups.lock().unwrap().push(Lookup::Id(id));
                if id == repository_id() {
                    Ok(repository_data())
                } else {
                    Err(ProtocolError::from(NotFound))
                }
            }
            (None, Some(name)) => {
                self.lookups
                    .lock()
                    .unwrap()
                    .push(Lookup::Name(name.to_string()));
                if self.refuse_names {
                    Err(ProtocolError::from(NotAuthenticated))
                } else if name == "repo" {
                    Ok(repository_data())
                } else {
                    Err(ProtocolError::from(NotFound))
                }
            }
            (None, None) => Err(ProtocolError::internal("not used by this test")),
        }
    }

    async fn list(&self) -> Result<Vec<RepositoryData>, ProtocolError> {
        Err(ProtocolError::internal("not used by this test"))
    }

    async fn metadata_get(&self, _id: RepositoryId) -> Result<Hash, ProtocolError> {
        Err(ProtocolError::internal("not used by this test"))
    }

    async fn metadata_set(
        &self,
        _id: RepositoryId,
        _expected: Hash,
        _new: Hash,
    ) -> Result<MetadataSetResult, ProtocolError> {
        Err(ProtocolError::internal("not used by this test"))
    }
}

/// A repository given by its ID is looked up by ID, which an access token scoped to it can
/// authorize where a lookup by name cannot.
#[tokio::test]
async fn an_id_resolves_where_lookups_by_name_are_refused() {
    let service = Service::new(true);

    let data = resolve_on(&service, REPOSITORY)
        .await
        .expect("the repository resolves by its ID");

    assert_eq!(data.id, repository_id());
    assert_eq!(service.lookups(), [Lookup::Id(repository_id())]);
}

#[tokio::test]
async fn a_name_resolves_by_name_alone() {
    let service = Service::new(false);

    let data = resolve_on(&service, "repo")
        .await
        .expect("the repository resolves by its name");

    assert_eq!(data.id, repository_id());
    assert_eq!(service.lookups(), [Lookup::Name("repo".to_string())]);
}

/// A name that parses as an ID but names no repository by that ID is still looked up by name.
#[tokio::test]
async fn a_name_that_parses_as_an_unknown_id_is_looked_up_by_name() {
    let service = Service::new(false);
    let unknown = "ffeeddccbbaa99887766554433221100";

    let err = resolve_on(&service, unknown)
        .await
        .expect_err("no repository has that ID or name");

    assert!(err.is_not_found(), "{err:?}");
    assert_eq!(
        service.lookups(),
        [
            Lookup::Id(unknown.parse().expect("a valid repository id")),
            Lookup::Name(unknown.to_string()),
        ]
    );
}

#[tokio::test]
async fn an_unknown_name_is_not_found() {
    let service = Service::new(false);

    let err = resolve_on(&service, "missing")
        .await
        .expect_err("no repository has that name");

    assert!(err.is_not_found(), "{err:?}");
}
