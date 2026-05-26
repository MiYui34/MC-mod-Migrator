use reqwest::Client;

use serde::Deserialize;



use crate::compat::{score_release, CompatTarget, PickContext, ReleaseChannel, ReleaseScore};

use crate::models::{ModFile, ModSource};

use crate::version::{asset_name_matches_loader, score_game_version_for_target, VersionPickScore};



pub struct GithubProvider {

    client: Client,

}



impl GithubProvider {

    pub fn new() -> Self {

        Self {

            client: crate::http::build_http_client(crate::http::APP_USER_AGENT),

        }

    }



    pub async fn get_compatible_release(

        &self,

        repo_url: &str,

        target: &CompatTarget,

        source_game_versions: &[String],

        source_mod_version: Option<&str>,

    ) -> anyhow::Result<Option<ModFile>> {

        let (owner, repo) = parse_github_repo(repo_url)?;

        let releases: Vec<GhRelease> = self

            .client

            .get(format!("https://api.github.com/repos/{owner}/{repo}/releases"))

            .send()

            .await?

            .error_for_status()?

            .json()

            .await?;



        let mut candidates: Vec<(ReleaseScore, &GhRelease, &GhAsset)> = Vec::new();



        for (idx, release) in releases.iter().enumerate() {

            if release.prerelease {

                continue;

            }

            for asset in &release.assets {

                if !asset.name.to_lowercase().ends_with(".jar") {

                    continue;

                }

                let name_lower = asset.name.to_lowercase();

                if !asset_name_matches_loader(&name_lower, &target.loader) {

                    continue;

                }

                let mc_score = extract_mc_score_from_asset_name(&name_lower, &target.mc_version);

                if mc_score.tier == u8::MAX {

                    continue;

                }

                let game_versions = vec![best_mc_from_asset(&name_lower, &target.mc_version)];

                let score = score_release(

                    &game_versions,

                    &[target.loader.clone()],

                    &release.tag_name,

                    target,

                    &PickContext {

                        source_mod_version,

                        source_game_versions,

                        list_index: idx,

                        channel: ReleaseChannel::Release,

                    },

                );

                if score.is_compatible() {

                    candidates.push((score, release, asset));

                }

            }

        }



        let best = candidates

            .into_iter()

            .min_by(|(sa, _, _), (sb, _, _)| sa.cmp(sb));



        if let Some((_, release, asset)) = best {

            return Ok(Some(ModFile {

                file_name: asset.name.clone(),

                download_url: asset.browser_download_url.clone(),

                version: release.tag_name.clone(),

                source: ModSource::Github,

            }));

        }



        Ok(None)

    }

}



impl Default for GithubProvider {

    fn default() -> Self {

        Self::new()

    }

}



fn best_mc_from_asset(name_lower: &str, target_mc: &str) -> String {

    use regex::Regex;



    if name_lower.contains(target_mc) {

        return target_mc.to_string();

    }

    let re = Regex::new(r"1\.\d+(?:\.\d+)?").unwrap();

    let mut best: Option<(VersionPickScore, String)> = None;

    for m in re.find_iter(name_lower) {

        if let Some(score) = score_game_version_for_target(m.as_str(), target_mc) {

            if best.as_ref().is_none_or(|(s, _)| score < *s) {

                best = Some((score, m.as_str().to_string()));

            }

        }

    }

    best.map(|(_, v)| v).unwrap_or_else(|| target_mc.to_string())

}



fn extract_mc_score_from_asset_name(name_lower: &str, mc_version: &str) -> VersionPickScore {

    use regex::Regex;



    if name_lower.contains(mc_version) {

        return score_game_version_for_target(mc_version, mc_version).unwrap_or(default_bad_score());

    }



    let re = Regex::new(r"1\.\d+(?:\.\d+)?").unwrap();

    let mut best = default_bad_score();

    for m in re.find_iter(name_lower) {

        if let Some(score) = score_game_version_for_target(m.as_str(), mc_version) {

            if score < best {

                best = score;

            }

        }

    }

    best

}



fn default_bad_score() -> VersionPickScore {

    VersionPickScore {

        tier: u8::MAX,

        patch_distance: u32::MAX,

    }

}



fn parse_github_repo(url: &str) -> anyhow::Result<(String, String)> {

    let url = url.trim_end_matches('/');

    let parts: Vec<&str> = url

        .trim_start_matches("https://")

        .trim_start_matches("http://")

        .trim_start_matches("github.com/")

        .trim_start_matches("www.github.com/")

        .split('/')

        .collect();

    if parts.len() >= 2 {

        Ok((parts[0].to_string(), parts[1].to_string()))

    } else {

        anyhow::bail!("invalid github url: {url}")

    }

}



#[derive(Debug, Deserialize)]

struct GhRelease {

    tag_name: String,

    prerelease: bool,

    assets: Vec<GhAsset>,

}



#[derive(Debug, Deserialize)]

struct GhAsset {

    name: String,

    browser_download_url: String,

}


