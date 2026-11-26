use serde::{Deserialize, Serialize};

macro_rules! choices {
    ($name:ident { $first:ident $(, $other:ident)* $(,)? }) => {
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub(crate) enum $name {
            #[default]
            $first,
            $($other,)*
        }
    };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RdpSecurity {
    #[default]
    #[serde(alias = "rdp")]
    Auto,
    Nla,
    Tls,
}
choices!(Resolution {
    FitWindow,
    Fixed,
    MatchLocalScreen
});
choices!(RdpPainting { Smooth, Fast });
choices!(RdpFramePainting {
    Incremental,
    FullFrame
});
choices!(RdpColorProfile {
    Automatic,
    GfxRfx,
    GfxRfxProgressive,
    RemoteFx,
    TrueColor32,
    TrueColor24,
    HighColor16,
    HighColor15
});
choices!(Audio { Local, Off });
choices!(Clipboard {
    Both,
    RemoteToLocal,
    LocalToRemote,
    Off
});
choices!(Quality {
    Auto,
    Best,
    Good,
    Fast
});
choices!(Encoding {
    Tight,
    Zrle,
    CopyRect,
    Raw
});
choices!(Scaling { Fit, Native, Fill });

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DriveShare {
    pub path: std::path::PathBuf,
    #[serde(default)]
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RdpProfile {
    pub enabled: bool,
    pub port: u16,
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    pub domain: String,
    pub same_username_as_ssh: bool,
    pub security: RdpSecurity,
    pub painting: RdpPainting,
    pub frame_painting: RdpFramePainting,
    pub quality: Quality,
    pub color_profile: Option<RdpColorProfile>,
    pub color_depth: u8,
    pub resolution: Resolution,
    pub width: u16,
    pub height: u16,
    pub multi_monitor: bool,
    pub left_handed_mouse: bool,
    pub disable_smooth_scrolling: bool,
    pub keyboard_layout: Option<u32>,
    pub audio: Audio,
    pub mic: bool,
    pub clipboard: Clipboard,
    pub drives: Vec<DriveShare>,
    pub wallpaper: bool,
    pub font_smoothing: bool,
    pub animations: bool,
    pub themes: bool,
    pub admin_session: bool,
    pub gateway: String,
    pub tunnel_via_ssh: bool,
    pub auto_reconnect: bool,
}

impl Default for RdpProfile {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 3389,
            username: String::new(),
            credential_ref: None,
            domain: String::new(),
            same_username_as_ssh: false,
            security: RdpSecurity::Auto,
            painting: RdpPainting::Smooth,
            frame_painting: RdpFramePainting::Incremental,
            quality: Quality::Auto,
            color_profile: None,
            color_depth: 32,
            resolution: Resolution::FitWindow,
            width: 1920,
            height: 1080,
            multi_monitor: false,
            left_handed_mouse: false,
            disable_smooth_scrolling: false,
            keyboard_layout: None,
            audio: Audio::Local,
            mic: false,
            clipboard: Clipboard::Both,
            drives: Vec::new(),
            wallpaper: false,
            font_smoothing: true,
            animations: false,
            themes: true,
            admin_session: false,
            gateway: String::new(),
            tunnel_via_ssh: false,
            auto_reconnect: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct VncProfile {
    pub enabled: bool,
    pub port: u16,
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    pub quality: Quality,
    pub encodings: Vec<Encoding>,
    pub color_depth: u8,
    pub view_only: bool,
    pub show_remote_cursor: bool,
    pub disable_smooth_scrolling: bool,
    pub tcp_user_timeout_secs: u32,
    pub shared: bool,
    pub clipboard: Clipboard,
    pub scaling: Scaling,
    pub tunnel_via_ssh: bool,
    pub auto_reconnect: bool,
}

impl Default for VncProfile {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 5900,
            username: String::new(),
            credential_ref: None,
            quality: Quality::Auto,
            encodings: vec![
                Encoding::Tight,
                Encoding::Zrle,
                Encoding::CopyRect,
                Encoding::Raw,
            ],
            color_depth: 32,
            view_only: false,
            show_remote_cursor: true,
            disable_smooth_scrolling: false,
            tcp_user_timeout_secs: 0,
            shared: true,
            clipboard: Clipboard::Both,
            scaling: Scaling::Fit,
            tunnel_via_ssh: true,
            auto_reconnect: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ConfigFile;

    #[test]
    fn legacy_rdp_security_loads_and_saves_as_auto() {
        use super::RdpSecurity;

        let security: RdpSecurity = serde_json::from_str(r#""rdp""#).unwrap();
        assert_eq!(security, RdpSecurity::Auto);
        assert_eq!(serde_json::to_string(&security).unwrap(), r#""auto""#);
    }

    #[test]
    fn missing_frame_painting_uses_incremental_updates() {
        use super::{RdpFramePainting, RdpProfile};

        let profile: RdpProfile = serde_json::from_str("{}").unwrap();
        assert_eq!(profile.frame_painting, RdpFramePainting::Incremental);
    }

    #[test]
    fn legacy_hosts_round_trip_without_protocol_keys() {
        let input = r#"{"servers":[],"ssh_hosts":[{"id":"test","name":"Test","host":"test.example","user":"root","port":2222,"remote_cwd":"/opt/app"}]}"#;
        let original: ConfigFile = serde_json::from_str(input).unwrap();
        let saved = serde_json::to_value(&original).unwrap();
        let host = &saved["ssh_hosts"][0];
        assert!(host.get("rdp").is_none());
        assert!(host.get("vnc").is_none());
        let reloaded: ConfigFile = serde_json::from_value(saved).unwrap();
        assert_eq!(reloaded.ssh_hosts, original.ssh_hosts);
    }

    #[test]
    fn partial_profiles_use_protocol_defaults_and_do_not_store_passwords() {
        let input = r#"{"id":"test","name":"Test","host":"test.example","rdp":{"enabled":true,"password":"must-not-save"},"vnc":{"enabled":true,"password":"must-not-save"}}"#;
        let host: crate::config::SshRemoteHost = serde_json::from_str(input).unwrap();
        assert_eq!(host.rdp.as_ref().unwrap().port, 3389);
        let vnc = host.vnc.as_ref().unwrap();
        assert_eq!(vnc.port, 5900);
        assert!(vnc.show_remote_cursor);
        assert!(vnc.tunnel_via_ssh);
        assert!(!serde_json::to_string(&host)
            .unwrap()
            .contains("must-not-save"));
    }
}
