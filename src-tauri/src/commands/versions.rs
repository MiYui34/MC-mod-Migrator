use crate::commands::transfer::resolve_target_env;
use crate::compat::{CompatTarget, ModVersionPolicy};
use crate::models::{AppSettings, IdentifiedMod, ModVersionOption, TargetEnv};
use crate::providers::curseforge::list_curseforge_versions;
use crate::providers::modrinth::{list_modrinth_versions, ProjectVersionCache};

pub async fn list_mod_versions(
    mod_info: IdentifiedMod,
    target: TargetEnv,
    source_mods: &[IdentifiedMod],
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
) -> anyhow::Result<Vec<ModVersionOption>> {
    let target_env = resolve_target_env(target, source_mods);
    let compat = CompatTarget::from_target(&target_env);
    let policy = ModVersionPolicy::from_setting(&settings.mod_version_policy);
    let mut last_err: Option<anyhow::Error> = None;

    if mod_info.project_id.as_ref().is_some_and(|p| !p.is_empty())
        || !mod_info.sha512.is_empty()
        || !mod_info.sha1.is_empty()
    {
        match list_modrinth_versions(
            settings,
            &mod_info,
            &compat,
            Some(version_cache),
            policy,
            false,
            false,
        )
        .await
        {
            Ok(list) if !list.is_empty() => return Ok(list),
            Ok(_) => {}
            Err(e) => last_err = Some(e),
        }
    }

    if mod_info.curseforge_id.is_some()
        || mod_info
            .mod_id
            .as_ref()
            .is_some_and(|id| !id.is_empty())
    {
        match list_curseforge_versions(
            settings,
            &mod_info,
            &compat,
            policy,
            false,
            false,
        )
        .await
        {
            Ok(list) if !list.is_empty() => return Ok(list),
            Ok(_) => {}
            Err(e) => last_err = Some(e),
        }
    }

    if let Some(e) = last_err {
        Err(e)
    } else {
        Ok(vec![])
    }
}
