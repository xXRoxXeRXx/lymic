use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, sqlx::Type,
)]
#[sqlx(transparent)]
#[serde(transparent)]
pub struct AlbumId(String);

impl AlbumId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AlbumId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for AlbumId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for AlbumId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for AlbumId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AlbumId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, sqlx::Type,
)]
#[sqlx(transparent)]
#[serde(transparent)]
pub struct RemoteAssetId(String);

impl RemoteAssetId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RemoteAssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for RemoteAssetId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for RemoteAssetId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for RemoteAssetId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for RemoteAssetId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newtypes_display_and_deref() {
        let album_id = AlbumId::new("album-uuid-123");
        assert_eq!(album_id.as_str(), "album-uuid-123");
        assert_eq!(album_id.to_string(), "album-uuid-123");
        assert_eq!(&*album_id, "album-uuid-123");

        let remote_id = RemoteAssetId::new("asset-uuid-456");
        assert_eq!(remote_id.as_str(), "asset-uuid-456");
        assert_eq!(remote_id.to_string(), "asset-uuid-456");
        assert_eq!(&*remote_id, "asset-uuid-456");
    }

    #[test]
    fn serde_transparency() {
        let album_id = AlbumId::new("alb-1");
        let json = serde_json::to_string(&album_id).unwrap();
        assert_eq!(json, "\"alb-1\"");
        let deserialized: AlbumId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, album_id);

        let remote_id = RemoteAssetId::new("ast-2");
        let json = serde_json::to_string(&remote_id).unwrap();
        assert_eq!(json, "\"ast-2\"");
        let deserialized: RemoteAssetId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, remote_id);
    }
}
