use anyhow::{anyhow, Result};
use git2::{
    build::CheckoutBuilder, AutotagOption, ErrorClass, ErrorCode, FetchOptions, Oid, Repository,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub repo_dir: PathBuf,
    pub commit: String,
}

#[derive(thiserror::Error, Debug)]
pub enum GitSyncError {
    #[error("network failure while accessing repository: {0}")]
    Network(String),
    #[error("repository not found or inaccessible: {0}")]
    RepoNotFound(String),
    #[error("requested ref not found: {0}")]
    RefNotFound(String),
    #[error("checkout failed for ref {0}: {1}")]
    CheckoutFailed(String, String),
    #[error("local repository cache is corrupted: {0}")]
    CacheCorrupted(String),
    #[error("git operation failed: {0}")]
    Other(String),
}

pub fn sync(url: &str, repo_dir: &Path, r#ref: Option<&str>) -> Result<SyncResult> {
    let requested_ref = r#ref.unwrap_or("HEAD");
    let repo = if repo_dir.exists() {
        open_or_reclone(repo_dir, url)?
    } else {
        clone_repo(url, repo_dir)?
    };

    fetch_repo(&repo, url)?;
    let resolved = resolve_ref(&repo, requested_ref)?;
    checkout_commit(&repo, resolved, requested_ref)?;

    Ok(SyncResult {
        repo_dir: repo_dir.to_path_buf(),
        commit: resolved.to_string(),
    })
}

fn open_or_reclone(repo_dir: &Path, url: &str) -> Result<Repository> {
    match Repository::open(repo_dir) {
        Ok(repo) => Ok(repo),
        Err(e) => {
            std::fs::remove_dir_all(repo_dir).map_err(|ioe| {
                anyhow!(GitSyncError::CacheCorrupted(format!(
                    "{}; failed to remove corrupted cache: {}",
                    e.message(),
                    ioe
                )))
            })?;
            clone_repo(url, repo_dir)
        }
    }
}

fn clone_repo(url: &str, repo_dir: &Path) -> Result<Repository> {
    if let Some(parent) = repo_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Repository::clone(url, repo_dir).map_err(map_git2_err)
}

fn fetch_repo(repo: &Repository, url: &str) -> Result<()> {
    let mut remote = match repo.find_remote("origin") {
        Ok(r) => r,
        Err(_) => repo.remote_anonymous(url).map_err(map_git2_err)?,
    };
    let mut fo = FetchOptions::new();
    fo.download_tags(AutotagOption::All);
    remote
        .fetch(
            &[
                "+refs/heads/*:refs/remotes/origin/*",
                "+refs/tags/*:refs/tags/*",
                "+HEAD:refs/remotes/origin/HEAD",
            ],
            Some(&mut fo),
            None,
        )
        .map_err(map_git2_err)?;
    Ok(())
}

fn resolve_ref(repo: &Repository, ref_name: &str) -> Result<Oid> {
    if ref_name == "HEAD" {
        return resolve_commit(repo, "refs/remotes/origin/HEAD")
            .or_else(|| {
                repo.head()
                    .ok()?
                    .peel_to_commit()
                    .ok()
                    .map(|commit| commit.id())
            })
            .ok_or_else(|| anyhow!(GitSyncError::RefNotFound(ref_name.to_string())));
    }

    if let Ok(oid) = Oid::from_str(ref_name) {
        if let Ok(object) = repo.find_object(oid, None) {
            if let Ok(commit) = object.peel_to_commit() {
                return Ok(commit.id());
            }
        }
    }

    let candidates = if ref_name.starts_with("refs/") {
        vec![ref_name.to_string()]
    } else {
        vec![
            format!("refs/remotes/origin/{ref_name}"),
            format!("refs/tags/{ref_name}"),
            format!("refs/heads/{ref_name}"),
            ref_name.to_string(),
        ]
    };

    for cand in candidates {
        if let Some(oid) = resolve_commit(repo, &cand) {
            return Ok(oid);
        }
    }

    Err(anyhow!(GitSyncError::RefNotFound(ref_name.to_string())))
}

fn resolve_commit(repo: &Repository, ref_name: &str) -> Option<Oid> {
    repo.revparse_single(ref_name)
        .ok()?
        .peel_to_commit()
        .ok()
        .map(|commit| commit.id())
}

fn checkout_commit(repo: &Repository, oid: Oid, ref_name: &str) -> Result<()> {
    let obj = repo.find_object(oid, None).map_err(map_git2_err)?;
    let mut opts = CheckoutBuilder::new();
    opts.force();
    repo.checkout_tree(&obj, Some(&mut opts)).map_err(|e| {
        anyhow!(GitSyncError::CheckoutFailed(
            ref_name.to_string(),
            e.message().to_string()
        ))
    })?;
    repo.set_head_detached(oid).map_err(|e| {
        anyhow!(GitSyncError::CheckoutFailed(
            ref_name.to_string(),
            e.message().to_string()
        ))
    })?;
    Ok(())
}

