//! Server-administration endpoints for packets A1 (P-ADM-1, P-ADM-2, P-ADM-22, P-ADM-23), A2 (P-ADM-5..10) and A3 (P-ADM-11..15). Every call here
//! is admin-only on the server: a non-admin (or signed-out) caller gets `ApiError::Forbidden` / `Unauthorized`.
use super::gen_admin::{
    ActivityLogEntry, AuthenticationInfo, DeviceInfoDto, LibraryOptions, LogFile, PackageInfo, PluginInfo,
    RepositoryInfo, SessionInfoDto, SystemInfo, TaskInfo, TaskTriggerInfo, UserDto, UserPolicy, VirtualFolderInfo,
};
use super::{url_encode, Client, Result};
use serde::{Deserialize, Serialize};

/// P-ADM-22: one page of activity-log entries (the server returns newest first).
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct ActivityLogPage {
    pub items: Vec<ActivityLogEntry>,
    pub total_record_count: i32,
    pub start_index: i32,
}

/// P-ADM-9: one entry of `/Localization/ParentalRatings` (`Unrated` has no value).
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct ParentalRating {
    pub name: String,
    pub value: Option<i32>,
}

impl Client {
    /// P-ADM-1: `GET /System/Info` (server name, version, OS, …).
    pub fn system_info(&self) -> Result<SystemInfo> {
        self.get_json("/System/Info")
    }

    /// P-ADM-1: `GET /Sessions` (active sessions).
    pub fn sessions(&self) -> Result<Vec<SessionInfoDto>> {
        self.get_json("/Sessions")
    }

    /// P-ADM-1 / P-ADM-23: `GET /ScheduledTasks` (every task, running ones included).
    pub fn scheduled_tasks(&self) -> Result<Vec<TaskInfo>> {
        self.get_json("/ScheduledTasks")
    }

    /// P-ADM-23: one task with its triggers.
    pub fn scheduled_task(&self, id: &str) -> Result<TaskInfo> {
        self.get_json(&format!("/ScheduledTasks/{id}"))
    }

    /// P-ADM-23: run a task now.
    pub fn run_task(&self, id: &str) -> Result<()> {
        self.post_empty_ok(&format!("/ScheduledTasks/Running/{id}"))
    }

    /// P-ADM-23: `POST /ScheduledTasks/{id}/Triggers` replaces the task's whole trigger list (add or remove =
    /// edit the list and post it back).
    pub fn set_task_triggers(&self, id: &str, triggers: &[TaskTriggerInfo]) -> Result<()> {
        self.post_ok(&format!("/ScheduledTasks/{}/Triggers", url_encode(id)), &triggers)
    }

    /// P-ADM-23: stop a running task.
    pub fn stop_task(&self, id: &str) -> Result<()> {
        self.delete_ok(&format!("/ScheduledTasks/Running/{id}"))
    }

    /// P-ADM-2: scan all libraries.
    pub fn scan_all_libraries(&self) -> Result<()> {
        self.post_empty_ok("/Library/Refresh")
    }

    /// P-ADM-2: restart the server (the dashboard confirms before calling this).
    pub fn restart_server(&self) -> Result<()> {
        self.post_empty_ok("/System/Restart")
    }

    /// P-ADM-2: shut the server down (the dashboard confirms before calling this).
    pub fn shutdown_server(&self) -> Result<()> {
        self.post_empty_ok("/System/Shutdown")
    }

    /// P-ADM-22: activity-log entries, newest first, paged.
    pub fn activity_log(&self, start: usize, limit: usize) -> Result<ActivityLogPage> {
        self.get_json(&format!("/System/ActivityLog/Entries?startIndex={start}&limit={limit}"))
    }
}

/// P-ADM-10: body of `POST /Users/{id}/Password`.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct PasswordBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    new_pw: Option<&'a str>,
    reset_password: bool,
}

