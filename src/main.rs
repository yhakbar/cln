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
}

trait Walkable {
    fn walk(&self, path: &RepoPath, target_path: &Path);
}

impl Walkable for Tree {
    fn walk(&self, path: &RepoPath, target_path: &Path) {
        self.rows
            .par_iter()
            .for_each(|row| match row.otype.as_str() {
                "blob" => {
                    row.write_to_store(path)
                        .unwrap_or_else(|_| panic!("Failed to write {} to cln-store", row.name));
                    let cur_path = Path::new(self.path.as_str());
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
                    let cur_path = Path::new(self.path.as_str());
                    let new_path = cur_path.join(row.path.clone());
                    let next_tree = path
                        .ls_tree(&row.name, new_path.display().to_string())
                        .unwrap_or_else(|_| panic!("Failed to `git ls-tree {}`", row.name));
                    next_tree.walk(path, target_path);
                }
                _ => {}
            });
    }
}

type RepoPath = Path;

trait Treevarsable {
    fn ls_tree(&self, reference: &str, path: String) -> Result<Tree, Error>;
    fn ls_head_tree(&self) -> Result<Tree, Error>;
}

impl Treevarsable for RepoPath {
    fn ls_tree(&self, reference: &str, path: String) -> Result<Tree, Error> {
        let ls_tree_stdout = Command::new("git")
            .args(["ls-tree", reference])
            .current_dir(self)
            .output()?
            .stdout;
        let ls_tree_string = String::from_utf8_lossy(&ls_tree_stdout);
        let ls_tree_trimmed = ls_tree_string.trim_end().to_string();
        Ok(Tree::new(&ls_tree_trimmed, path))
    }
    fn ls_head_tree(&self) -> Result<Tree, Error> {
        self.ls_tree("HEAD^{tree}", ".".to_string())
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
        if dir.contains('/') {
            anyhow::bail!("Target directory cannot contain a slash (/) character.");
        }

        dir.to_string()
    } else {
        get_repo_name(&args.repo)
    };

    if Path::new(&target_dir).exists() {
        anyhow::bail!("Target directory {} already exists.", target_dir);
    }

    let tmp_dir = create_temp_dir()?;
    let tmp_dir_path = tmp_dir.path();

    clone_repo(&args.repo, tmp_dir_path, args.branch.as_deref())?;

    std::fs::create_dir_all(&target_dir)?;

    let head_tree = tmp_dir_path.ls_head_tree()?;
    head_tree.walk(tmp_dir_path, Path::new(&target_dir));

    tmp_dir.close()?;
    Ok(())
}
