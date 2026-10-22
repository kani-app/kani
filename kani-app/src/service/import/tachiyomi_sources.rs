/// Map a Tachiyomi source ID to the corresponding Kani source name.
/// Source IDs from tachiyomi-extensions and mihon-extensions.
pub(crate) fn tachiyomi_source_to_kani_name(source_id: i64) -> Option<&'static str> {
    match source_id {
        2499283573021220255 => Some("MangaDex"),
        2131019126180322627 => Some("WeebCentral"),
        8448310129093543312 => Some("MangaPill"),
        6338219619148105941 | 1470847599087460255 | 2013845246758512290 => Some("Cubari"),
        _ => None,
    }
}

/// Map a Tachiyomi tracker syncId to the Kani tracker name.
/// Only AniList and MyAnimeList are supported by Kani.
pub(crate) fn tachiyomi_sync_id_to_tracker_name(sync_id: i32) -> Option<&'static str> {
    match sync_id {
        1 => Some("MyAnimeList"),
        2 => Some("AniList"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmapped_id_maps_to_nothing() {
        assert_eq!(tachiyomi_source_to_kani_name(1), None);
    }

    #[test]
    fn every_cubari_alias_maps() {
        for id in [
            6338219619148105941,
            1470847599087460255,
            2013845246758512290,
        ] {
            assert_eq!(tachiyomi_source_to_kani_name(id), Some("Cubari"));
        }
    }
}
