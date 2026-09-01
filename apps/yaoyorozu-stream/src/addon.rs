use std::path::PathBuf;

pub const ADDON_API_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddonOrigin {
    Official,
    External,
}

#[derive(Debug, Clone)]
pub struct AddonManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub required_api: u32,
    pub origin: AddonOrigin,
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

impl AddonManifest {
    pub fn is_compatible(&self) -> bool {
        self.required_api == ADDON_API_VERSION
    }

    pub fn compatibility_label(&self) -> &'static str {
        if self.is_compatible() {
            "互換"
        } else {
            "API不一致"
        }
    }
}

#[derive(Debug, Default)]
pub struct AddonRegistry {
    addons: Vec<AddonManifest>,
}

impl AddonRegistry {
    pub fn new() -> Self {
        let mut registry = Self::default();

        // Phase 10.0.0では、既存機能を壊さずにアドオン管理の枠だけ先に用意する。
        // ここに公式アドオンを登録すると、設定画面に自動で並ぶ。
        registry.addons.push(AddonManifest {
            id: "official.performance_details".to_owned(),
            name: "詳細パフォーマンス表示".to_owned(),
            version: "0.1.0".to_owned(),
            required_api: ADDON_API_VERSION,
            origin: AddonOrigin::Official,
            enabled: false,
            path: None,
        });
        registry.addons.push(AddonManifest {
            id: "official.test_overlay".to_owned(),
            name: "テストオーバーレイ拡張".to_owned(),
            version: "0.1.0".to_owned(),
            required_api: ADDON_API_VERSION,
            origin: AddonOrigin::Official,
            enabled: false,
            path: None,
        });

        registry
    }

    pub fn addons(&self) -> &[AddonManifest] {
        &self.addons
    }

    pub fn addons_mut(&mut self) -> &mut [AddonManifest] {
        &mut self.addons
    }

    pub fn register_external_placeholder(&mut self, path: PathBuf) {
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("外部アドオン")
            .to_owned();

        let id = format!("external.{}", self.addons.len());
        self.addons.push(AddonManifest {
            id,
            name: display_name,
            version: "未読込".to_owned(),
            required_api: ADDON_API_VERSION,
            origin: AddonOrigin::External,
            enabled: false,
            path: Some(path),
        });
    }

    pub fn official_count(&self) -> usize {
        self.addons
            .iter()
            .filter(|addon| addon.origin == AddonOrigin::Official)
            .count()
    }

    pub fn external_count(&self) -> usize {
        self.addons
            .iter()
            .filter(|addon| addon.origin == AddonOrigin::External)
            .count()
    }
}
