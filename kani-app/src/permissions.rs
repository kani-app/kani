//! Typed application permissions and zero-sized handler requirement markers.

#[derive(Debug, Clone, PartialEq, Eq)]
/// Failure to parse a persisted `resource:action` permission string.
pub struct PermissionParseError(pub String);

impl std::error::Error for PermissionParseError {}

impl std::fmt::Display for PermissionParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Declares the permission enforced by web middleware before a handler runs.
pub trait AuthRequirement {
    fn required_permission() -> Option<Permission>;
}

/// Requirement marker for authentication without an additional permission.
pub struct IsAuthenticated;
impl AuthRequirement for IsAuthenticated {
    fn required_permission() -> Option<Permission> {
        None
    }
}

/// Defines typed resource/action enums, their stable string encoding, and handler guards.
macro_rules! permissions {
    (
        $(
            $resource:ident => $resource_str:literal {
                $( $action:ident => $action_str:literal ),* $(,)?
            }
        ),* $(,)?
    ) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum $resource {
                $( $action ),*
            }

            impl std::fmt::Display for $resource {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let s = match self {
                        $( Self::$action => $action_str ),*
                    };
                    write!(f, "{}", s)
                }
            }

            impl std::str::FromStr for $resource {
                type Err = PermissionParseError;
                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    match s {
                        $( $action_str => Ok(Self::$action), )*
                        _ => Err(PermissionParseError(format!("unknown {} action: {}", $resource_str, s))),
                    }
                }
            }
        )*

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Permission {
            $( $resource($resource) ),*
        }

        impl std::fmt::Display for Permission {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $( Permission::$resource(a) => write!(f, "{}:{}", $resource_str, a) ),*
                }
            }
        }

        impl std::str::FromStr for Permission {
            type Err = PermissionParseError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let (resource, action) = s
                    .split_once(':')
                    .ok_or_else(|| PermissionParseError(format!("invalid permission format: {s}")))?;
                match resource {
                    $( $resource_str => Ok(Permission::$resource(action.parse()?)), )*
                    _ => Err(PermissionParseError(format!("unknown resource: {resource}"))),
                }
            }
        }

        pub mod guards {
            pub struct Authenticated;
            impl super::AuthRequirement for Authenticated {
                fn required_permission() -> Option<super::Permission> { None }
            }

            $(
                $(
                    ::paste::paste! {
                        pub struct [<$resource $action>];

                        impl super::AuthRequirement for [<$resource $action>] {
                            fn required_permission() -> Option<super::Permission> {
                                Some(super::Permission::$resource(
                                    super::$resource::$action
                                ))
                            }
                        }
                    }
                )*
            )*
        }
    };
}

permissions! {
    Library => "library" {
        View   => "view",
        Add    => "add",
        Delete => "delete",
        Refresh => "refresh",
        Manage => "manage",
    },
    Chapter => "chapter" {
        Download => "download",
        Delete   => "delete",
    },
    Source => "source" {
        Browse         => "browse",
        Install        => "install",
        Delete         => "delete",
        ToggleEnabled  => "toggle_enabled",
        Configure      => "configure",
    },
    Settings => "settings" {
        View         => "view",
        EditDownload => "edit_download",
        EditScan     => "edit_scan",
        EditAdvanced => "edit_advanced",
    },
    User => "user" {
        Manage => "manage",
    },
    Server => "server" {
        Manage => "manage",
    },
    Admin => "admin" {
        ViewLogs  => "view_logs",
        ViewAudit => "view_audit",
        Manage    => "manage",
        Jobs      => "jobs",
    },
    Repo => "repo" {
        Add     => "add",
        Remove  => "remove",
        Trust   => "trust",
        Refresh => "refresh",
    },
    Opds => "opds" {
        Read     => "read",
        Progress => "progress",
    },
    Token => "token" {
        CreateOpds => "create_opds",
        CreateApi  => "create_api",
    },
    Metrics => "metrics" {
        Read => "read",
    },
    Theme => "theme" {
        Publish => "publish",
    },
}

