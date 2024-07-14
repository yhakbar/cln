use anyhow::Error;
use clap::Parser;
use cln::cln;
use std::path::PathBuf;

/// Git clone client with a little bit of linking
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct ClnArgs {
    /// Repo to clone
    #[arg()]
    repo: String,

    /// Directory to clone into
    #[arg()]
    dir: Option<PathBuf>,

    /// Branch to checkout
    #[arg(short, long)]
    branch: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init();

    let args = ClnArgs::parse();

    let dir = args.dir;
    let branch = args.branch;
    let repo = args.repo;

    cln(&repo, dir, branch.as_deref()).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate assert_cmd;
    extern crate tempfile;

    use self::assert_cmd::Command;
    use self::tempfile::{Builder, TempDir};

    fn create_temp_dir() -> TempDir {
        Builder::new()
            .prefix("cln")
            .tempdir()
            .expect("Failed to create tempdir")
    }

    fn cln() -> Command {
        Command::cargo_bin("cln").expect("Error invoking cln")
    }

    #[test]
    fn test_cln_and_git_clone_are_equivalent() {
        let repo = "https://github.com/lua/lua.git";

        let cln_dir = create_temp_dir();
        let git_dir = create_temp_dir();

        cln()
            .args([
                repo,
                cln_dir
                    .path()
                    .to_str()
                    .expect("Failed to convert cln_dir path to string. Check the test setup."),
            ])
            .assert()
            .success();
        Command::new("git")
            .args([
                "clone",
                repo,
                git_dir
                    .path()
                    .to_str()
                    .expect("Failed to convert git_dir path to string. Check the test setup."),
            ])
            .assert()
            .success();

        for entry in git_dir.path().read_dir().expect("Failed to read git_dir") {
            let entry = entry.expect("Failed to get entry from git_dir. Check the test setup.");
            let entry_path = entry.path();
            let entry_name = entry_path
                .file_name()
                .expect("Failed to get file name from entry path in git_dir. Check the test setup.")
                .to_str()
                .expect("Failed to convert file name to string in git_dir. Check the test setup.");
            let cln_entry_path = cln_dir.path().join(entry_name);
            if entry_name == ".git" {
                continue;
            }
            assert!(cln_entry_path.exists());
        }

        for entry in cln_dir.path().read_dir().expect("Failed to read cln_dir") {
            let entry = entry.expect("Failed to get entry from cln_dir. Check the test setup.");
            let entry_path = entry.path();
            let entry_name = entry_path
                .file_name()
                .expect("Failed to get file name from entry path in cln_dir. Check the test setup.")
                .to_str()
                .expect("Failed to convert file name to string in cln_dir. Check the test setup.");
            let git_entry_path = git_dir.path().join(entry_name);
            assert!(git_entry_path.exists());
        }

        cln_dir.close().expect("Failed to close cln_dir");
        git_dir.close().expect("Failed to close git_dir");
    }
}
