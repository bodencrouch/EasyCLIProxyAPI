use super::*;

pub(crate) fn find_deepseek_harness_desktop_application(home: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let local = agent_configuration_environment("LOCALAPPDATA")
            .unwrap_or_else(|| home.join("AppData/Local"));
        let roots = ["ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(agent_configuration_environment)
            .collect::<Vec<_>>();
        // Keep fixture homes isolated from applications installed on the test host.
        #[cfg(test)]
        let registrations = Vec::new();
        #[cfg(not(test))]
        let registrations = collect_windows_desktop_registrations(
            "DeepSeek Harness.exe",
            windows_display_name_matches_deepseek_harness,
        );
        find_windows_deepseek_harness_desktop(&local, &roots, &registrations)
    }
    #[cfg(target_os = "macos")]
    {
        find_macos_deepseek_harness_desktop(home)
    }
    #[cfg(target_os = "linux")]
    {
        let mut executable_directories = agent_executable_directories(home);
        for directory in ["/opt/DeepSeek Harness", "/opt/deepseek-harness", "/opt/deepseek-harness-desktop"] {
            let directory = PathBuf::from(directory);
            if !executable_directories.contains(&directory) {
                executable_directories.push(directory);
            }
        }
        let appimage_directories = [
            home.join("AppImages"),
            home.join("Applications"),
            home.join(".local/Applications"),
            home.join(".local/bin"),
            home.join("Downloads"),
            home.join("Desktop"),
        ];
        find_linux_deepseek_harness_desktop_in_roots(
            home,
            &executable_directories,
            &linux_data_directories(home),
            &appimage_directories,
        )
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = home;
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_data_directories(home: &Path) -> Vec<PathBuf> {
    let data_home = agent_configuration_environment("XDG_DATA_HOME")
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    let mut directories = vec![data_home];
    let system = env::var_os("XDG_DATA_DIRS")
        .map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .filter(|directories| !directories.is_empty())
        .unwrap_or_else(|| vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")]);
    for directory in system {
        if directory.is_absolute() && !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    directories
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn find_linux_deepseek_harness_desktop_in_roots(
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
    appimage_directories: &[PathBuf],
) -> Option<PathBuf> {
    // Electron builds name the binary after package.json, deb/rpm installs after productName.
    const EXECUTABLE_NAMES: &[&str] = &["deepseek-harness-desktop", "deepseek-harness", "DeepSeek Harness"];
    for directory in executable_directories {
        for name in EXECUTABLE_NAMES {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    linux_deepseek_harness_desktop_entries(home, executable_directories, data_directories)
        .into_iter()
        .next()
        .map(|(application, _)| application)
        .or_else(|| {
            appimage_directories.iter().find_map(|directory| {
                let mut candidates = fs::read_dir(directory)
                    .ok()?
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_file() && is_deepseek_harness_appimage(path))
                    .collect::<Vec<_>>();
                candidates.sort();
                candidates.into_iter().next()
            })
        })
}

/// Desktop entries for DeepSeek Harness, resolved to the application they start,
/// with the `X-AppImage-Version` that AppImage integrators write next to it.
#[cfg(any(target_os = "linux", test))]
fn linux_deepseek_harness_desktop_entries(
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
) -> Vec<(PathBuf, Option<String>)> {
    let mut found = Vec::new();
    for data_directory in data_directories {
        let mut entries = fs::read_dir(data_directory.join("applications"))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("desktop"))
            })
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            let Ok(content) = fs::read_to_string(&entry) else {
                continue;
            };
            let fields = linux_desktop_entry_fields(&content);
            let field = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
            let matches = ["Name", "X-AppImage-Name", "StartupWMClass"].iter().any(|key| {
                field(key).is_some_and(|value| {
                    let value = value.trim().to_ascii_lowercase();
                    value == "deepseek harness" || value == "deepseek-harness"
                })
            });
            if !matches {
                continue;
            }
            // Launcher managers wrap Exec in their own scripts; TryExec still names the real binary.
            let application = field("TryExec")
                .and_then(|command| resolve_linux_desktop_command(home, executable_directories, command.trim()))
                .or_else(|| {
                    linux_desktop_entry_command(&content).and_then(|command| {
                        resolve_linux_desktop_command(home, executable_directories, &command)
                    })
                })
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| !matches!(name, "python3" | "python" | "env" | "sh" | "bash"))
                });
            if let Some(application) = application {
                let version = field("X-AppImage-Version")
                    .map(str::trim)
                    .filter(|version| !version.is_empty())
                    .map(str::to_string);
                found.push((application, version));
            }
        }
    }
    found
}

