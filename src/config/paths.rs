//! 数据目录路径约定。

use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::{CoreError, CoreResult};

/// 持久化文件路径集合。
#[derive(Debug, Clone)]
pub struct DataPaths {
    /// 根目录。
    pub root: PathBuf,
}

impl DataPaths {
    /// 使用平台标准应用数据目录，并确保目录存在。
    pub fn discover() -> CoreResult<Self> {
        let dirs = ProjectDirs::from("dev", "Astral", "astral-core").ok_or_else(|| {
            CoreError::Internal("无法解析平台数据目录".into())
        })?;
        let root = dirs.data_dir().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// 使用显式根目录（测试 / 便携模式）。
    pub fn from_root(root: PathBuf) -> CoreResult<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// 节点 ID 文件。
    pub fn node_id_file(&self) -> PathBuf {
        self.root.join("node_id")
    }
}
