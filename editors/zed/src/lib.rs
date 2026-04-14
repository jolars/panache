use std::fs;
use zed::LanguageServerId;
use zed_extension_api::http_client::{HttpMethod, HttpRequest};
use zed_extension_api::{self as zed, settings::LspSettings, Result};

struct PanacheBinary {
    path: String,
    args: Option<Vec<String>>,
}

struct PanacheExtension {
    cached_binary_path: Option<String>,
}

#[derive(Debug, PartialEq)]
struct GithubReleaseDetails {
    asset_name: String,
    downloaded_file_type: zed::DownloadedFileType,
    downloaded_directory: String,
    downloaded_binary_path: String,
}

impl PanacheExtension {
    fn latest_panache_release() -> Result<zed::GithubRelease> {
        let request = HttpRequest::builder()
            .method(HttpMethod::Get)
            .url("https://api.github.com/repos/jolars/panache/releases?per_page=100")
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "panache-zed-extension")
            .build()?;

        let response = request
            .fetch()
            .map_err(|error| format!("Failed to fetch releases from GitHub: {error}"))?;

        Self::latest_panache_release_from_json(&response.body)
    }

    fn latest_panache_release_from_json(body: &[u8]) -> Result<zed::GithubRelease> {
        let releases: zed::serde_json::Value = zed::serde_json::from_slice(body)
            .map_err(|error| format!("Failed to parse GitHub releases response: {error}"))?;
        let releases = releases
            .as_array()
            .ok_or_else(|| "GitHub releases response was not an array".to_string())?;

        for release in releases {
            if release
                .get("prerelease")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                continue;
            }

            let Some(tag_name) = release.get("tag_name").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(version) = tag_name.strip_prefix("panache-v") else {
                continue;
            };

            let assets = release
                .get("assets")
                .and_then(|value| value.as_array())
                .ok_or_else(|| {
                    format!("GitHub release {tag_name} did not include an assets array")
                })?;
            if assets.is_empty() {
                continue;
            }

            let assets = assets
                .iter()
                .map(|asset| {
                    let name = asset
                        .get("name")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| format!("GitHub release {tag_name} has an asset without a name"))?;
                    let download_url = asset
                        .get("browser_download_url")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            format!(
                                "GitHub release {tag_name} has an asset without browser_download_url"
                            )
                        })?;

                    Ok(zed::GithubReleaseAsset {
                        name: name.to_string(),
                        download_url: download_url.to_string(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            return Ok(zed::GithubRelease {
                version: version.to_string(),
                assets,
            });
        }

        Err("No stable GitHub release matching panache-v* with assets found".to_string())
    }

    fn language_server_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<PanacheBinary> {
        let binary_settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.binary);

        let binary_args = binary_settings
            .as_ref()
            .and_then(|binary_settings| binary_settings.arguments.clone());

        if let Some(path) = binary_settings.and_then(|binary_settings| binary_settings.path) {
            return Ok(PanacheBinary {
                path,
                args: binary_args,
            });
        }

        if let Some(path) = worktree.which("panache") {
            return Ok(PanacheBinary {
                path,
                args: binary_args,
            });
        }

        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).is_ok_and(|stat| stat.is_file()) {
                return Ok(PanacheBinary {
                    path: path.clone(),
                    args: binary_args,
                });
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = Self::latest_panache_release()?;

        let (platform, arch) = zed::current_platform();
        let release_details = GithubReleaseDetails::new(platform, arch, release.version);

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == release_details.asset_name)
            .ok_or_else(|| {
                format!(
                    "No asset found matching {asset_name:?}",
                    asset_name = release_details.asset_name
                )
            })?;

        if !fs::metadata(&release_details.downloaded_binary_path).is_ok_and(|stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &release_details.downloaded_directory,
                release_details.downloaded_file_type,
            )
            .map_err(|error| format!("Failed to download file: {error}"))?;

            let entries = fs::read_dir(".")
                .map_err(|error| format!("Failed to list working directory: {error}"))?;

            for entry in entries {
                let entry =
                    entry.map_err(|error| format!("Failed to load directory entry: {error}"))?;
                if entry.file_name().to_str() != Some(&release_details.downloaded_directory) {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }

        self.cached_binary_path = Some(release_details.downloaded_binary_path.clone());

        Ok(PanacheBinary {
            path: release_details.downloaded_binary_path,
            args: binary_args,
        })
    }
}