#[cfg(any(target_os = "linux", test))]
fn linux_desktop_entry_fields(content: &str) -> Vec<(String, String)> {
    let mut in_desktop_entry = false;
    let mut fields = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
            continue;
        }
        if let (true, Some((key, value))) = (in_desktop_entry, line.split_once('=')) {
            fields.push((key.trim().to_string(), value.to_string()));
        }
    }
    fields
}

#[cfg(any(target_os = "linux", test))]
fn is_deepseek_harness_appimage(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    name.ends_with(".appimage") && name.contains("deepseek") && name.contains("harness")
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn read_linux_deepseek_harness_desktop_version(
    application: &Path,
    home: &Path,
    executable_directories: &[PathBuf],
    data_directories: &[PathBuf],
) -> Option<String> {
    let canonical = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let target = canonical(application);
    linux_deepseek_harness_desktop_entries(home, executable_directories, data_directories)
        .into_iter()
        .find_map(|(entry, version)| (canonical(&entry) == target).then_some(version).flatten())
        .or_else(|| {
            // An unpacked build keeps package.json next to the binary; an extracted
            // AppImage sits in a folder named after the image.
            let parent = application.parent()?;
            let stem = application.file_stem()?;
            [parent.join("resources/app/package.json"), parent.join(stem).join("resources/app/package.json")]
                .iter()
                .find_map(|path| read_package_json_version(path))
        })
        .or_else(|| {
            let stem = application.file_stem()?.to_str()?;
            stem.split(['-', '_']).find_map(|token| {
                let version = token.strip_prefix('v').unwrap_or(token);
                (version.starts_with(|c: char| c.is_ascii_digit())
                    && version.contains('.')
                    && version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '~')))
                .then(|| version.to_string())
            })
        })
}

#[cfg(target_os = "windows")]
pub(crate) fn windows_display_name_matches_deepseek_harness(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    name.strip_prefix("deepseek harness")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '(']))
}

#[cfg(target_os = "windows")]
pub(crate) fn find_windows_deepseek_harness_desktop(
    local: &Path,
    program_files: &[PathBuf],
    registrations: &[WindowsZcodeRegistration],
) -> Option<PathBuf> {
    let registered = registrations.iter().filter_map(|entry| {
        parse_windows_desktop_registration(entry.kind, &entry.value, "DeepSeek Harness.exe")
    });
    let defaults = [
        local.join("Programs/DeepSeek Harness/DeepSeek Harness.exe"),
        local.join("DeepSeek Harness/DeepSeek Harness.exe"),
    ];
    registered
        .chain(defaults)
        .chain(
            program_files
                .iter()
                .map(|root| root.join("DeepSeek Harness/DeepSeek Harness.exe")),
        )
        .find(|path| path.is_file())
}

pub(crate) fn read_deepseek_harness_desktop_version(application: &Path) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        // Electron's PE ProductVersion drops prerelease suffixes; package.json retains them.
        read_codex_asar_version(&application.parent()?.join("resources/app.asar"))
            .or_else(|| read_windows_executable_version(application))
    }
    #[cfg(target_os = "macos")]
    {
        read_macos_app_version(application)
    }
    #[cfg(target_os = "linux")]
    {
        let home = env::var_os("HOME").map(PathBuf::from).filter(|path| path.is_absolute())?;
        read_linux_deepseek_harness_desktop_version(
            application,
            &home,
            &agent_executable_directories(&home),
            &linux_data_directories(&home),
        )
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = application;
        None
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn find_macos_deepseek_harness_desktop_in_roots(roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join("DeepSeek Harness.app"))
        .find(|path| path.is_dir())
}

#[cfg(target_os = "macos")]
pub(crate) fn find_macos_deepseek_harness_desktop(home: &Path) -> Option<PathBuf> {
    let roots = [PathBuf::from("/Applications"), home.join("Applications")];
    find_macos_deepseek_harness_desktop_in_roots(&roots)
}
