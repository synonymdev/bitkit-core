//! Recovery paths mirror the identity, network and provider binding of each store.
use super::*;

type StoreDirectory = (String, Option<BoltzNetwork>, Option<String>, PathBuf);

pub(super) fn store_directories(root: &Path) -> Result<Vec<StoreDirectory>, BoltzError> {
    let mut stores = Vec::new();
    if !root.exists() {
        return Ok(stores);
    }
    for identity in child_names(root)? {
        let path = checked_identity_path(root, &identity, false)?;
        if path.join("swaps.sqlite3").exists() {
            stores.push((identity.clone(), None, None, path.clone()));
        }
        for name in child_names(&path)? {
            let network_path = path.join(&name);
            if !network_path.is_dir() {
                continue;
            }
            if name == "delivery" {
                reject_delivery_backup(&path)?;
            }
            let network = BoltzNetwork::from_str(&name)
                .filter(|network| network.as_str() == name)
                .ok_or_else(|| invalid("unrecognized directory under swap identity"))?;
            check_directory(&network_path)?;
            for provider in child_names(&network_path)? {
                let provider_path = checked_identity_path(&network_path, &provider, false)?;
                stores.push((
                    identity.clone(),
                    Some(network),
                    Some(provider),
                    provider_path,
                ));
                if stores.len() > MAX_IDENTITIES {
                    return Err(invalid("too many scoped stores"));
                }
            }
        }
        if stores.len() > MAX_IDENTITIES {
            return Err(invalid("too many scoped stores"));
        }
    }
    Ok(stores)
}

fn child_names(path: &Path) -> Result<Vec<String>, BoltzError> {
    std::fs::read_dir(path)
        .map_err(|e| invalid(&e.to_string()))?
        .map(|entry| {
            entry
                .map_err(|e| invalid(&e.to_string()))?
                .file_name()
                .into_string()
                .map_err(|_| invalid("non-UTF8 recovery directory"))
        })
        .collect()
}

fn check_directory(path: &Path) -> Result<(), BoltzError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(invalid("recovery entry is not a real directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(invalid(&error.to_string())),
    }
}

pub(super) fn validate_store_scope(identity: &IdentitySnapshot) -> Result<(), BoltzError> {
    let parts: Vec<_> = identity.store.identity_binding.split('|').collect();
    binding_parts(&identity.store.identity_binding)?;
    match (&identity.network, &identity.provider) {
        (None, None) => Ok(()),
        (Some(network), Some(provider)) if network.as_str() == parts[2] && provider == parts[1] => {
            Ok(())
        }
        _ => Err(invalid("directory scope does not match the store binding")),
    }
}

pub(super) fn snapshot_path(
    root: &Path,
    identity: &IdentitySnapshot,
) -> Result<PathBuf, BoltzError> {
    validate_store_scope(identity)?;
    let path = checked_identity_path(root, &identity.identity, true)?;
    match (&identity.network, &identity.provider) {
        (Some(network), Some(provider)) => {
            let network_path = path.join(network.as_str());
            check_directory(&network_path)?;
            checked_identity_path(&network_path, provider, true)
        }
        _ => Ok(path),
    }
}

pub(super) fn reject_delivery_backup(path: &Path) -> Result<(), BoltzError> {
    if std::fs::symlink_metadata(path.join("delivery")).is_ok() {
        return Err(invalid("durable delivery journals are not supported by logical backup or restore yet; preserve the complete private application storage"));
    }
    Ok(())
}

/// A restore must not create a preferred legacy path that hides a newer scoped store.
pub(super) fn validate_restore_layout(
    root: &Path,
    incoming: &[IdentitySnapshot],
) -> Result<(), BoltzError> {
    let targets = incoming
        .iter()
        .map(|identity| {
            Ok((
                identity.store.identity_binding.as_str(),
                snapshot_path(root, identity)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, BoltzError>>()?;
    for (_, _, _, existing_path) in store_directories(root)? {
        reject_delivery_backup(&existing_path)?;
        let existing =
            Store::snapshot_directory(&existing_path).map_err(super::super::api::bridge_error)?;
        if targets
            .get(existing.identity_binding.as_str())
            .is_some_and(|target| target != &existing_path)
        {
            return Err(invalid(
                "the same identity binding already exists in another recovery directory layout",
            ));
        }
    }
    Ok(())
}