/// P-ADM-6: body of `POST /Users/New`.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct NewUserBody<'a> {
    name: &'a str,
    password: &'a str,
}

impl Client {
    /// P-ADM-5: `GET /Users` (every user; admin only).
    pub fn users(&self) -> Result<Vec<UserDto>> {
        self.get_json("/Users")
    }

    /// P-ADM-7: one user with their policy.
    pub fn user(&self, id: &str) -> Result<UserDto> {
        self.get_json(&format!("/Users/{id}"))
    }

    /// P-ADM-6: create a user with a password.
    pub fn create_user(&self, name: &str, password: &str) -> Result<UserDto> {
        self.post_json("/Users/New", &NewUserBody { name, password })
    }

    /// P-ADM-7: save the user record (the name).
    pub fn update_user(&self, id: &str, user: &UserDto) -> Result<()> {
        self.post_ok(&format!("/Users/{id}"), user)
    }

    /// P-ADM-7/8/9: save the whole policy (flags, library access, rating, tags, schedules).
    pub fn set_user_policy(&self, id: &str, policy: &UserPolicy) -> Result<()> {
        self.post_ok(&format!("/Users/{id}/Policy"), policy)
    }

    /// P-ADM-7: delete a user (the screen confirms first).
    pub fn delete_user(&self, id: &str) -> Result<()> {
        self.delete_ok(&format!("/Users/{id}"))
    }

    /// P-ADM-10: clear the password (the server then lets the user sign in with none).
    pub fn reset_user_password(&self, id: &str) -> Result<()> {
        self.post_ok(&format!("/Users/{id}/Password"), &PasswordBody { new_pw: None, reset_password: true })
    }

    /// P-ADM-10: set a new password (reset first, then set, as an admin has no current password to give).
    pub fn set_user_password(&self, id: &str, new_password: &str) -> Result<()> {
        self.reset_user_password(id)?;
        self.post_ok(
            &format!("/Users/{id}/Password"),
            &PasswordBody { new_pw: Some(new_password), reset_password: false },
        )
    }

    /// P-ADM-8: the libraries (virtual folders) with the ids `EnabledFolders` refers to; admin only.
    pub fn virtual_folders(&self) -> Result<Vec<VirtualFolderInfo>> {
        self.get_json("/Library/VirtualFolders")
    }

    /// P-ADM-9: the parental rating names and levels the server knows.
    pub fn parental_ratings(&self) -> Result<Vec<ParentalRating>> {
        self.get_json("/Localization/ParentalRatings")
    }
}

/// P-ADM-12: one fetcher / saver / reader the server offers (`/Libraries/AvailableOptions`).
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct FetcherInfo {
    pub name: String,
    pub default_enabled: bool,
}

/// P-ADM-12: the fetchers the server offers for one item type (Movie, Series, …).
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct AvailableType {
    #[serde(rename = "Type")]
    pub type_: String,
    pub metadata_fetchers: Vec<FetcherInfo>,
    pub image_fetchers: Vec<FetcherInfo>,
    pub default_image_options: Vec<super::gen_admin::ImageOption>,
}

/// P-ADM-12: `GET /Libraries/AvailableOptions`.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct AvailableOptions {
    pub metadata_savers: Vec<FetcherInfo>,
    pub metadata_readers: Vec<FetcherInfo>,
    pub type_options: Vec<AvailableType>,
}

/// P-ADM-11: body of `POST /Library/VirtualFolders`.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct AddFolderBody<'a> {
    library_options: &'a LibraryOptions,
}

/// P-ADM-12: body of `POST /Library/VirtualFolders/LibraryOptions`.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct OptionsBody<'a> {
    id: &'a str,
    library_options: &'a LibraryOptions,
}

/// P-ADM-12: body of `POST /Library/VirtualFolders/Paths`.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct PathBody<'a> {
    name: &'a str,
    path_info: super::gen_admin::MediaPathInfo,
}

