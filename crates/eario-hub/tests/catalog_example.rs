//! The example catalog in `docs/catalog/catalog.json` is read by the types
//! the hub reads catalogs with, so the documentation cannot drift from them.

use eario_core::{CATALOG_FORMAT_VERSION, Catalog, is_repo, is_slug};
use eario_hub::verify::PublicKey;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The example, as the hub's GitHub client reads it: `serde_json` into `Catalog`.
const EXAMPLE: &str = include_str!("../../../docs/catalog/catalog.json");

/// What the example has where a catalog needs a key. It is not a key: the hub
/// refuses to preview an install from an entry that still has it
/// (`release_invalid_key`), which is what the documentation says.
const PLACEHOLDER: &str = "REPLACE-WITH-THE-KEY-LINE-OF-THE-EARIO-RELEASE-KEY";

#[test]
fn the_example_catalog_is_read_by_the_type_the_hub_uses() -> TestResult {
    let catalog: Catalog = serde_json::from_str(EXAMPLE)?;
    assert_eq!(catalog.format_version, CATALOG_FORMAT_VERSION);
    assert!(!catalog.name.is_empty());

    let slugs: Vec<_> = catalog.dashboards.iter().map(|entry| entry.slug.as_deref()).collect();
    assert_eq!(slugs, [Some("data-explorer"), Some("signal-lab")]);

    for entry in &catalog.dashboards {
        assert_eq!(entry.repo, "rtonneau/eario");
        assert!(is_repo(&entry.repo), "{}", entry.repo);
        assert!(entry.slug.as_deref().is_some_and(is_slug), "{:?}", entry.slug);
        assert!(!entry.name.is_empty());
        assert!(!entry.description.is_empty());
    }
    Ok(())
}

#[test]
fn the_example_keys_are_the_placeholder_until_replaced_and_the_hub_refuses_it() -> TestResult {
    let catalog: Catalog = serde_json::from_str(EXAMPLE)?;
    for entry in &catalog.dashboards {
        if entry.public_key == PLACEHOLDER {
            let error = PublicKey::from_base64(&entry.repo, &entry.public_key)
                .err()
                .ok_or("the placeholder must not be a key")?;
            assert_eq!(error.code(), "release_invalid_key");
        } else {
            // A real key line, once someone has put one in the example.
            PublicKey::from_base64(&entry.repo, &entry.public_key)?;
        }
    }
    // One key per repository: both entries name the same one.
    assert_eq!(catalog.dashboards[0].public_key, catalog.dashboards[1].public_key);
    Ok(())
}
