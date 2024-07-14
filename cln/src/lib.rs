use async_trait::async_trait;
use home::home_dir;
use log::debug;
use rayon::prelude::*;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tempfile::{Builder, TempDir};
use thiserror::Error as ThisError;
use tokio::{
    fs::{create_dir, create_dir_all, hard_link, read_to_string, write, File},
    process::Command,
};

/// Clns a git repository into a given directory.
/// If no directory is given, the repository will be cloned into a directory with the same name as the repository.
/// If no branch is given, the repository will be cloned at HEAD.
/// If the repository is already in the cln-store, it will be copied from there.
/// If the repository is not in the cln-store, it will be cloned into a temporary directory and copied from there.
///
/// # Examples
/// ```rust
/// use cln::cln;
/// use tempfile::Builder;
///
/// #[tokio::main]
/// async fn main() {
///     let tempdir = Builder::new()
///        .prefix("cln")
///        .tempdir()
///        .unwrap();
///
///     let path = tempdir.into_path();
///
///     cln("https://github.com/yhakbar/cln.git", Some(path), None).await.unwrap();
/// }
/// ```
///
/// # Errors
/// Will return an error if the repository cannot be clned.
/// This can happen if:
/// - The tempdir where the repository is cloned cannot be created.
/// - The git command to clone the repository into the tempdir fails.
/// - The new directory where the repository is copied to cannot be created.
/// - The temporary directory cannot be persisted to the cln-store.
/// - The hard links from the cln-store to the new directory fail.
pub async fn cln(repo: &str, dir: Option<PathBuf>, branch: Option<&str>) -> Result<(), Error> {
    let target_dir = dir.map_or_else(|| get_repo_name(repo), |dir| dir);
    let remote_ref = branch.as_ref().map_or(HEAD, |branch| branch);
    let ls_remote = run_ls_remote(repo, remote_ref).await?;
    let ls_remote_hash = ls_remote.get_hash()?;

    if is_content_in_store(&ls_remote_hash).await? {
        let head_tree = Tree::from_hash(&ls_remote_hash, ".".to_string()).await?;
        if !&target_dir.exists() {
            create_dir(&target_dir)
                .await
                .map_err(Error::CreateDirError)?;
        }
        ls_remote_hash.walk(&head_tree, &target_dir).await?;

        return Ok(());
    }

    let tmp_dir = create_temp_dir()?;
    let tmp_dir_path = tmp_dir.path();

    clone_repo(repo, tmp_dir_path, branch).await?;

    let head_tree = tmp_dir_path
        .ls_tree(&ls_remote_hash, ".".to_string())
        .await?;

    if !Path::new(&target_dir).exists() {
        create_dir(&target_dir)
            .await
            .map_err(Error::CreateDirError)?;
    }
    tmp_dir_path
        .walk(&head_tree, Path::new(&target_dir))
        .await?;

    tmp_dir.close().map_err(Error::TempDirCloseError)?;

    Ok(())
}

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("Failed to create tempdir: {0}")]
    TempDirError(std::io::Error),
    #[error("Failed to close tempdir: {0}")]
    TempDirCloseError(std::io::Error),
    #[error("Failed to spawn git command: {0}")]
    CommandSpawnError(std::io::Error),
    #[error("Failed to complete git clone: {0}")]
    GitCloneError(String),
    #[error("Failed to parse git command output: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
    #[error("Failed to create cln-store directory: {0}")]
    CreateDirError(std::io::Error),
    #[error("Failed to find home directory")]
    HomeDirError,
    #[error("No matching reference found")]
    NoMatchingReferenceError,
    #[error("Failed to write to cln-store: {0}")]
    WriteToStoreError(std::io::Error),
    #[error("Failed to create directory: {0}")]
    CreateDirAllError(std::io::Error),
    #[error("Failed to hard link: {0}")]
    HardLinkError(std::io::Error),
    #[error("Failed to read tree: {0}")]
    ReadTreeError(std::io::Error),
    #[error("Parse mode error: {0}")]
    ParseModeError(std::num::ParseIntError),
}

fn create_temp_dir() -> Result<TempDir, Error> {
    let tempdir = Builder::new()
        .prefix("cln")
        .tempdir()
        .map_err(Error::TempDirError)?;

    Ok(tempdir)
}

