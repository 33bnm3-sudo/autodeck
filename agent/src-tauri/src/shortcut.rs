use std::path::Path;

pub fn resolve_target(lnk_path: &str) -> Option<String> {
    let lnk = parselnk::Lnk::try_from(Path::new(lnk_path)).ok()?;
    let base = lnk
        .link_info
        .local_base_path_unicode
        .or(lnk.link_info.local_base_path)?;
    let suffix = lnk
        .link_info
        .common_path_suffix_unicode
        .or(lnk.link_info.common_path_suffix)
        .unwrap_or_default();
    Some(format!("{base}{suffix}"))
}
