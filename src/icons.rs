/// Parses `gtk-icon-theme-name = "..."` out of a GTK settings.ini's `[Settings]` section.
// Not yet called outside tests; Task 4's `IconCache::get` will consume this.
#[allow(dead_code)]
pub(crate) fn parse_gtk_icon_theme(ini_text: &str) -> Option<String> {
    for line in ini_text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("gtk-icon-theme-name") {
            let rest = rest.trim_start();
            let value = rest.strip_prefix('=')?.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Ordered list of icon names to try when looking up an app's icon: the raw
/// window class, then the cleaned (reversed-domain-stripped), lowercased name.
// Not yet called outside tests; Task 4's `IconCache::get` will consume this.
#[allow(dead_code)]
pub(crate) fn icon_name_candidates(class_name: &str) -> Vec<String> {
    let cleaned = crate::render::clean_app_name(class_name).to_lowercase();
    let mut candidates = vec![class_name.to_string()];
    if cleaned != class_name {
        candidates.push(cleaned);
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_icon_theme_from_settings_ini() {
        let ini = "[Settings]\ngtk-theme-name=Adwaita\ngtk-icon-theme-name=Papirus-Dark\ngtk-font-name=Sans 10\n";
        assert_eq!(parse_gtk_icon_theme(ini), Some("Papirus-Dark".to_string()));
    }

    #[test]
    fn parses_icon_theme_with_spaces_around_equals() {
        let ini = "[Settings]\ngtk-icon-theme-name = Yaru\n";
        assert_eq!(parse_gtk_icon_theme(ini), Some("Yaru".to_string()));
    }

    #[test]
    fn missing_icon_theme_key_returns_none() {
        let ini = "[Settings]\ngtk-theme-name=Adwaita\n";
        assert_eq!(parse_gtk_icon_theme(ini), None);
    }

    #[test]
    fn empty_text_returns_none() {
        assert_eq!(parse_gtk_icon_theme(""), None);
    }

    #[test]
    fn candidates_for_reversed_domain_class_name() {
        assert_eq!(
            icon_name_candidates("dev.zed.Zed"),
            vec!["dev.zed.Zed", "zed"]
        );
    }

    #[test]
    fn candidates_dedup_when_class_already_clean_and_lowercase() {
        // "alacritty" cleaned is still "alacritty" and already lowercase —
        // candidates should not contain the exact same string twice.
        assert_eq!(icon_name_candidates("alacritty"), vec!["alacritty"]);
    }
}