async fn clone_repo(repo: &str, dir: &Path, branch: Option<&str>) -> Result<(), Error> {
    let mut cmd = Command::new("git");

    cmd.arg("clone")
        .arg("--bare")
        .arg("--depth")
        .arg("1")
        .arg("--single-branch");

    if let Some(branch) = branch {
        cmd.arg("--branch").arg(branch);
    };

    let out = cmd
        .arg(repo)
        .arg(dir)
        .output()
        .await
        .map_err(Error::CommandSpawnError)?;

    if !out.status.success() {
        return Err(Error::GitCloneError(
            String::from_utf8_lossy(&out.stderr).to_string(),
        ));
    }

    Ok(())
}

async fn get_cln_store_path() -> Result<String, Error> {
    if let Some(homedir) = home_dir() {
        let cln_store = homedir.join(".cln-store");
        if !cln_store.exists() {
            create_dir(&cln_store)
                .await
                .map_err(Error::CreateDirError)?;
        }
        Ok(cln_store.display().to_string())
    } else {
        Err(Error::HomeDirError)
    }
}

struct LsRemoteRow {
    hash: String,
    name: String,
}

impl LsRemoteRow {
    fn new(row: &str) -> Self {
        let mut row_iter = row.split_whitespace();
        let hash = row_iter
            .next()
            .expect("Failed to find hash in LsRemoteRow")
            .to_string();
        let name = row_iter.collect::<Vec<&str>>().join(" ");
        Self { hash, name }
    }
}

struct LsRemote {
    rows: Vec<LsRemoteRow>,
}

impl LsRemote {
    fn new(ls_remote: &str, reference: &str) -> Self {
        let rows = ls_remote
            .lines()
            .par_bridge()
            .map(LsRemoteRow::new)
            .filter(|row| match row.name.as_str() {
                _ if row.name == reference => true,
                _ if row.name == format!("refs/tags/{reference}") => true,
                _ if row.name == format!("refs/heads/{reference}") => true,
                _ => false,
            })
            .collect::<Vec<LsRemoteRow>>();
        Self { rows }
    }
    fn get_hash(&self) -> Result<String, Error> {
        if self.rows.is_empty() {
            return Err(Error::NoMatchingReferenceError);
        }
        Ok(self.rows[0].hash.clone())
    }
}

async fn run_ls_remote(repo: &str, reference: &str) -> Result<LsRemote, Error> {
    let output = Command::new("git")
        .args(["ls-remote", repo, reference])
        .output()
        .await
        .map_err(Error::CommandSpawnError)?;
    let stdout = String::from_utf8(output.stdout)?;
    let stdout = stdout.trim_end();
    Ok(LsRemote::new(stdout, reference))
}

async fn is_content_in_store(hash: &str) -> Result<bool, Error> {
    let store_root = get_cln_store_path().await?;
    let store_root_path = Path::new(&store_root);
    let store_path = store_root_path.join(hash);
    Ok(store_path.exists())
}

// Struct for parsing the rows of stdout from the `git ls-tree` command
struct TreeRow {
    mode: String,
    otype: String,
    name: String,
    path: String,
}

impl TreeRow {
    fn new(row: &str) -> Self {
        let mut row_iter = row.split_whitespace();
        let mode = row_iter
            .next()
            .expect("Failed to find mode in TreeRow")
            .to_string();
        let otype = row_iter
            .next()
            .expect("Failed to find otype in TreeRow")
            .to_string();
        let name = row_iter
            .next()
            .expect("Failed to find name in TreeRow")
            .to_string();
        let path = row_iter.collect::<Vec<&str>>().join(" ");
        Self {
            mode,
            otype,
            name,
            path,
        }
    }
    async fn write_to_store(&self, repo_dir: &RepoPath) -> Result<(), Error> {
        let store_root = get_cln_store_path().await?;
        let store_root_path = Path::new(&store_root);
        let store_path = store_root_path.join(&self.name);

        if store_path.exists() {
            return Ok(());
        }

        debug!(
            "Writing blob {} to store path {}",
            self.name,
            store_path.display()
        );

        let output = Command::new("git")
            .args(["cat-file", "-p", &self.name])
            .current_dir(repo_dir)
            .output()
            .await
            .map_err(Error::CommandSpawnError)?;
        write(&store_path, &output.stdout)
            .await
            .map_err(Error::WriteToStoreError)?;
        let mut stored_file_permissions =
            std::fs::Permissions::from_mode(self.mode.parse().map_err(Error::ParseModeError)?);
        stored_file_permissions.set_readonly(true);
        File::open(&store_path)
            .await
            .map_err(Error::ReadTreeError)?
            .set_permissions(stored_file_permissions)
            .await
            .map_err(Error::ReadTreeError)?;

        Ok(())
    }
}

