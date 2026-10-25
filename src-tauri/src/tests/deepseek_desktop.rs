use super::support::agent_test_home;
use super::*;

#[test]
fn deepseek_desktop_launch_targets_are_independent_of_cli_and_version() {
    let client = AgentClient::DeepSeekHarness;
    assert!(agent_installation_detected(client, None, false, true));
    let desktop = agent_launch_targets(client, None, None, true);
    assert_eq!(desktop.len(), 1);
    assert_eq!(desktop[0].id, "app");
    let both = agent_launch_targets(client, Some(Path::new("dsh")), None, true);
    assert_eq!(
        both.iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        ["cli", "app"]
    );
    // An installed profile alone must not create a Desktop launch target.
    assert!(agent_launch_targets(client, None, Some("0.2.0"), false).is_empty());
    assert!(!agent_installation_detected(client, None, false, false));
}

#[cfg(target_os = "windows")]
#[test]
fn deepseek_desktop_windows_finds_custom_and_default_installations() {
    let home = agent_test_home("deepseek-desktop-discovery");
    let local = home.join("local");
    let custom = home.join("custom/DeepSeek Harness.exe");
    let default = local.join("Programs/DeepSeek Harness/DeepSeek Harness.exe");
    fs::create_dir_all(custom.parent().unwrap()).unwrap();
    fs::create_dir_all(default.parent().unwrap()).unwrap();
    fs::write(&custom, []).unwrap();
    for (kind, value) in [
        ("directory", path_to_string(custom.parent().unwrap())),
        ("icon", format!("\"{}\",0", path_to_string(&custom))),
        (
            "uninstaller",
            format!(
                "\"{}\" /S",
                path_to_string(
                    &custom
                        .parent()
                        .unwrap()
                        .join("Uninstall DeepSeek Harness.exe")
                )
            ),
        ),
    ] {
        let entries = [WindowsZcodeRegistration { kind, value }];
        assert_eq!(
            find_windows_deepseek_harness_desktop(&local, &[], &entries),
            Some(custom.clone())
        );
    }
    fs::remove_file(&custom).unwrap();
    let stale = [WindowsZcodeRegistration {
        kind: "executable",
        value: path_to_string(&custom),
    }];
    assert_eq!(
        find_windows_deepseek_harness_desktop(&local, &[], &stale),
        None
    );
    fs::write(&default, []).unwrap();
    assert_eq!(
        find_windows_deepseek_harness_desktop(&local, &[], &stale),
        Some(default)
    );
    fs::remove_dir_all(home).unwrap();
}

#[cfg(target_os = "windows")]
#[test]
fn deepseek_desktop_windows_registration_name_is_specific() {
    for name in [
        "DeepSeek Harness",
        "DeepSeek Harness 0.2.0-rc.2",
        "deepseek harness (user)",
    ] {
        assert!(windows_display_name_matches_deepseek_harness(name));
    }
    for name in [
        "DeepSeek",
        "DeepSeek HarnessHelper",
        "Other DeepSeek Harness",
    ] {
        assert!(!windows_display_name_matches_deepseek_harness(name));
    }
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "Requires DeepSeek Harness Desktop installed on the host"]
fn deepseek_desktop_installed_host_probe() {
    let registrations = collect_windows_desktop_registrations(
        "DeepSeek Harness.exe",
        windows_display_name_matches_deepseek_harness,
    );
    let local = PathBuf::from(env::var_os("LOCALAPPDATA").unwrap());
    let app = find_windows_deepseek_harness_desktop(&local, &[], &registrations)
        .expect("installed Desktop should be detected");
    let version = read_deepseek_harness_desktop_version(&app).expect("Desktop version");
    println!("DeepSeek Harness Desktop: {} ({version})", app.display());
}