impl GithubReleaseDetails {
    fn new(
        platform: zed_extension_api::Os,
        arch: zed_extension_api::Architecture,
        version: String,
    ) -> Self {
        let target_triple = format!(
            "{arch}-{os}",
            arch = match arch {
                zed::Architecture::Aarch64 => "aarch64",
                zed::Architecture::X86 => "x86",
                zed::Architecture::X8664 => "x86_64",
            },
            os = match platform {
                zed::Os::Mac => "apple-darwin",
                zed::Os::Linux => "unknown-linux-gnu",
                zed::Os::Windows => "pc-windows-msvc",
            }
        );

        let asset_name = format!(
            "panache-{target_triple}.{suffix}",
            suffix = match platform {
                zed::Os::Mac | zed::Os::Linux => "tar.gz",
                zed::Os::Windows => "zip",
            }
        );

        let downloaded_file_type = match platform {
            zed::Os::Mac | zed::Os::Linux => zed::DownloadedFileType::GzipTar,
            zed::Os::Windows => zed::DownloadedFileType::Zip,
        };

        let downloaded_directory = format!("panache-{version}");

        let downloaded_binary_path = match platform {
            zed::Os::Mac | zed::Os::Linux => format!("{downloaded_directory}/panache"),
            zed::Os::Windows => format!("{downloaded_directory}/panache.exe"),
        };

        Self {
            asset_name,
            downloaded_file_type,
            downloaded_directory,
            downloaded_binary_path,
        }
    }
}

impl zed::Extension for PanacheExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let panache_binary = self.language_server_binary(language_server_id, worktree)?;
        Ok(zed::Command {
            command: panache_binary.path,
            args: panache_binary.args.unwrap_or_else(|| vec!["lsp".into()]),
            env: vec![],
        })
    }

    fn language_server_initialization_options(
        &mut self,
        server_id: &LanguageServerId,
        worktree: &zed_extension_api::Worktree,
    ) -> Result<Option<zed_extension_api::serde_json::Value>> {
        let settings = LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.initialization_options.clone())
            .unwrap_or_default();
        Ok(Some(settings))
    }

    fn language_server_workspace_configuration(
        &mut self,
        server_id: &LanguageServerId,
        worktree: &zed_extension_api::Worktree,
    ) -> Result<Option<zed_extension_api::serde_json::Value>> {
        let settings = LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.settings.clone())
            .unwrap_or_default();
        Ok(Some(settings))
    }
}

zed::register_extension!(PanacheExtension);

#[cfg(test)]
mod test {
    use crate::{GithubReleaseDetails, PanacheExtension};

    #[test]
    fn test_github_release_details() {
        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Mac,
                zed_extension_api::Architecture::Aarch64,
                String::from("0.1.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("panache-aarch64-apple-darwin.tar.gz"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::GzipTar,
                downloaded_directory: String::from("panache-0.1.0"),
                downloaded_binary_path: String::from("panache-0.1.0/panache")
            }
        );

        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Linux,
                zed_extension_api::Architecture::X8664,
                String::from("0.2.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("panache-x86_64-unknown-linux-gnu.tar.gz"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::GzipTar,
                downloaded_directory: String::from("panache-0.2.0"),
                downloaded_binary_path: String::from("panache-0.2.0/panache")
            }
        );

        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Windows,
                zed_extension_api::Architecture::X8664,
                String::from("0.1.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("panache-x86_64-pc-windows-msvc.zip"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::Zip,
                downloaded_directory: String::from("panache-0.1.0"),
                downloaded_binary_path: String::from("panache-0.1.0/panache.exe")
            }
        );
    }

    #[test]
    fn test_latest_panache_release_skips_non_panache_packages() {
        let body = r#"
[
  {
    "tag_name": "panache-parser-v0.2.0",
    "prerelease": false,
    "assets": [
      { "name": "parser-asset.tgz", "browser_download_url": "https://example.com/parser.tgz" }
    ]
  },
  {
    "tag_name": "panache-v2.33.0",
    "prerelease": false,
    "assets": [
      { "name": "panache-x86_64-unknown-linux-gnu.tar.gz", "browser_download_url": "https://example.com/panache.tar.gz" }
    ]
  }
]
"#;

        let release = PanacheExtension::latest_panache_release_from_json(body.as_bytes()).unwrap();

        assert_eq!(release.version, "2.33.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(
            release.assets[0].name,
            "panache-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_latest_panache_release_skips_prerelease() {
        let body = r#"
[
  {
    "tag_name": "panache-v2.34.0-rc1",
    "prerelease": true,
    "assets": [
      { "name": "panache-x86_64-unknown-linux-gnu.tar.gz", "browser_download_url": "https://example.com/rc.tar.gz" }
    ]
  },
  {
    "tag_name": "panache-v2.33.0",
    "prerelease": false,
    "assets": [
      { "name": "panache-x86_64-unknown-linux-gnu.tar.gz", "browser_download_url": "https://example.com/stable.tar.gz" }
    ]
  }
]
"#;

        let release = PanacheExtension::latest_panache_release_from_json(body.as_bytes()).unwrap();
        assert_eq!(release.version, "2.33.0");
        assert_eq!(
            release.assets[0].download_url,
            "https://example.com/stable.tar.gz"
        );
    }
}