struct Tree {
    rows: Vec<TreeRow>,
    path: String,
}

impl Tree {
    fn new(tree: &str, path: String) -> Self {
        let rows = tree
            .lines()
            .par_bridge()
            .map(TreeRow::new)
            .collect::<Vec<TreeRow>>();
        Self { rows, path }
    }
    fn from_path(store_path: &Path, path: String) -> Result<Self, Error> {
        let tree = std::fs::read_to_string(store_path).map_err(Error::ReadTreeError)?;
        let tree = tree.trim_end();
        Ok(Self::new(tree, path))
    }
    async fn from_hash(hash: &str, path: String) -> Result<Self, Error> {
        let store_root = get_cln_store_path().await?;
        let store_root_path = Path::new(&store_root);
        let store_path = store_root_path.join(hash);
        Self::from_path(&store_path, path)
    }
}

type RepoPath = Path;

#[async_trait]
trait Walkable {
    async fn walk(&self, tree: &Tree, target_path: &Path) -> Result<(), Error>;
    async fn write_blob(&self, tree: &Tree, row: &TreeRow, target_path: &Path)
        -> Result<(), Error>;
    async fn walk_tree(&self, tree: &Tree, row: &TreeRow, target_path: &Path) -> Result<(), Error>;
}

#[async_trait]
impl Walkable for RepoPath {
    async fn walk(&self, tree: &Tree, target_path: &Path) -> Result<(), Error> {
        let mut blob_tasks = vec![];
        let mut tree_tasks = vec![];

        for row in &tree.rows {
            match row.otype.as_str() {
                "blob" => {
                    blob_tasks.push(async move { self.write_blob(tree, row, target_path).await });
                }
                "tree" => {
                    tree_tasks.push(async move { self.walk_tree(tree, row, target_path).await });
                }
                _ => {}
            }
        }

        for task in blob_tasks {
            task.await?;
        }

        for task in tree_tasks {
            task.await?;
        }

        Ok(())
    }
    async fn write_blob(
        &self,
        tree: &Tree,
        row: &TreeRow,
        target_path: &Path,
    ) -> Result<(), Error> {
        row.write_to_store(self).await?;
        let cur_path = Self::new(tree.path.as_str());
        let target_dir = target_path.join(cur_path);
        if !target_dir.exists() {
            create_dir_all(&target_dir)
                .await
                .map_err(Error::CreateDirAllError)?;
        }
        let target_file = target_dir.join(row.path.clone());
        if target_file.exists() {
            return Ok(());
        }
        let store_path = PathBuf::from(get_cln_store_path().await?).join(&row.name);
        hard_link(store_path.clone(), &target_file)
            .await
            .map_err(Error::HardLinkError)?;

        debug!(
            "Linked {} to {}",
            store_path.display(),
            target_file.display()
        );

        Ok(())
    }
    async fn walk_tree(&self, tree: &Tree, row: &TreeRow, target_path: &Path) -> Result<(), Error> {
        let cur_path = Self::new(tree.path.as_str());
        let new_path = cur_path.join(row.path.clone());
        let next_tree = self
            .ls_tree(&row.name, new_path.display().to_string())
            .await?;
        self.walk(&next_tree, target_path).await?;

        Ok(())
    }
}

type Hash = String;