#[cfg(target_os = "macos")]
#[test]
fn deepseek_desktop_macos_finds_default_and_user_installations() {
    let home = agent_test_home("deepseek-desktop-macos");
    let system_root = home.join("Applications");
    let user_root = home.join("user/Applications");
    let roots = [system_root.clone(), user_root.clone()];

    assert_eq!(find_macos_deepseek_harness_desktop_in_roots(&roots), None);

    let user_app = user_root.join("DeepSeek Harness.app");
    fs::create_dir_all(&user_app).unwrap();
    assert_eq!(
        find_macos_deepseek_harness_desktop_in_roots(&roots),
        Some(user_app.clone())
    );

    let system_app = system_root.join("DeepSeek Harness.app");
    fs::create_dir_all(&system_app).unwrap();
    assert_eq!(
        find_macos_deepseek_harness_desktop_in_roots(&roots),
        Some(system_app)
    );

    fs::remove_dir_all(home).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn deepseek_harness_managed_executable_finds_bundled_and_home_cli() {
    use std::os::unix::fs::PermissionsExt;

    let home = agent_test_home("deepseek-cli-macos");
    let app_roots = [home.join("Applications")];
    let user_dsh = home.join(".dsh/bin/dsh");
    fs::create_dir_all(user_dsh.parent().unwrap()).unwrap();
    fs::write(&user_dsh, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&user_dsh, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        find_deepseek_harness_managed_executable_in_roots(&home, &app_roots),
        Some(user_dsh.clone())
    );

    let user_app_dsh = home.join("Applications/DeepSeek Harness.app/Contents/Resources/runtime/cli/bin/dsh");
    fs::create_dir_all(user_app_dsh.parent().unwrap()).unwrap();
    fs::write(&user_app_dsh, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&user_app_dsh, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(&user_dsh).unwrap();
    assert_eq!(
        find_deepseek_harness_managed_executable_in_roots(&home, &app_roots),
        Some(user_app_dsh)
    );

    fs::remove_dir_all(home).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn deepseek_desktop_macos_host_app_is_detected() {
    if !Path::new("/Applications/DeepSeek Harness.app").is_dir() {
        return;
    }
    let home = agent_test_home("deepseek-desktop-macos-host");
    let found = find_deepseek_harness_desktop_application(&home);
    assert_eq!(found, Some(PathBuf::from("/Applications/DeepSeek Harness.app")));
    let version = read_deepseek_harness_desktop_version(Path::new("/Applications/DeepSeek Harness.app"));
    assert!(version.is_some());
    let executable = find_deepseek_harness_executable(&home);
    assert!(executable.is_some());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn deepseek_desktop_linux_prefers_try_exec_and_appimage_version() {
    let home = agent_test_home("deepseek-desktop-linux-entry");
    let data = home.join("share");
    let appimage = home.join("AppImages/deepseek_harness.appimage");
    fs::create_dir_all(appimage.parent().unwrap()).unwrap();
    fs::create_dir_all(data.join("applications")).unwrap();
    fs::write(&appimage, []).unwrap();
    fs::write(
        data.join("applications/deepseek_harness.desktop"),
        format!(
            "[Desktop Entry]\nName=DeepSeek Harness\nTryExec={0}\nExec=\"/usr/bin/python3\" \"/x/plasma_app_actions.py\" \"run\" -- env DESKTOPINTEGRATION=1 {0} --no-sandbox %U\nStartupWMClass=DeepSeek Harness\nX-AppImage-Version=0.3.8\n\n[Desktop Action Other]\nName=DeepSeek Harness\nExec=/usr/bin/false\n",
            path_to_string(&appimage)
        ),
    )
    .unwrap();

    let found = find_linux_deepseek_harness_desktop_in_roots(&home, &[], &[data.clone()], &[]);
    assert_eq!(found, Some(appimage.clone()));
    assert_eq!(
        read_linux_deepseek_harness_desktop_version(&appimage, &home, &[], &[data]),
        Some("0.3.8".to_string())
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn deepseek_desktop_linux_scans_appimages_and_unpacked_installs() {
    let home = agent_test_home("deepseek-desktop-linux-scan");
    let images = home.join("AppImages");
    fs::create_dir_all(&images).unwrap();
    fs::write(images.join("Other-1.0.AppImage"), []).unwrap();
    assert_eq!(find_linux_deepseek_harness_desktop_in_roots(&home, &[], &[], &[images.clone()]), None);

    let appimage = images.join("DeepSeek-Harness-0.3.8-x86_64.AppImage");
    fs::write(&appimage, []).unwrap();
    assert_eq!(
        find_linux_deepseek_harness_desktop_in_roots(&home, &[], &[], &[images.clone()]),
        Some(appimage.clone())
    );
    assert_eq!(
        read_linux_deepseek_harness_desktop_version(&appimage, &home, &[], &[]),
        Some("0.3.8".to_string())
    );

    let opt = home.join("opt/DeepSeek Harness");
    let binary = opt.join("deepseek-harness");
    fs::create_dir_all(opt.join("resources/app")).unwrap();
    fs::write(&binary, []).unwrap();
    fs::write(opt.join("resources/app/package.json"), r#"{"version":"0.3.9-rc.1"}"#).unwrap();
    assert_eq!(
        find_linux_deepseek_harness_desktop_in_roots(&home, &[opt], &[], &[images]),
        Some(binary.clone())
    );
    assert_eq!(
        read_linux_deepseek_harness_desktop_version(&binary, &home, &[], &[]),
        Some("0.3.9-rc.1".to_string())
    );
    fs::remove_dir_all(home).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Requires DeepSeek Harness Desktop installed on the host"]
fn deepseek_desktop_linux_installed_host_probe() {
    let home = PathBuf::from(env::var_os("HOME").unwrap());
    let app = find_deepseek_harness_desktop_application(&home).expect("installed Desktop should be detected");
    let version = read_deepseek_harness_desktop_version(&app).expect("Desktop version");
    println!("DeepSeek Harness Desktop: {} ({version})", app.display());
}
