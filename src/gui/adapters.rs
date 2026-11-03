use crate::platform::Adapter;

/// Keep an unsaved selection visible when a removable adapter disappears.
pub(super) fn refresh(all: &[Adapter], selected: Option<&Adapter>) -> Vec<Adapter> {
    let mut items: Vec<_> = all.iter().filter(|a| a.ethernet).cloned().collect();
    if let Some(selected) = selected {
        if !items.iter().any(|a| a.id == selected.id) {
            let mut missing = selected.clone();
            missing.up = false;
            missing.ipv4.clear();
            missing.gateway.clear();
            missing.dns.clear();
            items.push(missing);
        }
    }
    items
}

pub(super) fn selection(items: &[Adapter], preferred: &str) -> Option<usize> {
    items
        .iter()
        .position(|a| a.id == preferred || a.name == preferred)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(id: &str, name: &str) -> Adapter {
        Adapter {
            id: id.into(),
            name: name.into(),
            ethernet: true,
            up: true,
            ..Adapter::default()
        }
    }

    #[test]
    fn hotplug_refresh_preserves_unsaved_selection_across_reorder_and_rename() {
        let selected = adapter("usb", "USB Ethernet");
        let renamed = adapter("usb", "USB Ethernet 2");
        let items = refresh(
            &[adapter("builtin", "Ethernet"), renamed.clone()],
            Some(&selected),
        );
        assert_eq!(items[selection(&items, &selected.id).unwrap()], renamed);
    }

    #[test]
    fn unplugged_selection_stays_visible_and_reconnect_replaces_placeholder() {
        let selected = adapter("usb", "USB Ethernet");
        let items = refresh(&[adapter("builtin", "Ethernet")], Some(&selected));
        let missing = &items[selection(&items, "usb").unwrap()];
        assert!(!missing.up);
        let items = refresh(std::slice::from_ref(&selected), Some(missing));
        assert_eq!(items, [selected]);
    }

    #[test]
    fn empty_list_and_missing_saved_adapter_do_not_select_another_adapter() {
        assert!(refresh(&[], None).is_empty());
        assert_eq!(selection(&[adapter("builtin", "Ethernet")], "usb"), None);
        assert!(refresh(&[Adapter::default()], None).is_empty());
    }
}
