use std::{
    io,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tempfile::{Builder, TempPath};
use tokio::{fs::File, io::AsyncWriteExt};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub(crate) enum ExportFileRequest {
    Open { path: PathBuf },
    Append { content: String },
    Finish,
    Discard,
}

pub(crate) struct ExportFile {
    file: File,
    temporary: TempPath,
    destination: PathBuf,
}

impl ExportFile {
    pub(crate) fn open(destination: PathBuf) -> io::Result<Self> {
        let directory = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let (file, temporary) = Builder::new()
            .prefix(".catabase-export-")
            .tempfile_in(directory)?
            .into_parts();
        Ok(Self {
            file: File::from_std(file),
            temporary,
            destination,
        })
    }

    pub(crate) async fn append(&mut self, content: &str) -> io::Result<()> {
        self.file.write_all(content.as_bytes()).await
    }

    pub(crate) async fn finish(self) -> io::Result<()> {
        let Self {
            mut file,
            temporary,
            destination,
        } = self;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        temporary.persist(destination).map_err(|error| error.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_export_preserves_existing_file_and_success_replaces_it() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("results.csv");
        std::fs::write(&destination, "existing").unwrap();

        let mut cancelled = ExportFile::open(destination.clone()).unwrap();
        cancelled.append("partial").await.unwrap();
        drop(cancelled);
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "existing");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);

        let mut complete = ExportFile::open(destination.clone()).unwrap();
        complete.append("id\n1").await.unwrap();
        complete.append("\n2").await.unwrap();
        complete.finish().await.unwrap();
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "id\n1\n2");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