/// P-ADM-11: the options a new library starts with. `LibraryOptions::default()` is not accepted by the server
/// (`AllowEmbeddedSubtitles` must be a real value), so start from here.
pub fn new_library_options(paths: &[String]) -> LibraryOptions {
    LibraryOptions {
        enabled: true,
        enable_photos: true,
        enable_automatic_series_grouping: true,
        season_zero_display_name: "Specials".into(),
        allow_embedded_subtitles: "AllowAll".into(),
        path_infos: paths.iter().map(|p| super::gen_admin::MediaPathInfo { path: p.clone() }).collect(),
        ..Default::default()
    }
}

impl Client {
    /// P-ADM-12: the metadata fetchers, image fetchers, savers and readers the server offers.
    pub fn library_available_options(&self) -> Result<AvailableOptions> {
        self.get_json("/Libraries/AvailableOptions")
    }

    /// P-ADM-11: add a library. `collection_type` is `None` for mixed content; `options.path_infos` should list `paths`.
    /// `refresh` starts a scan straight away.
    pub fn add_virtual_folder(
        &self,
        name: &str,
        collection_type: Option<&str>,
        paths: &[String],
        options: &LibraryOptions,
        refresh: bool,
    ) -> Result<()> {
        let mut q = format!("?name={}&refreshLibrary={refresh}", url_encode(name));
        if let Some(ct) = collection_type {
            q.push_str(&format!("&collectionType={}", url_encode(ct)));
        }
        for p in paths {
            q.push_str(&format!("&paths={}", url_encode(p)));
        }
        match self.post_ok(&format!("/Library/VirtualFolders{q}"), &AddFolderBody { library_options: options }) {
            // 10.10.7 creates the library, then answers 404 when the scan it starts trips over the new folder.
            // The library is there and the scan runs, so call it done when the list now has the name.
            Err(super::ApiError::NotFound) if refresh => {
                let made = self.virtual_folders()?.iter().any(|f| f.name.as_deref() == Some(name));
                if made {
                    Ok(())
                } else {
                    Err(super::ApiError::NotFound)
                }
            }
            r => r,
        }
    }

    /// P-ADM-11: rename a library.
    pub fn rename_virtual_folder(&self, name: &str, new_name: &str) -> Result<()> {
        self.post_empty_ok(&format!(
            "/Library/VirtualFolders/Name?name={}&newName={}",
            url_encode(name),
            url_encode(new_name)
        ))
    }

    /// P-ADM-11: remove a library (the media files stay; the screen confirms first). The refresh is what makes the
    /// server drop the library's item from the views; without it a scanned library lingers on Home.
    pub fn remove_virtual_folder(&self, name: &str) -> Result<()> {
        self.delete_ok(&format!("/Library/VirtualFolders?name={}&refreshLibrary=true", url_encode(name)))
    }

    /// P-ADM-12: save a library's options.
    pub fn set_library_options(&self, id: &str, options: &LibraryOptions) -> Result<()> {
        self.post_ok("/Library/VirtualFolders/LibraryOptions", &OptionsBody { id, library_options: options })
    }

    /// P-ADM-12: add a folder to a library (no scan).
    pub fn add_media_path(&self, name: &str, path: &str) -> Result<()> {
        self.post_ok(
            "/Library/VirtualFolders/Paths?refreshLibrary=false",
            &PathBody { name, path_info: super::gen_admin::MediaPathInfo { path: path.to_string() } },
        )
    }

    /// P-ADM-12: remove a folder from a library (no scan).
    pub fn remove_media_path(&self, name: &str, path: &str) -> Result<()> {
        self.delete_ok(&format!(
            "/Library/VirtualFolders/Paths?name={}&path={}&refreshLibrary=false",
            url_encode(name),
            url_encode(path)
        ))
    }

