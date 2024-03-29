use anyhow::Error;
use clap::Parser;
use home::home_dir;
use rayon::prelude::*;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempdir::TempDir;

/// Git clone client with a little bit of linking
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct ClnArgs {
    /// Repo to clone
    #[arg()]
    repo: String,

    /// Directory to clone into
    #[arg()]
    dir: Option<String>,

    /// Branch to checkout
    #[arg(short, long)]
    branch: Option<String>,
}

fn create_temp_dir() -> Result<TempDir, Error> {
    let tempdir = TempDir::new("cln")?;

    Ok(tempdir)
}

fn clone_repo(repo: &str, dir: &Path, branch: Option<&str>) -> Result<(), Error> {
    let mut cmd = Command::new("git");

    cmd.arg("clone")
        .arg("--bare")
        .arg("--depth")
        .arg("1")
        .arg("--single-branch");

    if let Some(branch) = branch {
        cmd.arg("--branch").arg(branch);
    };

    cmd.arg(repo).arg(dir).spawn()?.wait()?;

    Ok(())
}

fn get_cln_store_path() -> Result<String, Error> {
    if let Some(homedir) = home_dir() {
        let cln_store = homedir.join(".cln-store");
        if !cln_store.exists() {
            std::fs::create_dir(&cln_store)?;
        }
        Ok(cln_store.display().to_string())
    } else {
        Err(anyhow::anyhow!("Could not find home directory"))
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
            return Err(anyhow::anyhow!("No matching reference found"));
        }
        Ok(self.rows[0].hash.clone())
    }
}

fn run_ls_remote(repo: &str, reference: &str) -> Result<LsRemote, Error> {
    let output = Command::new("git")
        .args(["ls-remote", repo, reference])
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let stdout = stdout.trim_end();
    Ok(LsRemote::new(stdout, reference))
}

fn is_content_in_store(hash: &str) -> Result<bool, Error> {
    let store_root = get_cln_store_path()?;
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
    fn write_to_store(&self, repo_dir: &RepoPath) -> Result<(), Error> {
        let store_root = get_cln_store_path()?;
        let store_root_path = Path::new(&store_root);
        let store_path = store_root_path.join(&self.name);

        if store_path.exists() {
            return Ok(());
        }

        let store_file = File::create(&store_path)?;
        Command::new("git")
            .args(["cat-file", "-p", &self.name])
            .current_dir(repo_dir)
            .stdout(store_file)
            .output()?;
        let mut stored_file_permissions = std::fs::Permissions::from_mode(self.mode.parse()?);
        stored_file_permissions.set_readonly(true);
        File::open(&store_path)?.set_permissions(stored_file_permissions)?;

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
        let tree = std::fs::read_to_string(store_path)?;
        let tree = tree.trim_end();
        Ok(Self::new(tree, path))
    }
    fn from_hash(hash: &str, path: String) -> Result<Self, Error> {
        let store_root = get_cln_store_path()?;
        let store_root_path = Path::new(&store_root);
        let store_path = store_root_path.join(hash);
        Self::from_path(&store_path, path)
    }
}

type RepoPath = Path;

trait Walkable {
    fn walk(&self, tree: &Tree, target_path: &Path);
}

impl Walkable for RepoPath {
    fn walk(&self, tree: &Tree, target_path: &Path) {
        tree.rows
            .par_iter()
            .for_each(|row| match row.otype.as_str() {
                "blob" => {
                    row.write_to_store(self)
                        .unwrap_or_else(|_| panic!("Failed to write {} to cln-store", row.name));
                    let cur_path = Self::new(tree.path.as_str());
                    let target_dir = target_path.join(cur_path);
                    if !target_dir.exists() {
                        std::fs::create_dir_all(&target_dir).unwrap_or_else(|_| {
                            panic!("Failed to create directory {}", target_dir.display())
                        });
                    }
                    let target_file = target_dir.join(row.path.clone());
                    if target_file.exists() {
                        return;
                    }
                    std::fs::hard_link(
                        get_cln_store_path().expect("Failed to get cln-store path")
                            + "/"
                            + &row.name,
                        &target_file,
                    )
                    .unwrap_or_else(|_| {
                        panic!(
                            "Failed to hard link {} to {}",
                            row.name,
                            target_file.display()
                        )
                    });
                }
                "tree" => {
                    let cur_path = Self::new(tree.path.as_str());
                    let new_path = cur_path.join(row.path.clone());
                    let next_tree = self
                        .ls_tree(&row.name, new_path.display().to_string())
                        .unwrap_or_else(|_| panic!("Failed to `git ls-tree {}`", row.name));
                    self.walk(&next_tree, target_path);
                }
                _ => {}
            });
    }
}

type Hash = String;