impl serde::Serialize for Permission {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Permission {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_library_view() {
        assert_eq!(
            "library:view".parse::<Permission>().unwrap(),
            Permission::Library(Library::View)
        );
    }

    #[test]
    fn display_library_view() {
        assert_eq!(
            Permission::Library(Library::View).to_string(),
            "library:view"
        );
    }

    #[test]
    fn no_colon_is_parse_error() {
        assert!("libraryview".parse::<Permission>().is_err());
    }

    #[test]
    fn unknown_resource_is_error() {
        assert!("unknown:view".parse::<Permission>().is_err());
    }

    #[test]
    fn unknown_action_is_error() {
        assert!("library:fly".parse::<Permission>().is_err());
    }

    #[test]
    fn all_permissions_round_trip() {
        let perms = [
            "library:view",
            "library:add",
            "library:delete",
            "library:refresh",
            "library:manage",
            "chapter:download",
            "chapter:delete",
            "source:browse",
            "source:install",
            "source:delete",
            "source:toggle_enabled",
            "source:configure",
            "settings:view",
            "settings:edit_download",
            "settings:edit_scan",
            "settings:edit_advanced",
            "user:manage",
            "admin:view_logs",
            "admin:view_audit",
            "admin:jobs",
            "repo:add",
            "repo:remove",
            "repo:trust",
            "repo:refresh",
            "opds:read",
            "opds:progress",
            "token:create_opds",
            "token:create_api",
            "metrics:read",
        ];
        for raw in &perms {
            let parsed: Permission = raw.parse().expect(raw);
            assert_eq!(parsed.to_string(), *raw, "round-trip failed for {raw}");
        }
    }

    #[test]
    fn all_display_strings_are_unique() {
        let perms = [
            Permission::Library(Library::View),
            Permission::Library(Library::Add),
            Permission::Library(Library::Delete),
            Permission::Library(Library::Refresh),
            Permission::Library(Library::Manage),
            Permission::Chapter(Chapter::Download),
            Permission::Chapter(Chapter::Delete),
            Permission::Source(Source::Browse),
            Permission::Source(Source::Install),
            Permission::Source(Source::Delete),
            Permission::Source(Source::ToggleEnabled),
            Permission::Source(Source::Configure),
            Permission::Settings(Settings::View),
            Permission::Settings(Settings::EditDownload),
            Permission::Settings(Settings::EditScan),
            Permission::Settings(Settings::EditAdvanced),
            Permission::User(User::Manage),
            Permission::Server(Server::Manage),
            Permission::Admin(Admin::ViewLogs),
            Permission::Admin(Admin::ViewAudit),
            Permission::Repo(Repo::Add),
            Permission::Repo(Repo::Remove),
            Permission::Repo(Repo::Trust),
            Permission::Repo(Repo::Refresh),
            Permission::Opds(Opds::Read),
            Permission::Opds(Opds::Progress),
        ];
        let mut seen = std::collections::HashSet::new();
        for p in &perms {
            let s = p.to_string();
            assert!(seen.insert(s.clone()), "duplicate display string: {s}");
        }
    }

    #[test]
    fn serde_roundtrip() {
        let p = Permission::Source(Source::Install);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, r#""source:install""#);
        let back: Permission = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn source_install_guard_returns_correct_permission() {
        assert_eq!(
            guards::SourceInstall::required_permission(),
            Some(Permission::Source(Source::Install))
        );
    }

    #[test]
    fn source_delete_guard_returns_correct_permission() {
        assert_eq!(
            guards::SourceDelete::required_permission(),
            Some(Permission::Source(Source::Delete))
        );
    }

    #[test]
    fn is_authenticated_guard_returns_none() {
        assert_eq!(IsAuthenticated::required_permission(), None);
    }

    #[test]
    fn library_view_guard_returns_correct_permission() {
        assert_eq!(
            guards::LibraryView::required_permission(),
            Some(Permission::Library(Library::View))
        );
    }

    #[test]
    fn user_manage_guard_returns_correct_permission() {
        assert_eq!(
            guards::UserManage::required_permission(),
            Some(Permission::User(User::Manage))
        );
    }
}