fn map_git2_err(err: git2::Error) -> anyhow::Error {
    let msg = err.message().to_string();
    let mapped = match (err.class(), err.code()) {
        (ErrorClass::Net, _) | (_, ErrorCode::Certificate) | (_, ErrorCode::Auth) => {
            GitSyncError::Network(msg)
        }
        (_, ErrorCode::NotFound)
            if msg.contains("Repository not found") || msg.contains("not found") =>
        {
            GitSyncError::RepoNotFound(msg)
        }
        _ if msg.contains("Could not resolve host")
            || msg.contains("timed out")
            || msg.contains("connection")
            || msg.contains("TLS") =>
        {
            GitSyncError::Network(msg)
        }
        _ => GitSyncError::Other(msg),
    };
    anyhow!(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn commit_file(repo: &Repository, workdir: &Path, name: &str, body: &str, msg: &str) -> Oid {
        fs::write(workdir.join(name), body).expect("write file");
        let mut idx = repo.index().expect("index");
        idx.add_path(Path::new(name)).expect("add path");
        idx.write().expect("idx write");
        let tree_id = idx.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let sig = git2::Signature::now("t", "t@t.com").expect("sig");
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|id| repo.find_commit(id).ok());
        match parent {
            Some(p) => repo
                .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&p])
                .expect("commit"),
            None => repo
                .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[])
                .expect("commit"),
        }
    }

    #[test]
    fn resolve_ref_supports_commit_branch_tag() {
        let td = tempfile::tempdir().expect("tempdir");
        let repo = Repository::init(td.path()).expect("init");
        repo.set_head("refs/heads/master").expect("set master");
        let oid = commit_file(&repo, td.path(), "a.txt", "a", "init");
        let c = repo.find_commit(oid).expect("find commit");
        repo.tag_lightweight("v1", c.as_object(), false)
            .expect("tag");
        let sig = git2::Signature::now("t", "t@t.com").expect("sig");
        let annotated_tag = repo
            .tag("v2", c.as_object(), &sig, "annotated", false)
            .expect("annotated tag");

        assert_eq!(resolve_ref(&repo, "HEAD").expect("head"), oid);
        assert_eq!(resolve_ref(&repo, "master").expect("master"), oid);
        assert_eq!(resolve_ref(&repo, "v1").expect("tag"), oid);
        assert_ne!(annotated_tag, oid);
        assert_eq!(resolve_ref(&repo, "v2").expect("annotated tag"), oid);
        assert_eq!(resolve_ref(&repo, &oid.to_string()).expect("oid"), oid);
    }

    #[test]
    fn sync_clones_and_fetches_with_existing_cache() {
        let remote_dir = tempfile::tempdir().expect("remote tempdir");
        let remote_bare = remote_dir.path().join("remote.git");
        let remote = Repository::init_bare(&remote_bare).expect("init bare");
        remote.set_head("refs/heads/master").expect("remote master");

        let src_dir = tempfile::tempdir().expect("src tempdir");
        let src = Repository::init(src_dir.path()).expect("init src");
        src.set_head("refs/heads/master").expect("set master");
        let first = commit_file(&src, src_dir.path(), "SKILL.md", "# demo", "first");
        let mut r = src
            .remote("origin", remote_bare.to_str().expect("path str"))
            .expect("remote add");
        r.push(&["refs/heads/master:refs/heads/master"], None)
            .expect("push 1");

        let cache_dir = tempfile::tempdir().expect("cache");
        let repo_cache = cache_dir.path().join("repo");
        let url = remote_bare.to_string_lossy().to_string();
        let first_sync = sync(&url, &repo_cache, Some("HEAD")).expect("first sync");
        assert_eq!(first_sync.commit, first.to_string());

        let second = commit_file(&src, src_dir.path(), "README.md", "hi", "second");
        let mut r2 = src.find_remote("origin").expect("origin");
        r2.push(&["refs/heads/master:refs/heads/master"], None)
            .expect("push 2");

        let second_sync = sync(&url, &repo_cache, Some("HEAD")).expect("second sync");
        assert_ne!(second_sync.commit, first.to_string());
        assert_eq!(second_sync.commit, second.to_string());

        let branch_sync = sync(&url, &repo_cache, Some("master")).expect("branch sync");
        assert_eq!(branch_sync.commit, second.to_string());
    }
}