impl Walkable for Hash {
    fn walk(&self, tree: &Tree, target_path: &Path) {
        let cln_store = get_cln_store_path().expect("Failed to get cln-store path");
        tree.rows
            .par_iter()
            .for_each(|row| match row.otype.as_str() {
                "blob" => {
                    let cur_path = Path::new(tree.path.as_str());
                    let target_dir = target_path.join(cur_path);
                    if !target_dir.exists() {
                        std::fs::create_dir_all(&target_dir).unwrap_or_else(|_| {
                            panic!("Failed to create directory {}", target_dir.display())
                        });
                    }
                    let target_file = target_dir.join(row.path.clone());
                    if target_file.exists() {
                        return;
                    }
                    std::fs::hard_link(cln_store.clone() + "/" + &row.name, &target_file)
                        .unwrap_or_else(|_| {
                            panic!(
                                "Failed to hard link {} to {}",
                                row.name,
                                target_file.display()
                            )
                        });
                }
                "tree" => {
                    let cur_path = Path::new(tree.path.as_str());
                    let new_path = cur_path.join(row.path.clone());
                    let next_tree = Tree::from_path(
                        &Path::new(&cln_store).join(&row.name),
                        new_path.display().to_string(),
                    )
                    .unwrap_or_else(|_| panic!("Failed to read tree {}", row.name));
                    self.walk(&next_tree, target_path);
                }
                _ => {}
            });
    }
}

trait Treevarsable {
    fn ls_tree(&self, reference: &str, path: String) -> Result<Tree, Error>;
}

const HEAD: &str = "HEAD";

impl Treevarsable for RepoPath {
    fn ls_tree(&self, reference: &str, path: String) -> Result<Tree, Error> {
        let cln_store = get_cln_store_path()?;

        let store_path = Self::new(&cln_store).join(reference);

        if store_path.exists() {
            return Ok(Tree::new(&std::fs::read_to_string(&store_path)?, path));
        }

        let ls_tree_stdout = Command::new("git")
            .args(["ls-tree", reference])
            .current_dir(self)
            .output()?
            .stdout;
        let ls_tree_string = String::from_utf8_lossy(&ls_tree_stdout);
        let ls_tree_trimmed = ls_tree_string.trim_end().to_string();

        std::fs::write(&store_path, &ls_tree_trimmed)?;

        Ok(Tree::new(&ls_tree_trimmed, path))
    }
}

fn get_repo_name(repo: &str) -> String {
    let repo_name = repo
        .split('/')
        .last()
        .expect("Could not parse repo name. Check the URL.");
    repo_name.replace(".git", "")
}

fn main() -> Result<(), Error> {
    let args = ClnArgs::parse();

    let target_dir = if let Some(dir) = &args.dir {
        dir.to_string()
    } else {
        get_repo_name(&args.repo)
    };

    let remote_ref = args.branch.as_ref().map_or(HEAD, |branch| branch.as_str());
    let ls_remote = run_ls_remote(&args.repo, remote_ref)?;
    let ls_remote_hash = ls_remote.get_hash()?;

    if is_content_in_store(&ls_remote_hash)? {
        let head_tree = Tree::from_hash(&ls_remote_hash, ".".to_string())?;
        if !Path::new(&target_dir).exists() {
            std::fs::create_dir(&target_dir)?;
        }
        ls_remote_hash.walk(&head_tree, Path::new(&target_dir));

        return Ok(());
    }

    let tmp_dir = create_temp_dir()?;
    let tmp_dir_path = tmp_dir.path();

    clone_repo(&args.repo, tmp_dir_path, args.branch.as_deref())?;

    let head_tree = tmp_dir_path.ls_tree(&ls_remote_hash, ".".to_string())?;

    if !Path::new(&target_dir).exists() {
        std::fs::create_dir(&target_dir)?;
    }
    tmp_dir_path.walk(&head_tree, Path::new(&target_dir));

    tmp_dir.close()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_cmd::Command;

    fn cln() -> Command {
        Command::cargo_bin(env!("CARGO_PKG_NAME")).expect("Error invoking cln")
    }

    #[test]
    fn test_create_temp_dir() {
        let tempdir = create_temp_dir().unwrap();
        assert!(tempdir.path().exists());
        tempdir.close().unwrap();
    }

    #[test]
    fn test_get_cln_store_path() {
        let cln_store = get_cln_store_path().unwrap();
        assert!(Path::new(&cln_store).exists());
    }

    #[test]
    fn test_run_ls_remote() {
        let repo = "https://github.com/lua/lua.git";
        let reference = "HEAD";
        let ls_remote = run_ls_remote(repo, reference).unwrap();
        assert!(!ls_remote.rows.is_empty());
    }

    #[test]
    fn test_cln_and_git_clone_are_equivalent() {
        let repo = "https://github.com/lua/lua.git";

        let cln_dir = create_temp_dir().unwrap();
        let git_dir = create_temp_dir().unwrap();

        cln().args(&[repo, cln_dir.path().to_str().unwrap()]).assert().success();
        Command::new("git")
            .args(["clone", repo, git_dir.path().to_str().unwrap()])
            .assert()
            .success();

        for entry in git_dir.path().read_dir().unwrap() {
            let entry = entry.unwrap();
            let entry_path = entry.path();
            let entry_name = entry_path.file_name().unwrap().to_str().unwrap();
            let cln_entry_path = cln_dir.path().join(entry_name);
            if entry_name == ".git" {
                continue;
            }
            assert!(cln_entry_path.exists());
        }

        for entry in cln_dir.path().read_dir().unwrap() {
            let entry = entry.unwrap();
            let entry_path = entry.path();
            let entry_name = entry_path.file_name().unwrap().to_str().unwrap();
            let git_entry_path = git_dir.path().join(entry_name);
            assert!(git_entry_path.exists());
        }

        cln_dir.close().unwrap();
        git_dir.close().unwrap();
    }
}
