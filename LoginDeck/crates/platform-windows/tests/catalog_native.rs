#![cfg(windows)]

use autologin_core::{ApplicationCatalog, DiscoverySource, Platform};
use platform_windows::{WindowsApplicationCatalog, WindowsLaunchTarget};
use std::collections::HashSet;

#[tokio::test]
async fn live_catalog_is_bounded_typed_unique_and_non_destructive() {
    let items = WindowsApplicationCatalog::new().discover().await.unwrap();
    assert!(items.len() <= 4096);
    let mut ids = HashSet::new();
    for item in &items {
        assert_eq!(item.platform, Platform::Windows);
        assert_eq!(item.discovery_source, DiscoverySource::Automatic);
        assert!(WindowsLaunchTarget::parse(&item.launch_target).is_ok());
        assert!(ids.insert(item.platform_application_id.to_lowercase()));
        assert_eq!(item.path_access_ref, None);
    }
    eprintln!("live catalog: {} validated items", items.len());
}
