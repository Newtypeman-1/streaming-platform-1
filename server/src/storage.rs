//! 업로드된 영상을 로컬 디스크에 저장한다.
//! DB에는 절대경로가 아니라 저장소 안에서만 의미 있는 "키"를 저장한다.
//! (나중에 S3 같은 외부 저장소로 옮겨도 DB 데이터는 그대로 둘 수 있도록)

use std::io;
use std::path::{Path, PathBuf};

use tokio::io::AsyncRead;

#[derive(Debug, Clone)]
pub struct LocalVideoStorage {
    root: PathBuf,
}

impl LocalVideoStorage {
    /// 저장소 디렉터리가 없으면 만든다.
    pub fn new(root: impl AsRef<Path>) -> io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        // canonicalize 로 상대경로와 심볼릭 링크를 실제 절대경로로 고정한다.
        Ok(Self { root: root.as_ref().canonicalize()? })
    }

    /// 영상을 저장하고 다시 꺼낼 때 쓸 키를 돌려준다.
    /// 사용자가 보낸 파일명은 쓰지 않고 확장자만 유지한다.
    pub async fn save(&self, original_filename: Option<&str>, mut content: impl AsyncRead + Unpin) -> io::Result<String> {
        let extension = original_filename
            .and_then(|name| Path::new(name).extension())
            .and_then(|ext| ext.to_str())
            .filter(|ext| ext.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|ext| format!(".{}", ext.to_ascii_lowercase()))
            .unwrap_or_default();
        let key = format!("{}{extension}", uuid::Uuid::new_v4());

        let mut file = tokio::fs::File::create(self.root.join(&key)).await?;
        tokio::io::copy(&mut content, &mut file).await?;
        Ok(key)
    }

    /// 키에 해당하는 파일 경로. 없거나 저장소 밖을 가리키면 None.
    pub fn path_of(&self, key: &str) -> Option<PathBuf> {
        let path = self.root.join(key).canonicalize().ok()?;
        (path.starts_with(&self.root) && path.is_file()).then_some(path)
    }

    /// 저장한 영상을 지운다. (DB 저장 실패 시 고아 파일 정리용)
    pub async fn delete(&self, key: &str) -> io::Result<()> {
        match self.path_of(key) {
            Some(path) => tokio::fs::remove_file(path).await,
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, LocalVideoStorage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalVideoStorage::new(dir.path()).unwrap();
        (dir, storage)
    }

    #[tokio::test]
    async fn 저장한_영상을_같은_키로_다시_찾을_수_있다() {
        let (_dir, storage) = storage();

        let key = storage.save(Some("clip.mp4"), &[1u8, 2, 3, 4][..]).await.unwrap();

        let path = storage.path_of(&key).unwrap();
        assert_eq!(tokio::fs::read(path).await.unwrap(), vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn 저장_키는_사용자_파일명을_쓰지_않고_확장자만_유지한다() {
        let (dir, storage) = storage();

        let key = storage.save(Some("../../etc/passwd.MP4"), &[1u8][..]).await.unwrap();

        assert!(!key.contains('/') && !key.contains(".."), "{key}");
        assert!(key.ends_with(".mp4"), "{key}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn 파일명이_없거나_확장자가_없어도_저장된다() {
        let (_dir, storage) = storage();

        let key = storage.save(None, &[1u8][..]).await.unwrap();

        assert!(storage.path_of(&key).is_some());
    }

    #[test]
    fn 저장소_디렉터리가_없으면_만든다() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");

        LocalVideoStorage::new(&nested).unwrap();

        assert!(nested.is_dir());
    }

    #[test]
    fn 상대경로로도_만들_수_있다() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let relative = dir.path().file_name().unwrap().to_str().unwrap().to_string() + "/videos";

        LocalVideoStorage::new(&relative).unwrap();

        assert!(dir.path().join("videos").is_dir());
    }

    #[test]
    fn 없는_키나_저장소_밖을_가리키는_키는_찾지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("secret.mp4"), b"x").unwrap();
        let storage = LocalVideoStorage::new(dir.path().join("videos")).unwrap();

        assert_eq!(storage.path_of("missing.mp4"), None);
        assert_eq!(storage.path_of("../secret.mp4"), None);
    }

    #[tokio::test]
    async fn 저장한_영상을_지울_수_있다() {
        let (_dir, storage) = storage();
        let key = storage.save(Some("a.mp4"), &[1u8][..]).await.unwrap();

        storage.delete(&key).await.unwrap();

        assert_eq!(storage.path_of(&key), None);
    }
}