    /// P-ADM-11: scan one library (refresh its items, new and changed files only).
    pub fn scan_library(&self, item_id: &str) -> Result<()> {
        self.post_empty_ok(&format!(
            "/Items/{item_id}/Refresh?Recursive=true&ImageRefreshMode=Default&MetadataRefreshMode=Default&ReplaceAllImages=false&ReplaceAllMetadata=false"
        ))
    }

    /// P-ADM-13/14: the whole server configuration as JSON, so a page can change its fields and post the rest back untouched.
    pub fn system_configuration(&self) -> Result<serde_json::Value> {
        self.get_json("/System/Configuration")
    }

    /// P-ADM-13/14: replace the server configuration (post back what `system_configuration` returned, edited).
    pub fn set_system_configuration(&self, config: &serde_json::Value) -> Result<()> {
        self.post_ok("/System/Configuration", config)
    }

    /// P-ADM-15: a named configuration section (`xbmcmetadata` for NFO settings).
    pub fn named_configuration(&self, key: &str) -> Result<serde_json::Value> {
        self.get_json(&format!("/System/Configuration/{key}"))
    }

    /// P-ADM-15: replace a named configuration section.
    pub fn set_named_configuration(&self, key: &str, config: &serde_json::Value) -> Result<()> {
        self.post_ok(&format!("/System/Configuration/{key}"), config)
    }
}

/// A5: `{ "Items": [...] }` as the devices and API-key lists answer.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Items<T> {
    items: Vec<T>,
}

/// P-ADM-26: a key shown as bullets plus its last four characters. The full key never reaches a screen or a log.
pub fn mask_key(key: &str) -> String {
    let n = key.chars().count();
    if n < 8 {
        return "••••".to_string();
    }
    let tail: String = key.chars().skip(n - 4).collect();
    format!("••••••••{tail}")
}

/// P-ADM-25: the last `max` lines of a log, newest (last written) first.
pub fn newest_first(text: &str, max: usize) -> Vec<String> {
    text.lines().rev().take(max).map(str::to_string).collect()
}

impl Client {
    /// P-ADM-21: `GET /Devices` (every device that has signed in).
    pub fn devices(&self) -> Result<Vec<DeviceInfoDto>> {
        Ok(self.get_json::<Items<DeviceInfoDto>>("/Devices")?.items)
    }

    /// P-ADM-21: `POST /Devices/Options?id=` with the custom name (empty = back to the device's own name).
    pub fn rename_device(&self, id: &str, name: &str) -> Result<()> {
        self.post_ok(&format!("/Devices/Options?id={}", url_encode(id)), &serde_json::json!({ "CustomName": name }))
    }

    /// P-ADM-21: `DELETE /Devices?id=` (signs that device out). The screen never offers this for the current device.
    pub fn delete_device(&self, id: &str) -> Result<()> {
        self.delete_ok(&format!("/Devices?id={}", url_encode(id)))
    }

    /// P-ADM-26: `GET /Auth/Keys`.
    pub fn api_keys(&self) -> Result<Vec<AuthenticationInfo>> {
        Ok(self.get_json::<Items<AuthenticationInfo>>("/Auth/Keys")?.items)
    }

    /// P-ADM-26: `POST /Auth/Keys?app=` makes a key for that app name.
    pub fn create_api_key(&self, app: &str) -> Result<()> {
        self.post_empty_ok(&format!("/Auth/Keys?app={}", url_encode(app)))
    }

    /// P-ADM-26: `DELETE /Auth/Keys/{key}`.
    pub fn revoke_api_key(&self, key: &str) -> Result<()> {
        self.delete_ok(&format!("/Auth/Keys/{}", url_encode(key)))
    }

    /// P-ADM-25: `GET /System/Logs` (name, size, dates).
    pub fn log_files(&self) -> Result<Vec<LogFile>> {
        self.get_json("/System/Logs")
    }