#[async_trait]
impl Walkable for Hash {
    async fn walk(&self, tree: &Tree, target_path: &Path) -> Result<(), Error> {
        let mut blob_tasks = vec![];
        let mut tree_tasks = vec![];

        for row in &tree.rows {
            match row.otype.as_str() {
                "blob" => {
                    blob_tasks.push(async move { self.write_blob(tree, row, target_path).await });
                }
                "tree" => {
                    tree_tasks.push(async move { self.walk_tree(tree, row, target_path).await });
                }
                _ => {}
            }
        }

        for task in blob_tasks {
            task.await?;
        }

        for task in tree_tasks {
            task.await?;
        }

        Ok(())
    }
    async fn write_blob(
        &self,
        tree: &Tree,
        row: &TreeRow,
        target_path: &Path,
    ) -> Result<(), Error> {
        let cur_path = Path::new(tree.path.as_str());
        let target_dir = target_path.join(cur_path);
        if !target_dir.exists() {
            create_dir_all(&target_dir)
                .await
                .map_err(Error::CreateDirAllError)?;
        }
        let target_file = target_dir.join(row.path.clone());
        if target_file.exists() {
            return Ok(());
        }
        let store_path = PathBuf::from(get_cln_store_path().await?).join(&row.name);
        hard_link(store_path.clone(), &target_file)
            .await
            .map_err(Error::HardLinkError)?;

        debug!(
            "Linked {} to {}",
            store_path.display(),
            target_file.display()
        );

        Ok(())
    }
    async fn walk_tree(&self, tree: &Tree, row: &TreeRow, target_path: &Path) -> Result<(), Error> {
        let cur_path = Path::new(tree.path.as_str());
        let new_path = cur_path.join(row.path.clone());
        let next_tree = Tree::from_hash(&row.name, new_path.display().to_string()).await?;
        self.walk(&next_tree, target_path).await?;

        Ok(())
    }
}

trait Treevarsable {
    async fn ls_tree(&self, reference: &str, path: String) -> Result<Tree, Error>;
}

const HEAD: &str = "HEAD";

impl Treevarsable for RepoPath {
    async fn ls_tree(&self, reference: &str, path: String) -> Result<Tree, Error> {
        debug!("ls-tree: {}", reference);

        let cln_store = get_cln_store_path().await?;

        let store_path = Self::new(&cln_store).join(reference);

        if store_path.exists() {
            return Ok(Tree::new(
                &read_to_string(&store_path)
                    .await
                    .map_err(Error::ReadTreeError)?,
                path,
            ));
        }

        let ls_tree_stdout = Command::new("git")
            .args(["ls-tree", reference])
            .current_dir(self)
            .output()
            .await
            .map_err(Error::CommandSpawnError)?
            .stdout;
        let ls_tree_string = String::from_utf8_lossy(&ls_tree_stdout);
        let ls_tree_trimmed = ls_tree_string.trim_end().to_string();

        write(&store_path, &ls_tree_trimmed)
            .await
            .map_err(Error::WriteToStoreError)?;

        debug!("Wrote to store: {}", store_path.display());

        Ok(Tree::new(&ls_tree_trimmed, path))
    }
}

fn get_repo_name(repo: &str) -> PathBuf {
    let repo_name = repo
        .split('/')
        .last()
        .expect("Could not parse repo name. Check the URL.");
    PathBuf::from(repo_name.replace(".git", ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_temp_dir() {
        let tempdir = create_temp_dir().expect("Failed to create tempdir");
        assert!(tempdir.path().exists());
        tempdir.close().expect("Failed to close tempdir");
    }

    #[tokio::test]
    async fn test_get_cln_store_path() {
        let cln_store = get_cln_store_path()
            .await
            .expect("Failed to get cln-store path");
        assert!(Path::new(&cln_store).exists());
    }

    #[tokio::test]
    async fn test_run_ls_remote() {
        let repo = "https://github.com/lua/lua.git";
        let reference = "HEAD";
        let ls_remote = run_ls_remote(repo, reference)
            .await
            .expect("Failed to run ls-remote");
        assert!(!ls_remote.rows.is_empty());
    }

    #[tokio::test]
    async fn test_clone_repo() {
        let repo = "https://github.com/lua/lua.git";
        let tmp_dir = create_temp_dir().expect("Failed to create tempdir");
        let tmp_dir_path = tmp_dir.path();
        clone_repo(repo, tmp_dir_path, None)
            .await
            .expect("Failed to clone repo");
        assert!(tmp_dir_path.join("HEAD").exists());

        for entry in tmp_dir_path.read_dir().expect("Failed to read tempdir") {
            let entry = entry.expect("Failed to read entry");
            let entry_path = entry.path();
            let filesize = entry_path.metadata().expect("Failed to get metadata").len();
            assert!(filesize > 0);
            assert!(entry_path.exists());
        }

        tmp_dir.close().expect("Failed to close tempdir");
    }
}
