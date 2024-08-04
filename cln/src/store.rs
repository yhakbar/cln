use crate::Error;
use home::home_dir;
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::create_dir_all;
use tokio::sync::Mutex;

pub static STORE_PATH: Lazy<Arc<Mutex<PathBuf>>> =
    Lazy::new(|| Arc::new(Mutex::new(PathBuf::from(".cln-store"))));

pub async fn ensure_cln_store_path(store_path: Option<PathBuf>) -> Result<(), Error> {
    if let Some(store_path) = store_path {
        *STORE_PATH.lock().await = store_path;

        return Ok(());
    }

    if let Some(homedir) = home_dir() {
        let cln_store = homedir.join(".cache").join(".cln-store");
        if !cln_store.exists() {
            create_dir_all(&cln_store)
                .await
                .map_err(Error::CreateDirError)?;
        }

        *STORE_PATH.lock().await = cln_store;

        Ok(())
    } else {
        Err(Error::HomeDirError)
    }
}

pub async fn is_content_stored(hash: &str) -> Result<bool, Error> {
    let store_path = STORE_PATH.lock().await.clone();
    let content_path = store_path.join(hash);
    Ok(content_path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::Builder;

    #[tokio::test]
    async fn test_get_cln_store_path() {
        let tempdir = Builder::new()
            .prefix("cln")
            .tempdir()
            .expect("Failed to create tempdir");

        ensure_cln_store_path(Some(tempdir.path().to_path_buf()))
            .await
            .expect("Failed to ensure cln-store path");

        let store_path = STORE_PATH.lock().await.clone();

        assert!(store_path.exists());
    }
}
