// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! Server-owned branch guards and revision-scoped landing identities.
use std::sync::OnceLock;

use lore_base::types::Hash;
use lore_revision::lore::BranchId;
use lore_revision::lore::RepositoryId;
use serde::Deserialize;
use tonic::Status;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    repository: String,
    branch: String,
    name: String,
}

fn guards() -> Result<&'static [Guard], Status> {
    static GUARDS: OnceLock<Result<Vec<Guard>, String>> = OnceLock::new();
    GUARDS
        .get_or_init(|| {
            let value = match std::env::var("LORE_BRANCH_GUARDS") {
                Ok(value) => value,
                Err(std::env::VarError::NotPresent) => return Ok(vec![]),
                Err(_) => return Err("invalid branch guard configuration".into()),
            };
            let guards: Vec<Guard> = serde_json::from_str(&value).map_err(|e| e.to_string())?;
            for guard in &guards {
                if !valid_id(&guard.repository)
                    || !valid_id(&guard.branch)
                    || guard.name.is_empty()
                    || guard.name.contains(':')
                    || guard.name.chars().any(char::is_control)
                {
                    return Err("invalid branch guard configuration".into());
                }
            }
            Ok(guards)
        })
        .as_ref()
        .map(|g| g.as_slice())
        .map_err(|_| Status::failed_precondition("Invalid server branch guard configuration"))
}
fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn denied() -> Status {
    Status::permission_denied(
        "Protected branch: submit a change request and satisfy its review and checks",
    )
}

pub fn check_push(
    repository: RepositoryId,
    branch: BranchId,
    revision: Hash,
    user: &str,
    force: bool,
    merge: bool,
) -> Result<(), Status> {
    for guard in guards()? {
        if guard.repository == repository.to_string() && guard.branch == branch.to_string() {
            return guard.allow_push(&revision.to_string(), user, force, merge);
        }
    }
    Ok(())
}
impl Guard {
    fn allow_push(
        &self,
        revision: &str,
        user: &str,
        force: bool,
        merge: bool,
    ) -> Result<(), Status> {
        let expected = format!("landing:{}:{}:{}", self.repository, self.name, revision);
        if force || merge || user != expected {
            return Err(denied());
        }
        Ok(())
    }
}

pub fn check_branch(
    repository: RepositoryId,
    branch: BranchId,
    name: Option<&str>,
) -> Result<(), Status> {
    for guard in guards()? {
        if guard.repository == repository.to_string()
            && (guard.branch == branch.to_string() || name == Some(guard.name.as_str()))
        {
            return Err(denied());
        }
    }
    Ok(())
}

// Raw mutation APIs cannot bypass the revision service's guards, even with a landing identity.
pub fn check_repository_mutation(repository: RepositoryId) -> Result<(), Status> {
    if guards()?
        .iter()
        .any(|g| g.repository == repository.to_string())
    {
        return Err(denied());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn landing_is_bound_to_repository_branch_and_revision() {
        let g = Guard {
            repository: "a".repeat(32),
            branch: "b".repeat(32),
            name: "main".into(),
        };
        let revision = "c".repeat(64);
        let subject = format!("landing:{}:main:{}", g.repository, revision);
        assert!(g.allow_push(&revision, &subject, false, false).is_ok());
        for subject in [
            "human".to_string(),
            "agent:1".to_string(),
            subject.replace("main", "other"),
            subject.replace(&g.repository, &"d".repeat(32)),
            subject.replace(&revision, &"d".repeat(64)),
        ] {
            assert!(g.allow_push(&revision, &subject, false, false).is_err());
        }
        assert!(g.allow_push(&revision, &subject, true, false).is_err());
        assert!(g.allow_push(&revision, &subject, false, true).is_err());
    }
    #[test]
    fn identifiers_are_canonical() {
        assert!(valid_id(&"a".repeat(32)));
        for s in ["", "../main", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "a"] {
            assert!(!valid_id(s));
        }
    }
    #[test]
    fn configured_guards_cover_mutation_paths() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "branch_guard::tests::configured_guard_child",
                "--nocapture",
            ])
            .env(
                "LORE_BRANCH_GUARDS",
                format!(
                    r#"[{{"repository":"{}","branch":"{}","name":"main"}}]"#,
                    "a".repeat(32),
                    "b".repeat(32)
                ),
            )
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[test]
    fn configured_guard_child() {
        if std::env::var_os("LORE_BRANCH_GUARDS").is_none() {
            return;
        }
        let repo: RepositoryId = "a".repeat(32).parse().unwrap();
        let branch: BranchId = "b".repeat(32).parse().unwrap();
        let other: BranchId = "d".repeat(32).parse().unwrap();
        let revision = Hash::hash_buffer(b"candidate");
        assert!(check_branch(repo, branch, None).is_err());
        assert!(check_branch(repo, other, Some("main")).is_err());
        assert!(check_branch(repo, other, Some("feature")).is_ok());
        assert!(check_repository_mutation(repo).is_err());
        assert!(check_repository_mutation("d".repeat(32).parse().unwrap()).is_ok());
        assert!(check_push(repo, branch, revision, "human", false, false).is_err());
        assert!(
            check_push(
                repo,
                branch,
                revision,
                &format!("landing:{repo}:main:{revision}"),
                false,
                false
            )
            .is_ok()
        );
    }
}
