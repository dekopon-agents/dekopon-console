//! Ephemeral, bounded intake of broker-returned descriptors. Nothing is delivered or persisted.
use std::{collections::BTreeMap, fs::File, io::Read, os::fd::OwnedFd, sync::Mutex};

use dekopon_agent::attachment::{GeneratedAssetStore, GeneratedImage};
use dekopon_broker_protocol::NewAsset;
use dekopon_model::asset::BlobError;

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ITEMS: usize = 16;

#[derive(Default)]
pub(super) struct ConsoleAssets(Mutex<Inventory>);

#[derive(Default)]
struct Inventory {
    next_id: u64,
    bytes: usize,
    // Retained only for the lifetime of the agent leg. Do not expose a delivery API.
    items: BTreeMap<u64, Vec<u8>>,
}

impl GeneratedAssetStore for ConsoleAssets {
    fn register(
        &self,
        descriptor: OwnedFd,
        metadata: &NewAsset,
        _capability: &str,
        _invocation: &str,
    ) -> Result<u64, BlobError> {
        if metadata.bytes > MAX_BYTES as u64 {
            return Err(BlobError::TooLarge);
        }
        let len = metadata.bytes as usize; // bounded above by 64 MiB
        let mut inventory = self.0.lock().expect("console asset inventory poisoned");
        if inventory.items.len() >= MAX_ITEMS || inventory.bytes > MAX_BYTES - len {
            return Err(BlobError::Capacity);
        }
        let mut data = Vec::with_capacity(len.min(1024 * 1024));
        File::from(descriptor)
            .take((len as u64) + 1)
            .read_to_end(&mut data)?;
        if data.len() != len {
            return Err(BlobError::LengthChanged);
        }
        let id = inventory
            .next_id
            .checked_add(1)
            .ok_or(BlobError::Capacity)?;
        inventory.next_id = id;
        inventory.bytes += len;
        inventory.items.insert(id, data);
        Ok(id)
    }

    fn remove(&self, id: u64) -> Result<(), BlobError> {
        let mut inventory = self.0.lock().expect("console asset inventory poisoned");
        let data = inventory.items.remove(&id).ok_or(BlobError::Unknown)?;
        inventory.bytes -= data.len();
        Ok(())
    }

    fn send(&self, _id: u64) -> Result<Option<GeneratedImage>, BlobError> {
        // The console has no transport to send to. Never claim delivery.
        Err(BlobError::Disabled)
    }

    fn delivery_failed(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use dekopon_broker_protocol::AssetEncoding;

    #[test]
    fn bounds_and_removal() {
        let store = ConsoleAssets::default();
        let descriptor = || {
            let mut file = tempfile::tempfile().unwrap();
            std::io::Write::write_all(&mut file, b"test").unwrap();
            std::io::Seek::rewind(&mut file).unwrap();
            file.into()
        };
        let metadata = NewAsset {
            descriptor: 0,
            content_type: "text/plain".into(),
            encoding: AssetEncoding::Identity,
            bytes: 4,
            sha256: String::new(),
        };
        let id = store
            .register(descriptor(), &metadata, "test", "one")
            .unwrap();
        assert_eq!(id, 1);
        store.remove(id).unwrap();
        assert_eq!(store.0.lock().unwrap().bytes, 0);
        let mut oversized = metadata.clone();
        oversized.bytes = MAX_BYTES as u64 + 1;
        assert_eq!(
            store.register(descriptor(), &oversized, "test", "two"),
            Err(BlobError::TooLarge)
        );
        let mut incorrect = metadata;
        incorrect.bytes = 3;
        assert_eq!(
            store.register(descriptor(), &incorrect, "test", "three"),
            Err(BlobError::LengthChanged)
        );
        assert_eq!(store.0.lock().unwrap().items.len(), 0);
    }
}
