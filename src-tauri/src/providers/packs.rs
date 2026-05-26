use crate::compat::filename_stem;
use crate::models::{AppSettings, ModSource};
use crate::providers::curseforge::CurseForgeProvider;
use crate::providers::endpoints::{cf_usable_mirrors, mirrors_for_setting};
use crate::providers::modrinth::ModrinthProvider;

#[derive(Debug, Clone, Copy)]
pub enum PackKind {
    Shader,
    ResourcePack,
}

impl PackKind {
    fn modrinth_type(self) -> &'static str {
        match self {
            PackKind::Shader => "shader",
            PackKind::ResourcePack => "resourcepack",
        }
    }

    fn cf_class_id(self) -> i64 {
        match self {
            PackKind::Shader => 6552,
            PackKind::ResourcePack => 12,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OnlinePackHit {
    pub download_url: String,
    pub version: String,
    pub source: ModSource,
}

pub async fn lookup_online_pack(
    file_name: &str,
    kind: PackKind,
    target_mc: &str,
    settings: &AppSettings,
) -> Option<OnlinePackHit> {
    let query = pack_search_key(file_name);
    if query.is_empty() || target_mc.is_empty() || target_mc == "unknown" {
        return None;
    }

    for source in &settings.download_source_priority {
        match source.as_str() {
            "modrinth" => {
                for endpoints in mirrors_for_setting(&settings.mod_api_mirror) {
                    let provider = ModrinthProvider::with_endpoints(endpoints);
                    if let Ok(Some(file)) = provider
                        .find_pack_for_mc(&query, kind.modrinth_type(), target_mc)
                        .await
                    {
                        return Some(OnlinePackHit {
                            download_url: file.download_url,
                            version: file.version,
                            source: file.source,
                        });
                    }
                }
            }
            "curseforge" => {
                for endpoints in cf_usable_mirrors(settings) {
                    let cf = CurseForgeProvider::with_endpoints(
                        settings.curseforge_api_key.clone(),
                        endpoints,
                    );
                    if let Ok(Some(file)) = cf
                        .find_pack_for_mc(&query, kind.cf_class_id(), target_mc)
                        .await
                    {
                        return Some(OnlinePackHit {
                            download_url: file.download_url,
                            version: file.version,
                            source: ModSource::Curseforge,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn pack_search_key(file_name: &str) -> String {
    let stem = filename_stem(file_name);
    if stem.is_empty() {
        file_name.trim_end_matches(".zip").to_string()
    } else {
        stem
    }
}