    /// P-ADM-25: `GET /System/Logs/Log?name=` — the whole file as text.
    pub fn log_text(&self, name: &str) -> Result<String> {
        let mut resp = self
            .agent
            .get(&self.url(&format!("/System/Logs/Log?name={}", url_encode(name))))
            .header("Authorization", &self.auth_header())
            .call()
            .map_err(super::net)?;
        super::check_status(&mut resp)?;
        let bytes = resp.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(super::net)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// P-ADM-24: `GET /Plugins` (installed, with version and status).
    pub fn plugins(&self) -> Result<Vec<PluginInfo>> {
        self.get_json("/Plugins")
    }

    /// P-ADM-24: switch an installed plugin on (`POST /Plugins/{id}/{version}/Enable`); the server may need a restart.
    pub fn enable_plugin(&self, id: &str, version: &str) -> Result<()> {
        self.post_empty_ok(&format!("/Plugins/{}/{}/Enable", url_encode(id), url_encode(version)))
    }

    /// P-ADM-24: switch an installed plugin off (`POST /Plugins/{id}/{version}/Disable`).
    pub fn disable_plugin(&self, id: &str, version: &str) -> Result<()> {
        self.post_empty_ok(&format!("/Plugins/{}/{}/Disable", url_encode(id), url_encode(version)))
    }

    /// P-ADM-24: remove an installed plugin (`DELETE /Plugins/{id}/{version}`).
    pub fn uninstall_plugin(&self, id: &str, version: &str) -> Result<()> {
        self.delete_ok(&format!("/Plugins/{}/{}", url_encode(id), url_encode(version)))
    }

    /// P-ADM-24: `GET /Packages` — the catalogue from the server's repositories.
    pub fn packages(&self) -> Result<Vec<PackageInfo>> {
        self.get_json("/Packages")
    }

    /// P-ADM-24: `POST /Packages/Installed/{name}` for one catalogue version.
    pub fn install_package(&self, name: &str, guid: &str, version: &str, repository_url: &str) -> Result<()> {
        self.post_empty_ok(&format!(
            "/Packages/Installed/{}?assemblyGuid={}&version={}&repositoryUrl={}",
            url_encode(name),
            url_encode(guid),
            url_encode(version),
            url_encode(repository_url)
        ))
    }

    /// P-ADM-24: `GET /Repositories`.
    pub fn repositories(&self) -> Result<Vec<RepositoryInfo>> {
        self.get_json("/Repositories")
    }

    /// P-ADM-24: `POST /Repositories` replaces the whole list (add or remove = edit the list and post it back).
    pub fn set_repositories(&self, repos: &[RepositoryInfo]) -> Result<()> {
        self.post_ok("/Repositories", &repos)
    }

    /// P-ADM-24: `GET /web/ConfigurationPages` — the settings pages plugins register with the server's web app.
    pub fn configuration_pages(&self) -> Result<Vec<ConfigPage>> {
        self.get_json("/web/ConfigurationPages")
    }
}

/// P-ADM-24: one plugin settings page (`Name` is what the web app's `configurationpage?name=` takes).
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct ConfigPage {
    pub name: String,
    pub plugin_id: Option<String>,
}

fn same_guid(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.chars().filter(|c| *c != '-').collect::<String>().to_lowercase();
    norm(a) == norm(b)
}

/// P-ADM-24: the settings page of the plugin with this id, if it registered one.
pub fn page_for_plugin<'a>(pages: &'a [ConfigPage], plugin_id: &str) -> Option<&'a ConfigPage> {
    pages.iter().find(|p| !p.name.is_empty() && p.plugin_id.as_deref().is_some_and(|id| same_guid(id, plugin_id)))
}

/// P-ADM-24: where the server's web app shows that settings page (opened in the browser).
pub fn plugin_settings_url(base: &str, page_name: &str) -> String {
    format!("{}/web/#/configurationpage?name={}", base.trim_end_matches('/'), url_encode(page_name))
}
