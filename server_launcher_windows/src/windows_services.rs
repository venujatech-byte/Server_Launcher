//! Windows Service Control Manager adapter; remote hosts still use systemd.
use super::{NewServiceDefinition, SystemdService, UnitDependenciesInfo, UnitFilesInfo};
use std::process::Command;
use std::os::windows::process::CommandExt;

fn powershell(script: &str) -> Result<String, String> {
    let out = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x08000000).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()) }
    else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

fn quote(value: &str) -> String { format!("'{}'", value.replace('\'', "''")) }

pub fn list_local_services(user_mode: bool) -> Result<Vec<SystemdService>, String> {
    if user_mode { return Err("Windows services use system scope. Select System services.".into()); }
    let out = powershell("$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; @(Get-CimInstance Win32_Service | Select-Object Name,DisplayName,State,StartMode) | ConvertTo-Json -Compress")?;
    parse_services(&out)
}

fn parse_services(out: &str) -> Result<Vec<SystemdService>, String> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(out).map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|row| {
        let running = row["State"].as_str() == Some("Running");
        SystemdService {
            name: row["Name"].as_str().unwrap_or_default().into(),
            description: row["DisplayName"].as_str().unwrap_or_default().into(),
            load_state: row["StartMode"].as_str().unwrap_or_default().into(),
            active_state: if running { "active" } else { "inactive" }.into(),
            sub_state: row["State"].as_str().unwrap_or_default().into(), is_user: false,
        }
    }).collect())
}

pub fn control_local_service(unit: &str, action: &str, user_mode: bool, _: Option<&str>) -> Result<String, String> {
    if user_mode { return Err("Windows services use system scope.".into()); }
    let name = quote(unit);
    let cmd = match action {
        "start" => format!("Start-Service -Name {name}"),
        "stop" => format!("Stop-Service -Name {name}"),
        "restart" => format!("Restart-Service -Name {name}"),
        "enable" => format!("Set-Service -Name {name} -StartupType Automatic"),
        "disable" => format!("Set-Service -Name {name} -StartupType Disabled"),
        "daemon-reload" => return Ok("Windows Service Control Manager reads changes immediately.".into()),
        _ => return Err(format!("Unsupported Windows service action: {action}")),
    };
    powershell(&format!("$ErrorActionPreference='Stop'; {cmd}"))?;
    Ok(format!("{action}: {unit}"))
}

pub fn fetch_local_journal(unit: &str, _: bool, lines: usize, _: Option<&str>) -> Result<String, String> {
    // Filter event properties, since localized messages need not contain the service's internal name.
    powershell(&format!("$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-WinEvent -FilterHashtable @{{LogName='System'; ProviderName='Service Control Manager'}} -MaxEvents 1000 | Where-Object {{ $_.Message -like ('*' + {} + '*') }} | Select-Object -First {} TimeCreated,Id,LevelDisplayName,Message | Format-List | Out-String", quote(unit), lines.clamp(1,1000)))
}

const UNSUPPORTED: &str = "systemd unit files and drop-ins apply to remote Linux hosts only. Use services.msc or sc.exe to configure local Windows services.";
pub fn create_local_service(_: &NewServiceDefinition, _: Option<&str>) -> Result<String,String> { Err(UNSUPPORTED.into()) }
pub fn get_local_unit_files(_: &str, _: bool) -> UnitFilesInfo { UnitFilesInfo::default() }
pub fn read_local_unit_file(_: &str, _: Option<&str>) -> Result<String,String> { Err(UNSUPPORTED.into()) }
pub fn write_local_unit_file(_: &str, _: &str, _: bool, _: Option<&str>) -> Result<String,String> { Err(UNSUPPORTED.into()) }
pub fn delete_local_drop_in(_: &str, _: bool, _: Option<&str>) -> Result<String,String> { Err(UNSUPPORTED.into()) }
pub fn get_local_unit_dependencies(_: &str, _: bool) -> UnitDependenciesInfo {
    UnitDependenciesInfo { tree_output: "Use sc.exe enumdepend <service> for Windows service dependencies.".into(), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_state_and_quoting() {
        let rows = parse_services(r#"[{"Name":"Spooler","DisplayName":"Print Spooler","State":"Running","StartMode":"Auto"},{"Name":"Stopped","State":"Stopped"}]"#).unwrap();
        assert!(rows[0].is_active());
        assert!(!rows[1].is_active());
        assert_eq!(quote("a'; Stop-Service x; '"), "'a''; Stop-Service x; '''");
        assert!(parse_services("invalid").is_err());
    }
}
