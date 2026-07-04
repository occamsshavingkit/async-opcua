use std::collections::HashMap;
use std::time::SystemTime;

pub type CacheKey = (Vec<u8>, Vec<u8>, Vec<u8>);

pub struct OcspCache {
    entries: HashMap<CacheKey, (Vec<u8>, SystemTime)>,
}

impl OcspCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &CacheKey) -> Option<Vec<u8>> {
        let now = SystemTime::now();
        self.entries
            .retain(|_, (_, next_update)| now < *next_update);
        self.entries.get(key).map(|(response, _)| response.clone())
    }

    pub fn insert(&mut self, key: CacheKey, response: Vec<u8>, next_update: SystemTime) {
        self.entries.insert(key, (response, next_update));
    }
}

impl Default for OcspCache {
    fn default() -> Self {
        Self::new()
    }
}
